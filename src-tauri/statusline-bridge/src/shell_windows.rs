//! Windows: which shell Claude Code runs a status line in, and the exact
//! command it hands that shell (spec §2.1 and §4.3).
//!
//! Everything here is pure: it reads an injected environment and file system,
//! so it compiles and is tested on every platform. Paths are plain `String`s
//! with Windows separators, because a `PathBuf` on macOS would not split on
//! `\`. The code that actually spawns a process is at the bottom, behind
//! `cfg(windows)`. `tests/bridge_cli_windows.rs` runs it against Git Bash,
//! PowerShell 7 and Windows PowerShell 5.1 (pass-through, exit codes, large
//! pipes, quoting parity with a direct run, the Job object). Not covered:
//! quoting pinned against Bun's own spawn (Bun cannot run in CI), OEM code
//! pages such as cp936, and AppLocker/WDAC.

use std::collections::HashMap;

/// The slice of the environment the resolvers read. Windows variable names
/// are case-insensitive, so names are stored upper-cased.
#[derive(Debug, Clone, Default)]
pub struct WinEnv {
    vars: HashMap<String, String>,
}

impl WinEnv {
    pub fn from_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        WinEnv {
            vars: pairs
                .into_iter()
                .map(|(k, v)| (k.to_ascii_uppercase(), v.to_string()))
                .collect(),
        }
    }

    pub fn from_process() -> Self {
        WinEnv {
            vars: std::env::vars_os()
                .filter_map(|(k, v)| Some((k.into_string().ok()?.to_ascii_uppercase(), v.into_string().ok()?)))
                .collect(),
        }
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.vars
            .get(&name.to_ascii_uppercase())
            .map(String::as_str)
            .filter(|v| !v.is_empty())
    }

    /// `CLAUDE_CODE_POWERSHELL_RESPECT_EXECUTION_POLICY` is set: PowerShell is
    /// then started without `-ExecutionPolicy Bypass`.
    pub fn respects_execution_policy(&self) -> bool {
        self.get("CLAUDE_CODE_POWERSHELL_RESPECT_EXECUTION_POLICY").is_some()
    }

    fn path_dirs(&self) -> Vec<&str> {
        self.get("PATH")
            .map(|p| p.split(';').collect())
            .unwrap_or_default()
    }
}

/// The file-system questions the resolvers ask.
pub trait Fs {
    fn is_file(&self, path: &str) -> bool;
}

/// The real file system.
pub struct RealFs;

impl Fs for RealFs {
    fn is_file(&self, path: &str) -> bool {
        std::path::Path::new(path).is_file()
    }
}

/// The shell Claude Code picks, with its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shell {
    Bash(String),
    PowerShell(String),
}

impl Shell {
    /// What the consent panel calls this shell.
    pub fn display_name(&self) -> String {
        match self {
            Shell::Bash(_) => "Git Bash".into(),
            Shell::PowerShell(path) if basename(path).eq_ignore_ascii_case("pwsh.exe") => "PowerShell 7".into(),
            Shell::PowerShell(_) => "Windows PowerShell".into(),
        }
    }
}

/// The program and arguments Claude Code uses to run a status-line command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: String,
    pub args: Vec<String>,
}

/// One argument as libuv's `quote_cmd_arg` (src/win/process.c) writes it into a
/// Windows command line. Node and Bun build theirs with that routine, so this
/// is what Claude Code hands a shell.
///
/// It is not Rust's std quoting: std wraps an argument only when it holds a
/// space or tab, libuv also wraps when it holds a `"`. For `"true"` that is
/// the difference between a command Git Bash runs and one it rejects with an
/// unterminated quote.
pub fn quote_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "\"\"".into();
    }
    if !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    if !arg.contains(['"', '\\']) {
        return format!("\"{arg}\"");
    }
    // Walk backwards, doubling every backslash that ends up before a quote
    // (or before the closing quote) and escaping each quote, then reverse.
    let mut reversed = String::with_capacity(arg.len() + 8);
    let mut before_quote = true;
    for c in arg.chars().rev() {
        reversed.push(c);
        if before_quote && c == '\\' {
            reversed.push('\\');
        } else if c == '"' {
            before_quote = true;
            reversed.push('\\');
        } else {
            before_quote = false;
        }
    }
    let body: String = reversed.chars().rev().collect();
    format!("\"{body}\"")
}

/// `inv` as a [`std::process::Command`] whose command line is written with
/// [`quote_arg`] (`raw_arg`), not std's quoting.
#[cfg(windows)]
pub fn windows_command(inv: &Invocation) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    let mut command = std::process::Command::new(&inv.program);
    for arg in &inv.args {
        command.raw_arg(quote_arg(arg));
    }
    command
}

/// How `shell` is started to run `original`: bash gets `-c <command>`, and
/// PowerShell gets its flags (ending in `-Command`) then the command text.
pub fn invocation(shell: &Shell, original: &str, respect_execution_policy: bool) -> Invocation {
    match shell {
        Shell::Bash(bash) => Invocation {
            program: bash.clone(),
            args: vec!["-c".into(), bash_command(original)],
        },
        Shell::PowerShell(ps) => {
            let (mut args, text) = powershell_command(original, respect_execution_policy);
            args.push(text);
            Invocation { program: ps.clone(), args }
        }
    }
}

fn join(dir: &str, rest: &str) -> String {
    format!("{}\\{}", dir.trim_end_matches(['\\', '/']), rest)
}

fn basename(path: &str) -> &str {
    path.rsplit(['\\', '/']).next().unwrap_or(path)
}

fn dirname(path: &str) -> Option<&str> {
    path.rfind(['\\', '/']).map(|i| &path[..i])
}

/// `git.exe` and friends found on `PATH`. The current directory is skipped, as
/// Claude Code skips it, so a planted `git.exe` next to the project is not
/// picked up.
fn find_on_path(name: &str, env: &WinEnv, fs: &dyn Fs) -> Option<String> {
    env.path_dirs()
        .into_iter()
        .map(str::trim)
        .filter(|d| !d.is_empty() && *d != ".")
        .map(|d| join(d, &format!("{name}.exe")))
        .find(|candidate| fs.is_file(candidate))
}

/// Claude Code's `x()`: Git for Windows' bash.
///
/// 1. `CLAUDE_CODE_GIT_BASH_PATH`, but only when its file name is `bash.exe`,
///    `sh.exe`, `bash` or `sh` and the file exists;
/// 2. `C:\Program Files\Git\bin\bash.exe`;
/// 3. the same under `Program Files (x86)`;
/// 4. `git` on `PATH`, then `..\..\bin\bash.exe` from it.
pub fn resolve_git_bash(env: &WinEnv, fs: &dyn Fs) -> Option<String> {
    if let Some(configured) = env.get("CLAUDE_CODE_GIT_BASH_PATH") {
        let name = basename(configured).to_ascii_lowercase();
        if matches!(name.as_str(), "bash.exe" | "sh.exe" | "bash" | "sh") && fs.is_file(configured) {
            return Some(configured.to_string());
        }
    }
    for fixed in [
        r"C:\Program Files\Git\bin\bash.exe",
        r"C:\Program Files (x86)\Git\bin\bash.exe",
    ] {
        if fs.is_file(fixed) {
            return Some(fixed.to_string());
        }
    }
    let git = find_on_path("git", env, fs)?;
    let install_root = dirname(dirname(&git)?)?;
    let bash = join(install_root, r"bin\bash.exe");
    fs.is_file(&bash).then_some(bash)
}

/// Claude Code's `oy()`: PowerShell 7, then Windows PowerShell.
///
/// `pwsh` on `PATH`, `%ProgramFiles%\PowerShell\7\pwsh.exe`, the WindowsApps
/// app-execution link, `%USERPROFILE%\.dotnet\tools\pwsh.exe`, `powershell` on
/// `PATH`, and finally `%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe`.
pub fn resolve_powershell(env: &WinEnv, fs: &dyn Fs) -> Option<String> {
    if let Some(found) = find_on_path("pwsh", env, fs) {
        return Some(found);
    }
    let under = |var: &str, rest: &str| env.get(var).map(|dir| join(dir, rest));
    let candidates = [
        under("ProgramFiles", r"PowerShell\7\pwsh.exe"),
        under("LOCALAPPDATA", r"Microsoft\WindowsApps\pwsh.exe"),
        under("USERPROFILE", r".dotnet\tools\pwsh.exe"),
    ];
    if let Some(found) = candidates.into_iter().flatten().find(|c| fs.is_file(c)) {
        return Some(found);
    }
    if let Some(found) = find_on_path("powershell", env, fs) {
        return Some(found);
    }
    under("SystemRoot", r"System32\WindowsPowerShell\v1.0\powershell.exe").filter(|c| fs.is_file(c))
}

/// Claude Code's `OG()`: bash when Git Bash exists, PowerShell otherwise.
pub fn resolve_shell(env: &WinEnv, fs: &dyn Fs) -> Option<Shell> {
    match resolve_git_bash(env, fs) {
        Some(bash) => Some(Shell::Bash(bash)),
        None => resolve_powershell(env, fs).map(Shell::PowerShell),
    }
}

/// Every shell Claude Code could pick on this machine, for the preflight:
/// installing Git later switches Claude Code from PowerShell to bash, so both
/// have to work when both exist.
pub fn candidate_shells(env: &WinEnv, fs: &dyn Fs) -> Vec<Shell> {
    let mut shells = Vec::new();
    if let Some(bash) = resolve_git_bash(env, fs) {
        shells.push(Shell::Bash(bash));
    }
    if let Some(ps) = resolve_powershell(env, fs) {
        shells.push(Shell::PowerShell(ps));
    }
    shells
}

/// Claude Code's `apr()`: a command whose first word ends in `.sh` gets
/// `bash ` prepended, so a bare script path runs.
pub fn bash_command(original: &str) -> String {
    let first = original.split_whitespace().next().unwrap_or("");
    if first.ends_with(".sh") {
        format!("bash {original}")
    } else {
        original.to_string()
    }
}

const POWERSHELL_EPILOGUE: &str = "\n; if (-not $?) { if ($LASTEXITCODE -eq 2) { exit 2 } }";

/// Claude Code's `ZUe()`, as far as it could be read: a lone `.ps1` path.
/// ASSUMPTION: the binary's definition was not resolved; this is the narrowest
/// reading of "bare script path". A wrong guess only changes whether the
/// epilogue is appended to a command that is already just a script.
fn is_bare_script_path(command: &str) -> bool {
    let trimmed = command.trim();
    !trimmed.contains(char::is_whitespace) && trimmed.to_ascii_lowercase().ends_with(".ps1")
}

/// The PowerShell argument vector and command string for `original`: flags
/// first (ending in `-Command`), then the single command argument.
///
/// `respect_execution_policy` mirrors `CLAUDE_CODE_POWERSHELL_RESPECT_EXECUTION_POLICY`.
pub fn powershell_command(original: &str, respect_execution_policy: bool) -> (Vec<String>, String) {
    let mut flags = vec!["-NoProfile".to_string(), "-NonInteractive".to_string()];
    if !respect_execution_policy {
        flags.push("-ExecutionPolicy".into());
        flags.push("Bypass".into());
    }
    flags.push("-Command".into());

    // Claude Code's `yEr()`.
    let mut command = original.replace("${CLAUDE_PROJECT_DIR}", "${env:CLAUDE_PROJECT_DIR}");
    if !is_bare_script_path(original) {
        command.push_str(POWERSHELL_EPILOGUE);
    }
    (flags, command)
}

#[cfg(windows)]
mod spawn {
    use super::*;
    use std::io::Write;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Stdio};
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcessId, OpenProcess, WaitForSingleObject, INFINITE, PROCESS_SYNCHRONIZE,
    };

    /// A Job object that kills everything in it when its last handle closes.
    /// Held for the life of the bridge process: when the bridge dies, so does
    /// the original's whole tree.
    pub struct Job(HANDLE);

    impl Job {
        pub fn new() -> std::io::Result<Job> {
            // SAFETY: plain Win32 calls with valid arguments; the handle is
            // closed in `Drop`.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return Err(std::io::Error::last_os_error());
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if ok == 0 {
                    let err = std::io::Error::last_os_error();
                    CloseHandle(handle);
                    return Err(err);
                }
                Ok(Job(handle))
            }
        }

        pub fn assign(&self, child: &Child) -> std::io::Result<()> {
            // SAFETY: both handles are valid for the duration of the call.
            let ok = unsafe { AssignProcessToJobObject(self.0, child.as_raw_handle() as HANDLE) };
            if ok == 0 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    impl Drop for Job {
        fn drop(&mut self) {
            // SAFETY: the handle came from `CreateJobObjectW` and is closed once.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// Our parent's process id, from a toolhelp snapshot.
    fn parent_pid() -> Option<u32> {
        // SAFETY: the snapshot handle is closed before returning; the entry
        // struct is initialised with its size as the API requires.
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot as isize == -1 {
                return None;
            }
            let me = GetCurrentProcessId();
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = None;
            let mut more = Process32FirstW(snapshot, &mut entry) != 0;
            while more {
                if entry.th32ProcessID == me {
                    found = Some(entry.th32ParentProcessID);
                    break;
                }
                more = Process32NextW(snapshot, &mut entry) != 0;
            }
            CloseHandle(snapshot);
            found
        }
    }

    /// Exit when the parent does. Killing the shell may not kill us, and our
    /// exit closes the Job, which kills the original's tree.
    fn exit_with_parent() {
        let Some(pid) = parent_pid() else { return };
        std::thread::spawn(move || {
            // SAFETY: the handle is used only on this thread and closed after the wait.
            unsafe {
                let handle = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
                if handle.is_null() {
                    return;
                }
                WaitForSingleObject(handle, INFINITE);
                CloseHandle(handle);
            }
            std::process::exit(1);
        });
    }

    /// Test seam (feature `force-shell`): `SWITCHBOARD_TEST_SHELL` names the
    /// shell to use, as `bash|<path>` or `powershell|<path>`.
    #[cfg(feature = "force-shell")]
    fn forced_shell() -> Option<Shell> {
        let value = std::env::var("SWITCHBOARD_TEST_SHELL").ok()?;
        match value.split_once('|')? {
            ("bash", path) => Some(Shell::Bash(path.to_string())),
            ("powershell", path) => Some(Shell::PowerShell(path.to_string())),
            _ => None,
        }
    }

    #[cfg(not(feature = "force-shell"))]
    fn forced_shell() -> Option<Shell> {
        None
    }

    pub fn spawn_original(
        original: &str,
        stdin_bytes: &[u8],
        depth_env: &str,
    ) -> std::io::Result<Child> {
        let env = WinEnv::from_process();
        let shell = forced_shell().or_else(|| resolve_shell(&env, &RealFs)).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "neither Git Bash nor PowerShell was found",
            )
        })?;
        let invocation = invocation(&shell, original, env.respects_execution_policy());
        let mut command = windows_command(&invocation);
        // The command line is written the way libuv writes it (`quote_arg`),
        // which is how Claude Code's runtime builds it. The spec's corpus
        // pinned against Bun itself cannot run here; the unit tests pin
        // `quote_arg` to libuv's documented examples, and the integration tests
        // run awkward originals through the real shells.
        let mut child = command
            .env(depth_env, "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()?;
        if let Ok(job) = Job::new() {
            if job.assign(&child).is_ok() {
                // Leak on purpose: the handle must live until the process
                // ends, and the OS closes it then.
                std::mem::forget(job);
            }
        }
        exit_with_parent();
        if let Some(mut stdin) = child.stdin.take() {
            let bytes = stdin_bytes.to_vec();
            std::thread::spawn(move || {
                let _ = stdin.write_all(&bytes);
            });
        }
        Ok(child)
    }
}

#[cfg(windows)]
pub use spawn::{spawn_original, Job};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Case-insensitive, separator-insensitive, like NTFS.
    struct FakeFs(HashSet<String>);

    impl FakeFs {
        fn new(files: &[&str]) -> Self {
            FakeFs(files.iter().map(|f| norm(f)).collect())
        }
    }

    fn norm(p: &str) -> String {
        p.replace('/', "\\").to_ascii_lowercase()
    }

    impl Fs for FakeFs {
        fn is_file(&self, path: &str) -> bool {
            self.0.contains(&norm(path))
        }
    }

    fn env(pairs: &[(&str, &str)]) -> WinEnv {
        WinEnv::from_pairs(pairs.iter().copied())
    }

    const SYS: (&str, &str) = ("SystemRoot", r"C:\Windows");
    const PS51: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";

    #[test]
    fn git_installed_resolves_to_its_bash_and_wins_over_powershell() {
        let fs = FakeFs::new(&[r"C:\Program Files\Git\bin\bash.exe", r"C:\Program Files\PowerShell\7\pwsh.exe"]);
        let e = env(&[SYS, ("ProgramFiles", r"C:\Program Files")]);
        assert_eq!(resolve_shell(&e, &fs), Some(Shell::Bash(r"C:\Program Files\Git\bin\bash.exe".into())));
    }

    #[test]
    fn the_x86_git_is_second_choice() {
        let fs = FakeFs::new(&[r"C:\Program Files (x86)\Git\bin\bash.exe"]);
        assert_eq!(
            resolve_git_bash(&env(&[]), &fs),
            Some(r"C:\Program Files (x86)\Git\bin\bash.exe".into())
        );
    }

    #[test]
    fn git_on_path_leads_to_bin_bash_two_levels_up() {
        let fs = FakeFs::new(&[r"D:\Tools\Git\cmd\git.exe", r"D:\Tools\Git\bin\bash.exe"]);
        let e = env(&[("Path", r"C:\Windows;D:\Tools\Git\cmd")]);
        assert_eq!(resolve_git_bash(&e, &fs), Some(r"D:\Tools\Git\bin\bash.exe".into()));
    }

    #[test]
    fn git_on_path_without_a_bash_beside_it_is_not_git_bash() {
        let fs = FakeFs::new(&[r"D:\Tools\Git\cmd\git.exe"]);
        let e = env(&[("PATH", r"D:\Tools\Git\cmd")]);
        assert_eq!(resolve_git_bash(&e, &fs), None);
    }

    #[test]
    fn the_current_directory_is_skipped_on_path() {
        // A `git.exe` in the cwd must not be found through a `.` entry.
        let fs = FakeFs::new(&[r".\git.exe", r"bin\bash.exe"]);
        let e = env(&[("PATH", r".;")]);
        assert_eq!(resolve_git_bash(&e, &fs), None);
    }

    #[test]
    fn a_configured_git_bash_path_must_be_a_shell_and_exist() {
        let good = r"E:\Portable\Git\usr\bin\bash.exe";
        let fs = FakeFs::new(&[good, r"E:\Portable\notes.txt"]);
        assert_eq!(
            resolve_git_bash(&env(&[("CLAUDE_CODE_GIT_BASH_PATH", good)]), &fs),
            Some(good.into())
        );
        // Not a shell by name: ignored, and nothing else is installed.
        assert_eq!(
            resolve_git_bash(&env(&[("CLAUDE_CODE_GIT_BASH_PATH", r"E:\Portable\notes.txt")]), &fs),
            None
        );
        // A shell by name that does not exist: ignored.
        assert_eq!(
            resolve_git_bash(&env(&[("CLAUDE_CODE_GIT_BASH_PATH", r"E:\Gone\bash.exe")]), &fs),
            None
        );
    }

    #[test]
    fn a_configured_path_pointing_at_a_non_bash_file_falls_through_to_the_standard_install() {
        let fs = FakeFs::new(&[r"E:\x\python.exe", r"C:\Program Files\Git\bin\bash.exe"]);
        assert_eq!(
            resolve_git_bash(&env(&[("CLAUDE_CODE_GIT_BASH_PATH", r"E:\x\python.exe")]), &fs),
            Some(r"C:\Program Files\Git\bin\bash.exe".into())
        );
    }

    #[test]
    fn without_git_pwsh_7_is_preferred_over_windows_powershell() {
        let fs = FakeFs::new(&[r"C:\Program Files\PowerShell\7\pwsh.exe", PS51]);
        let e = env(&[SYS, ("ProgramFiles", r"C:\Program Files")]);
        assert_eq!(
            resolve_shell(&e, &fs),
            Some(Shell::PowerShell(r"C:\Program Files\PowerShell\7\pwsh.exe".into()))
        );
    }

    #[test]
    fn only_windows_powershell_5_1_is_the_last_resort() {
        let fs = FakeFs::new(&[PS51]);
        assert_eq!(resolve_shell(&env(&[SYS]), &fs), Some(Shell::PowerShell(PS51.into())));
    }

    #[test]
    fn powershell_candidates_are_tried_in_the_documented_order() {
        let fs = FakeFs::new(&[
            r"C:\Users\me\.dotnet\tools\pwsh.exe",
            r"C:\Users\me\AppData\Local\Microsoft\WindowsApps\pwsh.exe",
            PS51,
        ]);
        let e = env(&[
            SYS,
            ("LOCALAPPDATA", r"C:\Users\me\AppData\Local"),
            ("USERPROFILE", r"C:\Users\me"),
        ]);
        // WindowsApps comes before the dotnet tool.
        assert_eq!(
            resolve_powershell(&e, &fs),
            Some(r"C:\Users\me\AppData\Local\Microsoft\WindowsApps\pwsh.exe".into())
        );
        let fs = FakeFs::new(&[r"C:\Users\me\.dotnet\tools\pwsh.exe", PS51]);
        assert_eq!(
            resolve_powershell(&e, &fs),
            Some(r"C:\Users\me\.dotnet\tools\pwsh.exe".into())
        );
    }

    #[test]
    fn pwsh_on_path_beats_every_fixed_location() {
        let fs = FakeFs::new(&[r"D:\ps\pwsh.exe", r"C:\Program Files\PowerShell\7\pwsh.exe"]);
        let e = env(&[SYS, ("ProgramFiles", r"C:\Program Files"), ("PATH", r"D:\ps")]);
        assert_eq!(resolve_powershell(&e, &fs), Some(r"D:\ps\pwsh.exe".into()));
    }

    #[test]
    fn windows_powershell_on_path_beats_the_system_directory_copy() {
        let fs = FakeFs::new(&[r"D:\old\powershell.exe", PS51]);
        let e = env(&[SYS, ("PATH", r"D:\old")]);
        assert_eq!(resolve_powershell(&e, &fs), Some(r"D:\old\powershell.exe".into()));
    }

    #[test]
    fn no_shell_at_all_resolves_to_none() {
        assert_eq!(resolve_shell(&env(&[SYS]), &FakeFs::new(&[])), None);
    }

    #[test]
    fn the_preflight_tests_both_shells_when_both_exist() {
        let fs = FakeFs::new(&[r"C:\Program Files\Git\bin\bash.exe", PS51]);
        let shells = candidate_shells(&env(&[SYS]), &fs);
        assert_eq!(
            shells,
            vec![
                Shell::Bash(r"C:\Program Files\Git\bin\bash.exe".into()),
                Shell::PowerShell(PS51.into())
            ]
        );
    }

    #[test]
    fn env_names_are_case_insensitive_and_empty_values_are_absent() {
        let e = env(&[("systemroot", r"C:\Windows"), ("Path", "")]);
        assert_eq!(e.get("SYSTEMROOT"), Some(r"C:\Windows"));
        assert_eq!(e.get("PATH"), None);
    }

    #[test]
    fn a_script_path_gets_bash_prepended() {
        assert_eq!(bash_command("~/.claude/statusline.sh"), "bash ~/.claude/statusline.sh");
        assert_eq!(bash_command("/c/x/line.sh --flag"), "bash /c/x/line.sh --flag");
        assert_eq!(bash_command("  ~/s.sh"), "bash   ~/s.sh");
    }

    #[test]
    fn other_commands_are_left_alone_for_bash() {
        assert_eq!(bash_command("node ~/line.js"), "node ~/line.js");
        assert_eq!(bash_command("echo done.sh"), "echo done.sh");
        assert_eq!(bash_command(""), "");
    }

    #[test]
    fn powershell_gets_the_documented_flags_and_epilogue() {
        let (flags, command) = powershell_command("node ~/line.js", false);
        assert_eq!(
            flags,
            ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command"]
        );
        assert_eq!(command, "node ~/line.js\n; if (-not $?) { if ($LASTEXITCODE -eq 2) { exit 2 } }");
    }

    #[test]
    fn powershell_keeps_the_execution_policy_when_asked_to_respect_it() {
        let (flags, _) = powershell_command("node ~/line.js", true);
        assert_eq!(flags, ["-NoProfile", "-NonInteractive", "-Command"]);
    }

    #[test]
    fn powershell_rewrites_the_project_dir_variable() {
        let (_, command) = powershell_command("& '${CLAUDE_PROJECT_DIR}/line.ps1' -x", false);
        assert!(command.starts_with("& '${env:CLAUDE_PROJECT_DIR}/line.ps1' -x"));
        assert!(!command.contains("${CLAUDE_PROJECT_DIR}"));
    }

    #[test]
    fn a_lone_script_path_gets_no_epilogue() {
        let (_, command) = powershell_command(r"C:\tools\line.ps1", false);
        assert_eq!(command, r"C:\tools\line.ps1");
    }

    #[test]
    fn a_script_with_arguments_still_gets_the_epilogue() {
        let (_, command) = powershell_command(r"C:\tools\line.ps1 -Verbose", false);
        assert!(command.ends_with("exit 2 } }"));
    }

    #[test]
    fn invocation_for_bash_wraps_the_command_after_dash_c() {
        let bash = Shell::Bash(r"C:\Program Files\Git\bin\bash.exe".into());
        let script = invocation(&bash, "~/.claude/statusline.sh --x", false);
        assert_eq!(script.program, r"C:\Program Files\Git\bin\bash.exe");
        assert_eq!(script.args, ["-c", "bash ~/.claude/statusline.sh --x"]);
        let plain = invocation(&bash, "node ~/line.js", false);
        assert_eq!(plain.args, ["-c", "node ~/line.js"]);
    }

    #[test]
    fn invocation_for_powershell_is_the_flags_then_the_command_with_epilogue() {
        let ps = Shell::PowerShell(PS51.into());
        let inv = invocation(&ps, "node ~/line.js", false);
        assert_eq!(inv.program, PS51);
        assert_eq!(
            inv.args,
            [
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                "node ~/line.js\n; if (-not $?) { if ($LASTEXITCODE -eq 2) { exit 2 } }",
            ]
        );
        let respecting = invocation(&ps, "x", true);
        assert!(!respecting.args.iter().any(|a| a == "Bypass"));
    }

    #[test]
    fn display_names_tell_the_three_shells_apart() {
        assert_eq!(Shell::Bash(r"C:\Git\bin\bash.exe".into()).display_name(), "Git Bash");
        assert_eq!(
            Shell::PowerShell(r"C:\Program Files\PowerShell\7\pwsh.exe".into()).display_name(),
            "PowerShell 7"
        );
        assert_eq!(Shell::PowerShell(r"D:\PS\PWSH.EXE".into()).display_name(), "PowerShell 7");
        assert_eq!(Shell::PowerShell(PS51.into()).display_name(), "Windows PowerShell");
    }

    /// The examples in libuv's own comment on `quote_cmd_arg` (src/win/process.c),
    /// the routine Node and Bun use to build a Windows command line, plus the
    /// case that matters here: quotes with no whitespace.
    #[test]
    fn quote_arg_matches_libuv() {
        let cases: &[(&str, &str)] = &[
            ("", r#""""#),
            ("hello", "hello"),
            ("hello world", r#""hello world""#),
            (r#"hello"world"#, r#""hello\"world""#),
            (r#"hello""world"#, r#""hello\"\"world""#),
            (r"hello\world", r"hello\world"),
            (r"hello\\world", r"hello\\world"),
            (r#"hello\"world"#, r#""hello\\\"world""#),
            (r#"hello\\"world"#, r#""hello\\\\\"world""#),
            (r"hello world\", r#""hello world\\""#),
            // Rust's std would leave these unwrapped (no space or tab), and
            // Git Bash then sees an unterminated quote.
            (r#""true""#, r#""\"true\"""#),
            (r#"echo "a b""#, r#""echo \"a b\"""#),
        ];
        for (input, want) in cases {
            assert_eq!(quote_arg(input), *want, "quoting {input:?}");
        }
    }

    #[test]
    fn respects_execution_policy_reads_the_environment() {
        assert!(!env(&[]).respects_execution_policy());
        assert!(env(&[("CLAUDE_CODE_POWERSHELL_RESPECT_EXECUTION_POLICY", "1")]).respects_execution_policy());
    }

    #[cfg(windows)]
    fn run_in_real_shell(shell: &Shell, original: &str) -> (Option<i32>, String) {
        let inv = invocation(shell, original, false);
        let out = windows_command(&inv)
            .stdin(std::process::Stdio::null())
            .output()
            .expect("the shell starts");
        (out.status.code(), String::from_utf8_lossy(&out.stdout).into_owned())
    }

    #[cfg(windows)]
    #[test]
    fn real_shells_run_an_invocation() {
        let shells = candidate_shells(&WinEnv::from_process(), &RealFs);
        assert!(!shells.is_empty(), "this machine has neither Git Bash nor PowerShell");
        for shell in &shells {
            let (code, out) = run_in_real_shell(shell, "echo contract-ok");
            assert_eq!(code, Some(0), "{}", shell.display_name());
            assert!(out.contains("contract-ok"), "{}: {out:?}", shell.display_name());
        }
    }

    #[cfg(windows)]
    #[test]
    fn real_shells_keep_the_exit_code_the_documented_way() {
        for shell in candidate_shells(&WinEnv::from_process(), &RealFs) {
            let name = shell.display_name();
            match &shell {
                Shell::Bash(_) => assert_eq!(run_in_real_shell(&shell, "exit 3").0, Some(3), "{name}"),
                Shell::PowerShell(_) => {
                    // `exit` ends the shell with that code directly.
                    assert_eq!(run_in_real_shell(&shell, "exit 3").0, Some(3), "{name}");
                    // A failed native command surfaces as 1 ...
                    assert_eq!(run_in_real_shell(&shell, "cmd /c exit 3").0, Some(1), "{name}");
                    // ... except 2, which the epilogue passes through.
                    assert_eq!(run_in_real_shell(&shell, "cmd /c exit 2").0, Some(2), "{name}");
                }
            }
        }
    }
}
