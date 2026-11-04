//! Editing `settings.json` without disturbing anything but `statusLine`
//! (spec §6).
//!
//! The file belongs to the user and to Claude Code, so nothing here
//! re-serializes it: key order, indentation, a BOM and CRLF line endings all
//! survive. An edit is one contiguous splice of the original text, and the
//! record of it (`old_text`, `new_text`) lets [`disable`] reverse it byte for
//! byte. If another writer has re-serialized the file in between,
//! [`disable`] falls back to restoring the `statusLine` member alone, which
//! is the most anyone can promise once another writer has touched the file.

use crate::target::{is_bridge_command, Target};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Every writer of `settings.json` inside one process takes this, so two
/// edits never interleave (`providers/default_env.rs` takes it too).
pub static SETTINGS_WRITE_LOCK: Mutex<()> = Mutex::new(());

const BACKUP_KEEP: usize = 5;
const ATTEMPTS: usize = 3;
const BOM: &str = "\u{feff}";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditRefused {
    InvalidJson,
    NotAnObject,
    DuplicateStatusLine,
    UnrecognizedStatusLine,
    ReadOnly,
    NotWritable(String),
    ChangedWhileEditing,
    /// The current command already runs the bridge.
    SelfWrap,
    /// Our own check of the result failed, so nothing was written.
    GuardFailed(String),
    Io(String),
}

impl std::fmt::Display for EditRefused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditRefused::InvalidJson => write!(f, "settings.json isn't valid JSON. Fix it, then try again."),
            EditRefused::NotAnObject => write!(f, "settings.json isn't a JSON object. Fix it, then try again."),
            EditRefused::DuplicateStatusLine => write!(
                f,
                "settings.json has more than one \"statusLine\" entry. Remove the extra one, then try again."
            ),
            EditRefused::UnrecognizedStatusLine => {
                write!(f, "The status line setting has a shape Switchboard doesn't recognize.")
            }
            EditRefused::ReadOnly => write!(f, "settings.json is read-only."),
            EditRefused::NotWritable(why) => write!(f, "settings.json can't be changed: {why}"),
            EditRefused::ChangedWhileEditing => {
                write!(f, "settings.json changed while Switchboard was editing it. Try again.")
            }
            EditRefused::SelfWrap => write!(f, "The status line already runs the Switchboard bridge."),
            EditRefused::GuardFailed(why) => write!(
                f,
                "Switchboard couldn't verify the change it was about to make, so it wrote nothing ({why})."
            ),
            EditRefused::Io(e) => write!(f, "Couldn't update settings.json: {e}"),
        }
    }
}

impl std::error::Error for EditRefused {}

impl From<io::Error> for EditRefused {
    fn from(e: io::Error) -> Self {
        EditRefused::Io(e.to_string())
    }
}

/// What enabling spliced in, enough to undo it exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnableRecord {
    /// The text the splice replaced (`""` for a pure insertion).
    pub old_text: String,
    /// The text the splice put there.
    pub new_text: String,
    /// The exact text of the original `statusLine` value, if there was one.
    pub original_value_text: Option<String>,
    /// The original command, `None` when absent or blank.
    pub original_command: Option<String>,
    pub file_was_created: bool,
    pub file_was_empty: bool,
    /// The command now in `settings.json`.
    pub installed_command: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisableOutcome {
    Restored,
    /// The command is no longer ours: the user's own change is left alone.
    ReleasedUserChanged,
}

/// The user's status line as it is now, read without changing anything, so
/// the target file can be written before the settings file is touched.
#[derive(Debug, Clone, PartialEq)]
pub struct OriginalStatusLine {
    pub file_exists: bool,
    pub present: bool,
    /// The exact text of the `statusLine` value.
    pub value_text: Option<String>,
    pub value: Option<Value>,
    /// Its `command`, which may be blank.
    pub command: Option<String>,
}

pub fn read_original(path: &Path) -> Result<OriginalStatusLine, EditRefused> {
    let real = resolve_real_path(path)?;
    let (text, file_exists) = match fs::read(&real) {
        Ok(b) => (String::from_utf8(b).map_err(|_| EditRefused::InvalidJson)?, true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (String::new(), false),
        Err(e) => return Err(e.into()),
    };
    let doc = parse_doc(&text)?;
    let Some(obj) = &doc.obj else {
        return Ok(OriginalStatusLine { file_exists, present: false, value_text: None, value: None, command: None });
    };
    Ok(match status_line(&text, obj)? {
        StatusLine::Absent => OriginalStatusLine { file_exists, present: false, value_text: None, value: None, command: None },
        StatusLine::Command { value_start, value_end, command, .. } => {
            let value_text = text[value_start..value_end].to_string();
            OriginalStatusLine {
                file_exists,
                present: true,
                value: serde_json::from_str(&value_text).ok(),
                value_text: Some(value_text),
                command: Some(command),
            }
        }
    })
}

/// A read-only view of `settings.json` for the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsState {
    pub real_path: PathBuf,
    pub exists: bool,
    pub sha256: String,
    pub status_line_present: bool,
    /// `statusLine.command` when the member has the shape Claude Code accepts.
    pub command: Option<String>,
    pub disable_all_hooks: bool,
    pub shared_warning: Option<String>,
}

// ---------------------------------------------------------------------------
// A small JSON scanner. The text is validated by `serde_json` first, so this
// only has to find spans in well-formed input.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Member {
    key: String,
    key_start: usize,
    value_start: usize,
    value_end: usize,
}

#[derive(Debug, Clone)]
struct Object {
    open: usize,
    close: usize,
    members: Vec<Member>,
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && matches!(b[i], b' ' | b'\t' | b'\r' | b'\n') {
        i += 1;
    }
    i
}

/// `start` is at an opening quote; returns the index just past the closing one.
fn string_end(b: &[u8], start: usize) -> Option<usize> {
    let mut i = start + 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

fn value_end(b: &[u8], start: usize) -> Option<usize> {
    match *b.get(start)? {
        b'"' => string_end(b, start),
        b'{' | b'[' => {
            let mut depth = 0usize;
            let mut i = start;
            while i < b.len() {
                match b[i] {
                    b'"' => {
                        i = string_end(b, i)?;
                        continue;
                    }
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return Some(i + 1);
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            None
        }
        _ => {
            let mut i = start;
            while i < b.len() && !matches!(b[i], b',' | b'}' | b']' | b' ' | b'\t' | b'\r' | b'\n') {
                i += 1;
            }
            Some(i)
        }
    }
}

/// `b[open]` is `{`.
fn scan_object(b: &[u8], open: usize) -> Option<Object> {
    let mut members = Vec::new();
    let mut i = skip_ws(b, open + 1);
    if *b.get(i)? == b'}' {
        return Some(Object { open, close: i, members });
    }
    loop {
        if *b.get(i)? != b'"' {
            return None;
        }
        let key_end = string_end(b, i)?;
        let key: String = serde_json::from_slice(&b[i..key_end]).ok()?;
        let mut j = skip_ws(b, key_end);
        if *b.get(j)? != b':' {
            return None;
        }
        j = skip_ws(b, j + 1);
        let end = value_end(b, j)?;
        members.push(Member { key, key_start: i, value_start: j, value_end: end });
        i = skip_ws(b, end);
        match *b.get(i)? {
            b',' => i = skip_ws(b, i + 1),
            b'}' => return Some(Object { open, close: i, members }),
            _ => return None,
        }
    }
}

// ---------------------------------------------------------------------------
// Analysis
// ---------------------------------------------------------------------------

struct Doc {
    /// `None` for an empty or whitespace-only file, which Claude Code treats as `{}`.
    obj: Option<Object>,
    value: Value,
    body_start: usize,
}

fn parse_doc(text: &str) -> Result<Doc, EditRefused> {
    let body_start = if text.starts_with(BOM) { BOM.len() } else { 0 };
    let body = &text[body_start..];
    if body.trim().is_empty() {
        return Ok(Doc { obj: None, value: Value::Object(Default::default()), body_start });
    }
    let value: Value = serde_json::from_str(body).map_err(|_| EditRefused::InvalidJson)?;
    if !value.is_object() {
        return Err(EditRefused::NotAnObject);
    }
    let open = skip_ws(text.as_bytes(), body_start);
    let obj = scan_object(text.as_bytes(), open)
        .ok_or_else(|| EditRefused::GuardFailed("could not locate the members".into()))?;
    Ok(Doc { obj: Some(obj), value, body_start })
}

#[derive(Debug, Clone)]
enum StatusLine {
    Absent,
    Command {
        value_start: usize,
        value_end: usize,
        cmd_start: usize,
        cmd_end: usize,
        command: String,
    },
}

fn json_string(token: &str) -> Option<String> {
    serde_json::from_str::<String>(token).ok()
}

fn status_line(text: &str, obj: &Object) -> Result<StatusLine, EditRefused> {
    let mut hits = obj.members.iter().filter(|m| m.key == "statusLine");
    let Some(member) = hits.next() else {
        return Ok(StatusLine::Absent);
    };
    if hits.next().is_some() {
        return Err(EditRefused::DuplicateStatusLine);
    }
    let b = text.as_bytes();
    if b[member.value_start] != b'{' {
        return Err(EditRefused::UnrecognizedStatusLine);
    }
    let inner = scan_object(b, member.value_start).ok_or(EditRefused::UnrecognizedStatusLine)?;
    let find = |name: &str| -> Result<Option<&Member>, EditRefused> {
        let mut it = inner.members.iter().filter(|m| m.key == name);
        let first = it.next();
        if it.next().is_some() {
            return Err(EditRefused::UnrecognizedStatusLine);
        }
        Ok(first)
    };
    let kind = find("type")?.ok_or(EditRefused::UnrecognizedStatusLine)?;
    if json_string(&text[kind.value_start..kind.value_end]).as_deref() != Some("command") {
        return Err(EditRefused::UnrecognizedStatusLine);
    }
    let cmd = find("command")?.ok_or(EditRefused::UnrecognizedStatusLine)?;
    let command = json_string(&text[cmd.value_start..cmd.value_end])
        .ok_or(EditRefused::UnrecognizedStatusLine)?;
    Ok(StatusLine::Command {
        value_start: member.value_start,
        value_end: member.value_end,
        cmd_start: cmd.value_start,
        cmd_end: cmd.value_end,
        command,
    })
}

fn eol_of(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// The leading whitespace of the first member's line, as one indent unit.
fn indent_of(text: &str, obj: &Object) -> String {
    let Some(first) = obj.members.first() else {
        return "  ".into();
    };
    let before = &text[..first.key_start];
    match before.rfind('\n') {
        Some(nl) => {
            let lead = &before[nl + 1..];
            if lead.chars().all(|c| c == ' ' || c == '\t') && !lead.is_empty() {
                lead.to_string()
            } else {
                "  ".into()
            }
        }
        None => "  ".into(),
    }
}

/// `"statusLine": { "type": "command", "command": … }`, laid out to match
/// the file.
fn member_text(command: &str, indent: &str, eol: &str) -> String {
    let token = serde_json::to_string(command).expect("a string always serializes");
    format!(
        "\"statusLine\": {{{eol}{indent}{indent}\"type\": \"command\",{eol}{indent}{indent}\"command\": {token}{eol}{indent}}}"
    )
}

fn whole_file(command: &str, eol: &str) -> String {
    format!("{{{eol}  {}{eol}}}{eol}", member_text(command, "  ", eol))
}

struct Planned {
    new_text: String,
    record: EnableRecord,
}

fn splice(text: &str, range: (usize, usize), replacement: &str) -> String {
    format!("{}{}{}", &text[..range.0], replacement, &text[range.1..])
}

fn plan_enable(
    text: &str,
    exists: bool,
    our_command: &str,
    bridge_path: &Path,
) -> Result<Planned, EditRefused> {
    let doc = parse_doc(text)?;
    let eol = eol_of(text);
    let mut record = EnableRecord {
        old_text: String::new(),
        new_text: String::new(),
        original_value_text: None,
        original_command: None,
        file_was_created: !exists,
        file_was_empty: exists && doc.obj.is_none(),
        installed_command: our_command.to_string(),
    };

    let (range, replacement) = match &doc.obj {
        None => {
            // Missing, empty or whitespace-only: write a minimal object. A BOM
            // stays where it is, outside the range.
            ((doc.body_start, text.len()), whole_file(our_command, eol))
        }
        Some(obj) => match status_line(text, obj)? {
            StatusLine::Command { value_start, value_end, cmd_start, cmd_end, command } => {
                if is_bridge_command(&command, bridge_path) {
                    return Err(EditRefused::SelfWrap);
                }
                record.original_value_text = Some(text[value_start..value_end].to_string());
                record.original_command = Some(command).filter(|c| !c.trim().is_empty());
                // Only the command string changes: `padding`,
                // `refreshInterval` and any other key stay exactly as written.
                (
                    (cmd_start, cmd_end),
                    serde_json::to_string(our_command).expect("a string always serializes"),
                )
            }
            StatusLine::Absent => {
                let indent = indent_of(text, obj);
                match obj.members.last() {
                    None => (
                        (obj.open + 1, obj.close),
                        format!("{eol}{indent}{}{eol}", member_text(our_command, &indent, eol)),
                    ),
                    Some(last) => (
                        (last.value_end, last.value_end),
                        format!(",{eol}{indent}{}", member_text(our_command, &indent, eol)),
                    ),
                }
            }
        },
    };
    record.old_text = text[range.0..range.1].to_string();
    record.new_text = replacement.clone();
    Ok(Planned { new_text: splice(text, range, &replacement), record })
}

/// Everything except `statusLine` must be untouched, and `statusLine` must
/// still be something Claude Code accepts. Guards against a scanner bug.
fn check_untouched_except_status_line(before: &str, after: &str) -> Result<(), EditRefused> {
    let strip = |text: &str| -> Result<Value, EditRefused> {
        let mut v = parse_doc(text)?.value;
        if let Some(map) = v.as_object_mut() {
            map.remove("statusLine");
        }
        Ok(v)
    };
    if strip(before)? != strip(after)? {
        return Err(EditRefused::GuardFailed("another setting would have changed".into()));
    }
    Ok(())
}

fn verify_enabled(before: &str, after: &str, our_command: &str) -> Result<(), EditRefused> {
    check_untouched_except_status_line(before, after)?;
    let doc = parse_doc(after)?;
    let obj = doc.obj.as_ref().ok_or_else(|| EditRefused::GuardFailed("no object".into()))?;
    match status_line(after, obj)? {
        StatusLine::Command { command, .. } if command == our_command => Ok(()),
        _ => Err(EditRefused::GuardFailed("the status line isn't what was intended".into())),
    }
}

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

fn sha_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn resolve_real_path(path: &Path) -> io::Result<PathBuf> {
    // An existing symlink is followed so the link survives the edit.
    match path.canonicalize() {
        Ok(real) => Ok(real),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
            let name = path
                .file_name()
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no file name"))?;
            Ok(parent.canonicalize()?.join(name))
        }
        Err(e) => Err(e),
    }
}

fn shared_warning(real: &Path) -> Option<String> {
    #[cfg(unix)]
    let linked = {
        use std::os::unix::fs::MetadataExt;
        fs::metadata(real).map(|m| m.nlink() > 1).unwrap_or(false)
    };
    #[cfg(not(unix))]
    let linked = false;
    let text = real.to_string_lossy();
    let synced = ["Mobile Documents", "Dropbox", "OneDrive", "Google Drive", "GoogleDrive"]
        .iter()
        .any(|marker| text.contains(marker));
    let in_repo = real.ancestors().skip(1).any(|a| a.join(".git").exists());
    (linked || synced || in_repo).then(|| {
        format!(
            "This file is shared through {}. Other computers that use it will lose their status line unless Switchboard is set up there too.",
            real.display()
        )
    })
}

/// Read `settings.json` for the app: where it really is, what its status line
/// is, whether hooks are disabled.
pub fn read_state(path: &Path) -> Result<SettingsState, EditRefused> {
    let real_path = resolve_real_path(path)?;
    let bytes = match fs::read(&real_path) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let exists = bytes.is_some();
    let bytes = bytes.unwrap_or_default();
    let text = String::from_utf8(bytes.clone()).map_err(|_| EditRefused::InvalidJson)?;
    let doc = parse_doc(&text)?;
    let (status_line_present, command) = match &doc.obj {
        None => (false, None),
        Some(obj) => {
            let present = obj.members.iter().any(|m| m.key == "statusLine");
            let command = match status_line(&text, obj) {
                Ok(StatusLine::Command { command, .. }) => Some(command),
                _ => None,
            };
            (present, command)
        }
    };
    Ok(SettingsState {
        shared_warning: shared_warning(&real_path),
        disable_all_hooks: doc.value.get("disableAllHooks") == Some(&Value::Bool(true)),
        sha256: sha_hex(&bytes),
        real_path,
        exists,
        status_line_present,
        command,
    })
}

enum Action {
    Write(String),
    Delete,
    Nothing,
}

/// Test hooks run just before the rename, one per file so parallel tests
/// never see each other's.
#[cfg(test)]
type RenameHooks = Vec<(PathBuf, Box<dyn Fn(&Path) + Send>)>;
#[cfg(test)]
static BEFORE_RENAME: Mutex<RenameHooks> = Mutex::new(Vec::new());

fn backup_prefix(real: &Path) -> String {
    format!("{}.switchboard-statusline-", real.file_name().unwrap_or_default().to_string_lossy())
}

/// Copy the current file next to itself and keep the newest five. The
/// `-anchor` backup and `default_env`'s `.switchboard-<ts>` backups never
/// match, so neither pruner can touch the other's files.
fn backup_and_prune(real: &Path) -> io::Result<()> {
    let ns = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let prefix = backup_prefix(real);
    let dir = real.parent().unwrap_or(Path::new("."));
    fs::copy(real, dir.join(format!("{prefix}{ns}")))?;
    let mut ours: Vec<(u128, PathBuf)> = fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let suffix = name.strip_prefix(&prefix)?;
            (!suffix.is_empty() && suffix.bytes().all(|b| b.is_ascii_digit()))
                .then(|| suffix.parse::<u128>().ok().map(|n| (n, e.path())))
                .flatten()
        })
        .collect();
    ours.sort();
    while ours.len() > BACKUP_KEEP {
        let (_, oldest) = ours.remove(0);
        let _ = fs::remove_file(oldest);
    }
    Ok(())
}

/// The anchor backup (spec §4.4): a byte copy taken just before enabling,
/// never pruned while a target is enabled, restorable by hand.
pub fn write_anchor(path: &Path) -> Result<Option<PathBuf>, EditRefused> {
    let real = resolve_real_path(path)?;
    if !real.exists() {
        return Ok(None);
    }
    let anchor = real.with_file_name(format!("{}.switchboard-statusline-anchor", real.file_name().unwrap_or_default().to_string_lossy()));
    fs::copy(&real, &anchor)?;
    Ok(Some(anchor))
}

/// Run `plan` against the current text and carry out what it decides, safely:
/// under the process-wide lock, with a backup, through a temp file in the same
/// directory, and with the content hash re-checked just before the rename
/// (mtime and length miss same-length edits on coarse-timestamp file systems).
fn edit(path: &Path, plan: &dyn Fn(Option<&str>) -> Result<Action, EditRefused>) -> Result<(), EditRefused> {
    let _guard = SETTINGS_WRITE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let real = resolve_real_path(path)?;
    if real.starts_with("/nix/store") {
        return Err(EditRefused::NotWritable("it is managed by Nix".into()));
    }
    for _ in 0..ATTEMPTS {
        let bytes = match fs::read(&real) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let text = match &bytes {
            Some(b) => Some(String::from_utf8(b.clone()).map_err(|_| EditRefused::InvalidJson)?),
            None => None,
        };
        let before_hash = sha_hex(bytes.as_deref().unwrap_or_default());
        let changed_since = |real: &Path| -> bool {
            let now = fs::read(real).ok();
            sha_hex(now.as_deref().unwrap_or_default()) != before_hash
        };

        match plan(text.as_deref())? {
            Action::Nothing => return Ok(()),
            Action::Delete => {
                if changed_since(&real) {
                    continue;
                }
                fs::remove_file(&real)?;
                return Ok(());
            }
            Action::Write(new_text) => {
                let meta = fs::metadata(&real).ok();
                if meta.as_ref().is_some_and(|m| m.permissions().readonly()) {
                    return Err(EditRefused::ReadOnly);
                }
                if meta.is_some() {
                    backup_and_prune(&real).map_err(|e| EditRefused::Io(format!("backup: {e}")))?;
                }
                let dir = real.parent().unwrap_or(Path::new("."));
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0);
                let tmp = dir.join(format!(
                    "{}.{}.{nanos:x}.tmp",
                    real.file_name().unwrap_or_default().to_string_lossy(),
                    std::process::id()
                ));
                let written = (|| -> io::Result<()> {
                    let mut file = fs::File::create(&tmp)?;
                    file.write_all(new_text.as_bytes())?;
                    file.sync_all()?;
                    drop(file);
                    // Keep the file's mode; a new file is private.
                    if let Some(m) = &meta {
                        fs::set_permissions(&tmp, m.permissions())?;
                    } else {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
                        }
                    }
                    Ok(())
                })();
                if let Err(e) = written {
                    let _ = fs::remove_file(&tmp);
                    return Err(EditRefused::NotWritable(e.to_string()));
                }
                #[cfg(test)]
                for (only, hook) in BEFORE_RENAME.lock().unwrap_or_else(|e| e.into_inner()).iter() {
                    if *only == real {
                        hook(&real);
                    }
                }
                if changed_since(&real) {
                    let _ = fs::remove_file(&tmp);
                    continue;
                }
                if let Err(e) = fs::rename(&tmp, &real) {
                    let _ = fs::remove_file(&tmp);
                    return Err(EditRefused::Io(e.to_string()));
                }
                return Ok(());
            }
        }
    }
    Err(EditRefused::ChangedWhileEditing)
}

// ---------------------------------------------------------------------------
// Enable, disable, restore
// ---------------------------------------------------------------------------

/// Point `statusLine.command` at `our_command`, remembering what was there.
pub fn enable(path: &Path, our_command: &str, bridge_path: &Path) -> Result<EnableRecord, EditRefused> {
    let outcome = std::cell::RefCell::new(None);
    edit(path, &|current| {
        let planned = plan_enable(current.unwrap_or(""), current.is_some(), our_command, bridge_path)?;
        verify_enabled(current.unwrap_or(""), &planned.new_text, our_command)?;
        *outcome.borrow_mut() = Some(planned.record);
        Ok(Action::Write(planned.new_text))
    })?;
    Ok(outcome.into_inner().expect("a successful edit records its splice"))
}

struct Undo<'a> {
    installed_command: &'a str,
    /// The splice to reverse exactly, when known.
    exact: Option<(&'a str, &'a str)>,
    original_value_text: Option<&'a str>,
    file_was_created: bool,
}

fn remove_member(text: &str, obj: &Object, index: usize) -> String {
    let m = &obj.members[index];
    let range = if obj.members.len() == 1 {
        (obj.open + 1, obj.close)
    } else if index > 0 {
        (obj.members[index - 1].value_end, m.value_end)
    } else {
        (m.key_start, obj.members[1].key_start)
    };
    splice(text, range, "")
}

fn undo(path: &Path, u: &Undo) -> Result<DisableOutcome, EditRefused> {
    let outcome = std::cell::Cell::new(DisableOutcome::ReleasedUserChanged);
    edit(path, &|current| {
        let Some(text) = current else {
            return Ok(Action::Nothing);
        };
        // Only undo what is still ours. A file that doesn't parse says
        // nothing about that, so it is an error the user can act on, not a
        // quiet "released".
        let doc = parse_doc(text)?;
        let Some(obj) = doc.obj.as_ref() else {
            return Ok(Action::Nothing);
        };
        let ours = matches!(
            status_line(text, obj),
            Ok(StatusLine::Command { ref command, .. }) if command == u.installed_command
        );
        if !ours {
            return Ok(Action::Nothing);
        }
        outcome.set(DisableOutcome::Restored);

        if let Some((old, new)) = u.exact {
            // We created the file and nothing has touched it since: delete it.
            if u.file_was_created && text == new {
                return Ok(Action::Delete);
            }
            if text.matches(new).count() == 1 {
                let restored = text.replacen(new, old, 1);
                verify_restored(text, &restored, u.original_value_text)?;
                return Ok(Action::Write(restored));
            }
        }
        // Another writer re-serialized the file: restore the member alone.
        let restored = match (&status_line(text, obj)?, u.original_value_text) {
            (StatusLine::Command { value_start, value_end, .. }, Some(original)) => {
                splice(text, (*value_start, *value_end), original)
            }
            (StatusLine::Command { .. }, None) => {
                let index = obj.members.iter().position(|m| m.key == "statusLine").expect("statusLine is present");
                remove_member(text, obj, index)
            }
            (StatusLine::Absent, _) => unreachable!("checked above"),
        };
        verify_restored(text, &restored, u.original_value_text)?;
        Ok(Action::Write(restored))
    })?;
    Ok(outcome.get())
}

fn verify_restored(before: &str, after: &str, original_value_text: Option<&str>) -> Result<(), EditRefused> {
    check_untouched_except_status_line(before, after)?;
    let doc = parse_doc(after)?;
    let current = doc.value.get("statusLine");
    match (current, original_value_text) {
        (None, None) => Ok(()),
        (Some(now), Some(original)) => {
            let original: Value =
                serde_json::from_str(original).map_err(|_| EditRefused::GuardFailed("original unreadable".into()))?;
            if *now == original {
                Ok(())
            } else {
                Err(EditRefused::GuardFailed("the restored status line differs from the original".into()))
            }
        }
        _ => Err(EditRefused::GuardFailed("the status line was not restored".into())),
    }
}

/// Put the original back, exactly, if the command is still ours.
pub fn disable(path: &Path, rec: &EnableRecord) -> Result<DisableOutcome, EditRefused> {
    undo(
        path,
        &Undo {
            installed_command: &rec.installed_command,
            // An adopted target has no splice record (empty `new_text`): that
            // would match everywhere, so restore at the member level instead.
            exact: (!rec.new_text.is_empty()).then_some((rec.old_text.as_str(), rec.new_text.as_str())),
            original_value_text: rec.original_value_text.as_deref(),
            file_was_created: rec.file_was_created,
        },
    )
}

/// `bridge --restore`: the same, from the target file alone (member-level
/// exact, since the splice record lives in the app's database).
pub fn restore(target: &Target) -> Result<DisableOutcome, EditRefused> {
    undo(
        Path::new(&target.settings_path),
        &Undo {
            installed_command: &target.installed_command,
            exact: None,
            original_value_text: target.original_value_text.as_deref(),
            file_was_created: target.file_was_created,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use filetime::{set_file_mtime, FileTime};
    use tempfile::tempdir;

    const CMD: &str = "'/Users/u/.claude-switchboard/statusline/bridge' --target 0123456789ab";

    fn bridge() -> PathBuf {
        PathBuf::from("/Users/u/.claude-switchboard/statusline/bridge")
    }

    fn put(dir: &Path, bytes: &[u8]) -> PathBuf {
        let p = dir.join("settings.json");
        fs::write(&p, bytes).unwrap();
        p
    }

    fn roundtrip(bytes: &[u8]) {
        let dir = tempdir().unwrap();
        let p = put(dir.path(), bytes);
        let rec = enable(&p, CMD, &bridge()).unwrap();
        let during = fs::read_to_string(&p).unwrap();
        assert!(during.contains("0123456789ab"), "the command is in place");
        assert_eq!(disable(&p, &rec).unwrap(), DisableOutcome::Restored);
        assert_eq!(fs::read(&p).unwrap(), bytes, "bytes after enable then disable");
    }

    #[test]
    fn golden_round_trip_for_every_fixture() {
        let owner = br#"{"agentPushNotifEnabled":true,"statusLine":{"type":"command","command":"bash ~/.claude/statusline.sh"},"theme":"dark"}"#;
        let fixtures: Vec<(&str, Vec<u8>)> = vec![
            ("owner compact", owner.to_vec()),
            ("owner with refreshInterval", br#"{"statusLine":{"type":"command","command":"bash ~/.claude/statusline.sh","refreshInterval":60}}"#.to_vec()),
            ("pretty, two spaces", b"{\n  \"a\": 1,\n  \"statusLine\": {\n    \"type\": \"command\",\n    \"command\": \"x\",\n    \"padding\": 2\n  },\n  \"z\": [1, 2]\n}\n".to_vec()),
            ("pretty, tabs", b"{\n\t\"a\": 1,\n\t\"statusLine\": {\n\t\t\"type\": \"command\",\n\t\t\"command\": \"x\"\n\t}\n}\n".to_vec()),
            ("crlf", b"{\r\n  \"a\": 1,\r\n  \"statusLine\": {\r\n    \"type\": \"command\",\r\n    \"command\": \"x\"\r\n  }\r\n}\r\n".to_vec()),
            ("bom", [b"\xEF\xBB\xBF".as_slice(), b"{\n  \"a\": 1,\n  \"statusLine\": {\"type\": \"command\", \"command\": \"x\"}\n}\n"].concat()),
            ("unicode and escaped quotes", "{\n  \"statusLine\": {\"type\": \"command\", \"command\": \"echo \\\"héllo ✓\\\" && printf '\\\\n'\"},\n  \"emoji\": \"😀\"\n}\n".as_bytes().to_vec()),
            ("absent key, pretty", b"{\n  \"a\": 1,\n  \"b\": {\"c\": [1, 2, 3]}\n}\n".to_vec()),
            ("absent key, compact", br#"{"a":1,"b":2}"#.to_vec()),
            ("absent key, crlf", b"{\r\n  \"a\": 1\r\n}\r\n".to_vec()),
            ("absent key, tabs", b"{\n\t\"a\": 1\n}\n".to_vec()),
            ("absent key, bom", [b"\xEF\xBB\xBF".as_slice(), b"{\n  \"a\": 1\n}\n"].concat()),
            ("empty object", b"{}".to_vec()),
            ("empty object, newline", b"{\n}\n".to_vec()),
            ("empty file", b"".to_vec()),
            ("whitespace only", b"  \n\n".to_vec()),
            ("statusLine with empty command", br#"{"statusLine":{"type":"command","command":""}}"#.to_vec()),
            ("no trailing newline", b"{\n  \"a\": 1\n}".to_vec()),
            ("statusLine first of several", br#"{"statusLine":{"type":"command","command":"x"},"a":1,"b":2}"#.to_vec()),
        ];
        for (name, bytes) in fixtures {
            let result = std::panic::catch_unwind(|| roundtrip(&bytes));
            assert!(result.is_ok(), "fixture failed: {name}");
        }
    }

    #[test]
    fn missing_file_is_created_then_deleted_on_disable() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("settings.json");
        let rec = enable(&p, CMD, &bridge()).unwrap();
        assert!(rec.file_was_created);
        let text = fs::read_to_string(&p).unwrap();
        assert!(serde_json::from_str::<Value>(&text).unwrap()["statusLine"]["command"] == CMD);
        assert_eq!(disable(&p, &rec).unwrap(), DisableOutcome::Restored);
        assert!(!p.exists(), "a file we created is removed again");
    }

    #[test]
    fn empty_file_is_restored_to_empty() {
        let dir = tempdir().unwrap();
        let p = put(dir.path(), b"");
        let rec = enable(&p, CMD, &bridge()).unwrap();
        assert!(rec.file_was_empty);
        disable(&p, &rec).unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"");
    }

    #[test]
    fn only_the_command_token_changes_and_other_keys_stay() {
        let dir = tempdir().unwrap();
        let original = r#"{"type":"command","command":"bash ~/.claude/statusline.sh","refreshInterval":60}"#;
        let p = put(dir.path(), format!(r#"{{"statusLine":{original}}}"#).as_bytes());
        let rec = enable(&p, CMD, &bridge()).unwrap();
        assert_eq!(rec.original_command.as_deref(), Some("bash ~/.claude/statusline.sh"));
        assert_eq!(rec.original_value_text.as_deref(), Some(original));
        let v: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["statusLine"]["command"], CMD);
        assert_eq!(v["statusLine"]["refreshInterval"], 60);
        assert_eq!(v["statusLine"]["type"], "command");
    }

    #[test]
    fn invalid_json_and_json_with_comments_are_refused_untouched() {
        for bad in [&b"{ not json"[..], b"{\n // comment\n \"a\": 1\n}", b"[1,2]", b"{\"a\":1,}"] {
            let dir = tempdir().unwrap();
            let p = put(dir.path(), bad);
            let err = enable(&p, CMD, &bridge()).unwrap_err();
            assert!(matches!(err, EditRefused::InvalidJson | EditRefused::NotAnObject), "{err:?}");
            assert_eq!(fs::read(&p).unwrap(), bad, "refused means untouched");
        }
    }

    #[test]
    fn unrecognized_status_line_shapes_are_refused() {
        let cases = [
            (r#"{"statusLine":{"type":"command","command":"a"},"statusLine":{"type":"command","command":"b"}}"#, EditRefused::DuplicateStatusLine),
            (r#"{"statusLine":"bash x"}"#, EditRefused::UnrecognizedStatusLine),
            (r#"{"statusLine":{"type":"static","command":"a"}}"#, EditRefused::UnrecognizedStatusLine),
            (r#"{"statusLine":{"type":"command","command":7}}"#, EditRefused::UnrecognizedStatusLine),
            (r#"{"statusLine":{"command":"a"}}"#, EditRefused::UnrecognizedStatusLine),
            (r#"{"statusLine":null}"#, EditRefused::UnrecognizedStatusLine),
        ];
        for (text, want) in cases {
            let dir = tempdir().unwrap();
            let p = put(dir.path(), text.as_bytes());
            assert_eq!(enable(&p, CMD, &bridge()).unwrap_err(), want, "{text}");
            assert_eq!(fs::read_to_string(&p).unwrap(), text);
        }
    }

    #[test]
    fn enable_on_an_already_ours_command_is_refused_as_self_wrap() {
        let dir = tempdir().unwrap();
        let p = put(dir.path(), format!(r#"{{"statusLine":{{"type":"command","command":{}}}}}"#, serde_json::to_string(CMD).unwrap()).as_bytes());
        assert_eq!(enable(&p, CMD, &bridge()).unwrap_err(), EditRefused::SelfWrap);
        // A command the user edited around ours is refused too.
        let wrapped = format!("{CMD}; echo extra");
        let p2 = put(dir.path(), format!(r#"{{"statusLine":{{"type":"command","command":{}}}}}"#, serde_json::to_string(&wrapped).unwrap()).as_bytes());
        assert_eq!(enable(&p2, "'/x/bridge' --target ffffffffffff", &bridge()).unwrap_err(), EditRefused::SelfWrap);
    }

    #[test]
    fn disable_after_the_user_changed_the_command_leaves_the_file_untouched() {
        let dir = tempdir().unwrap();
        let p = put(dir.path(), br#"{"statusLine":{"type":"command","command":"x"}}"#);
        let rec = enable(&p, CMD, &bridge()).unwrap();
        let theirs = r#"{"statusLine":{"type":"command","command":"my own"}}"#;
        fs::write(&p, theirs).unwrap();
        assert_eq!(disable(&p, &rec).unwrap(), DisableOutcome::ReleasedUserChanged);
        assert_eq!(fs::read_to_string(&p).unwrap(), theirs, "we never overwrite the user's own change");
    }

    #[test]
    fn disable_after_another_writer_reserialized_the_file_restores_the_member() {
        let dir = tempdir().unwrap();
        let p = put(dir.path(), b"{\n  \"a\": 1\n}\n");
        let rec = enable(&p, CMD, &bridge()).unwrap();
        // Claude Code rewrites the file in its own layout, adding a setting.
        let mut v: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
        v["theme"] = Value::String("dark".into());
        fs::write(&p, serde_json::to_string(&v).unwrap()).unwrap();
        assert_eq!(disable(&p, &rec).unwrap(), DisableOutcome::Restored);
        let after: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(after, serde_json::json!({"a": 1, "theme": "dark"}), "their new setting survives, ours is gone");
    }

    #[test]
    fn restore_from_the_target_alone_is_member_level_exact() {
        let dir = tempdir().unwrap();
        let p = put(dir.path(), br#"{"a":1,"statusLine":{"type":"command","command":"keep me","padding":1}}"#);
        let rec = enable(&p, CMD, &bridge()).unwrap();
        let target = Target {
            v: 1,
            config_home: dir.path().to_string_lossy().into(),
            settings_path: p.to_string_lossy().into(),
            settings_real_path: p.to_string_lossy().into(),
            original: None,
            original_value_text: rec.original_value_text.clone(),
            file_was_created: false,
            installed_command: CMD.into(),
            installed_at_ms: 0,
            state: crate::target::TargetState::Enabled,
            last_run_ms: None,
            released_by: None,
            released_at_ms: None,
        };
        assert_eq!(restore(&target).unwrap(), DisableOutcome::Restored);
        let v: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["statusLine"], serde_json::json!({"type":"command","command":"keep me","padding":1}));
        // Restoring twice is a no-op, not an error.
        assert_eq!(restore(&target).unwrap(), DisableOutcome::ReleasedUserChanged);
    }

    #[test]
    fn the_guard_trips_on_a_deliberately_bad_splice() {
        let before = r#"{"a":1,"statusLine":{"type":"command","command":"x"}}"#;
        // A splice that also altered "a".
        let after = format!(r#"{{"a":2,"statusLine":{{"type":"command","command":{}}}}}"#, serde_json::to_string(CMD).unwrap());
        assert!(matches!(verify_enabled(before, &after, CMD), Err(EditRefused::GuardFailed(_))));
        // A splice that left a status line Claude Code would reject.
        let bad = r#"{"a":1,"statusLine":{"type":"command","command":5}}"#;
        assert!(verify_enabled(before, bad, CMD).is_err());
        // A splice that broke the JSON.
        assert!(verify_enabled(before, r#"{"a":1,"statusLine":{"type":"command""#, CMD).is_err());
    }

    #[test]
    fn a_hash_mismatch_retries_then_errors_and_a_same_length_same_mtime_edit_is_caught() {
        let dir = tempdir().unwrap();
        let p = put(dir.path(), br#"{"a":"1111"}"#);
        let mtime = FileTime::from_last_modification_time(&fs::metadata(&p).unwrap());

        // The file is edited, in place and the same length, with its mtime
        // put back, every time just before the rename.
        let only = p.canonicalize().unwrap();
        BEFORE_RENAME.lock().unwrap().push((only.clone(), Box::new(move |real| {
            let now = fs::read_to_string(real).unwrap();
            let flipped = if now.contains("1111") { now.replace("1111", "2222") } else { now.replace("2222", "1111") };
            fs::write(real, flipped).unwrap();
            set_file_mtime(real, mtime).unwrap();
        })));
        let result = enable(&p, CMD, &bridge());
        BEFORE_RENAME.lock().unwrap().retain(|(path, _)| *path != only);

        assert_eq!(result.unwrap_err(), EditRefused::ChangedWhileEditing);
        let left: Vec<_> = fs::read_dir(dir.path()).unwrap().filter_map(|e| e.ok()).filter(|e| e.path().extension().is_some_and(|x| x == "tmp")).collect();
        assert!(left.is_empty(), "no temp file may be left behind");
    }

    #[test]
    fn a_single_concurrent_edit_is_retried_and_then_succeeds() {
        let dir = tempdir().unwrap();
        let p = put(dir.path(), br#"{"a":1}"#);
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let calls = std::sync::Arc::new(calls);
        let c = calls.clone();
        let only = p.canonicalize().unwrap();
        BEFORE_RENAME.lock().unwrap().push((only.clone(), Box::new(move |real| {
            if c.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                fs::write(real, br#"{"a":1,"b":2}"#).unwrap();
            }
        })));
        let rec = enable(&p, CMD, &bridge());
        BEFORE_RENAME.lock().unwrap().retain(|(path, _)| *path != only);
        rec.unwrap();
        let v: Value = serde_json::from_str(&fs::read_to_string(&p).unwrap()).unwrap();
        assert_eq!(v["b"], 2, "the concurrent change was re-read, not overwritten");
        assert!(v["statusLine"]["command"] == CMD);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_settings_keep_the_link_and_edit_the_target() {
        let dir = tempdir().unwrap();
        let dotfiles = dir.path().join("dotfiles");
        fs::create_dir(&dotfiles).unwrap();
        let real = dotfiles.join("settings.json");
        fs::write(&real, br#"{"a":1}"#).unwrap();
        let link = dir.path().join("settings.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();

        let rec = enable(&link, CMD, &bridge()).unwrap();
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link survives");
        assert!(fs::read_to_string(&real).unwrap().contains("0123456789ab"));
        disable(&link, &rec).unwrap();
        assert_eq!(fs::read(&real).unwrap(), br#"{"a":1}"#);
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn read_only_target_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let p = put(dir.path(), br#"{"a":1}"#);
        fs::set_permissions(&p, fs::Permissions::from_mode(0o444)).unwrap();
        assert_eq!(enable(&p, CMD, &bridge()).unwrap_err(), EditRefused::ReadOnly);
        assert_eq!(fs::read(&p).unwrap(), br#"{"a":1}"#);
    }

    #[cfg(unix)]
    #[test]
    fn hardlinked_files_and_git_repos_produce_a_shared_warning() {
        let dir = tempdir().unwrap();
        let p = put(dir.path(), br#"{"a":1}"#);
        assert!(read_state(&p).unwrap().shared_warning.is_none());
        fs::hard_link(&p, dir.path().join("settings.copy.json")).unwrap();
        assert!(read_state(&p).unwrap().shared_warning.unwrap().contains("shared through"));

        let repo = tempdir().unwrap();
        fs::create_dir(repo.path().join(".git")).unwrap();
        let q = put(repo.path(), br#"{"a":1}"#);
        assert!(read_state(&q).unwrap().shared_warning.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn file_mode_is_preserved_and_a_new_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let p = put(dir.path(), br#"{"a":1}"#);
        fs::set_permissions(&p, fs::Permissions::from_mode(0o640)).unwrap();
        enable(&p, CMD, &bridge()).unwrap();
        assert_eq!(fs::metadata(&p).unwrap().permissions().mode() & 0o777, 0o640);

        let fresh = dir.path().join("new").join("settings.json");
        fs::create_dir(fresh.parent().unwrap()).unwrap();
        enable(&fresh, CMD, &bridge()).unwrap();
        assert_eq!(fs::metadata(&fresh).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn backups_prune_to_five_and_never_touch_other_switchboard_backups_or_the_anchor() {
        let dir = tempdir().unwrap();
        let p = put(dir.path(), br#"{"a":1}"#);
        let anchor = write_anchor(&p).unwrap().unwrap();
        // Another writer's backup (`default_env` uses `.switchboard-<ts>`).
        let theirs = dir.path().join("settings.json.switchboard-1761580000");
        fs::write(&theirs, "theirs").unwrap();

        for _ in 0..8 {
            let rec = enable(&p, CMD, &bridge()).unwrap();
            disable(&p, &rec).unwrap();
        }
        let ours = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.strip_prefix("settings.json.switchboard-statusline-").is_some_and(|s| s.bytes().all(|b| b.is_ascii_digit()))
            })
            .count();
        assert_eq!(ours, BACKUP_KEEP);
        assert!(anchor.exists(), "the anchor is never pruned");
        assert!(theirs.exists(), "another writer's backups are not ours to prune");
    }

    #[test]
    fn the_anchor_is_a_byte_copy_taken_before_enabling() {
        let dir = tempdir().unwrap();
        let bytes = b"\xEF\xBB\xBF{\r\n  \"a\": 1\r\n}\r\n";
        let p = put(dir.path(), bytes);
        let anchor = write_anchor(&p).unwrap().unwrap();
        enable(&p, CMD, &bridge()).unwrap();
        assert_eq!(fs::read(anchor).unwrap(), bytes);
    }

    #[test]
    fn writes_serialize_on_the_process_wide_lock() {
        let dir = tempdir().unwrap();
        let p = std::sync::Arc::new(put(dir.path(), br#"{"a":1}"#));
        let handles: Vec<_> = (0..6)
            .map(|i| {
                let p = p.clone();
                std::thread::spawn(move || {
                    let cmd = format!("'/Users/u/.claude-switchboard/statusline/bridge' --target {i:012x}");
                    // Whatever interleaving, every operation either succeeds or
                    // is refused cleanly (the file is already ours, or changed).
                    match enable(&p, &cmd, &bridge()) {
                        Ok(rec) => {
                            let _ = disable(&p, &rec);
                        }
                        Err(EditRefused::SelfWrap | EditRefused::ChangedWhileEditing) => {}
                        Err(other) => panic!("unexpected: {other:?}"),
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        let text = fs::read_to_string(&*p).unwrap();
        serde_json::from_str::<Value>(&text).expect("the file is still valid JSON");
    }

    #[test]
    fn read_original_reports_the_status_line_without_changing_anything() {
        let dir = tempdir().unwrap();
        let text = r#"{"a":1,"statusLine":{"type":"command","command":"bash x","refreshInterval":60}}"#;
        let p = put(dir.path(), text.as_bytes());
        let o = read_original(&p).unwrap();
        assert!(o.file_exists && o.present);
        assert_eq!(o.command.as_deref(), Some("bash x"));
        assert_eq!(o.value_text.as_deref(), Some(r#"{"type":"command","command":"bash x","refreshInterval":60}"#));
        assert_eq!(o.value.unwrap()["refreshInterval"], 60);
        assert_eq!(fs::read_to_string(&p).unwrap(), text);

        let none = read_original(&dir.path().join("absent.json")).unwrap();
        assert!(!none.file_exists && !none.present && none.value_text.is_none());
        put(dir.path(), br#"{"a":1}"#);
        assert!(!read_original(&dir.path().join("settings.json")).unwrap().present);
    }

    #[test]
    fn read_state_reports_command_hooks_and_shape() {
        let dir = tempdir().unwrap();
        let p = put(dir.path(), br#"{"disableAllHooks":true,"statusLine":{"type":"command","command":"x"}}"#);
        let s = read_state(&p).unwrap();
        assert!(s.exists && s.status_line_present && s.disable_all_hooks);
        assert_eq!(s.command.as_deref(), Some("x"));

        let q = put(dir.path(), br#"{"statusLine":"weird"}"#);
        let s = read_state(&q).unwrap();
        assert!(s.status_line_present);
        assert_eq!(s.command, None, "an unrecognized shape is reported, not guessed at");

        let missing = read_state(&dir.path().join("nope").join("settings.json"));
        assert!(missing.is_err(), "no parent directory");
        let none = read_state(&dir.path().join("absent.json")).unwrap();
        assert!(!none.exists && !none.status_line_present);
    }
}
