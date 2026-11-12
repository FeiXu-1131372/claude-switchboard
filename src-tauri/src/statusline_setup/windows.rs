//! Windows command forms for `statusLine.command` (spec §4.5).
//!
//! The string work is pure and tested on every platform. The preflight tries
//! these forms in order, in each shell Claude Code can pick, and installs the
//! first that passes everywhere. Only `short_path` calls into Windows.

// Only the Windows build of the manager asks for these forms.
#![cfg_attr(not(windows), allow(dead_code))]

use std::path::Path;

/// The 8.3 short form of an existing path (`C:\Users\FEIXU~1\…`), or `None`
/// when the path does not exist or the volume has short names turned off.
#[cfg(windows)]
pub fn short_path(path: &Path) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    // SAFETY: `wide` is NUL-terminated. The first call, with a null buffer,
    // returns the size needed including the terminator; the second fills a
    // buffer of exactly that size and returns the length without it.
    unsafe {
        let needed = GetShortPathNameW(wide.as_ptr(), std::ptr::null_mut(), 0);
        if needed == 0 {
            return None;
        }
        let mut buf = vec![0u16; needed as usize];
        let written = GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), needed);
        if written == 0 || written >= needed {
            return None;
        }
        String::from_utf16(&buf[..written as usize]).ok()
    }
}

#[cfg(not(windows))]
pub fn short_path(_path: &Path) -> Option<String> {
    None
}

/// A path containing any of these needs quoting in at least one of the shells
/// Claude Code can pick, and the three shells disagree on how.
const UNSAFE_IN_UNQUOTED_PATH: &[char] = &[' ', '\'', '"', '$', '&', '(', ')', '@', '{', '}', ';', '#', '`', '%'];

fn forward_slashes(path: &str) -> String {
    path.replace('\\', "/")
}

fn is_plain(path: &str) -> bool {
    !path.contains(UNSAFE_IN_UNQUOTED_PATH)
}

/// The command strings to try, best first. The first one that passes in every
/// shell is used.
///
/// 1. the absolute path with forward slashes, when it needs no quoting;
/// 2. the 8.3 short path (`GetShortPathNameW`) when the volume has short names;
/// 3. the tilde form, which every shell expands but which assumes the bridge
///    lives at the default location (`file_name` under `~/.claude-switchboard/statusline/`).
pub fn command_candidates(
    bridge_path: &str,
    short_path: Option<&str>,
    target_id: &str,
    file_name: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    let absolute = forward_slashes(bridge_path);
    if is_plain(&absolute) {
        out.push(format!("{absolute} --target {target_id}"));
    }
    if let Some(short) = short_path.map(forward_slashes).filter(|s| is_plain(s) && *s != absolute) {
        out.push(format!("{short} --target {target_id}"));
    }
    out.push(format!("~/.claude-switchboard/statusline/{file_name} --target {target_id}"));
    out
}

#[cfg(test)]
mod short_path_tests {
    use super::*;

    #[test]
    fn a_path_that_does_not_exist_has_no_short_form() {
        assert_eq!(short_path(Path::new(r"C:\definitely\not\here\bridge.exe")), None);
    }

    #[cfg(windows)]
    #[test]
    fn an_existing_path_gets_a_space_free_short_form_or_none_when_the_volume_has_no_short_names() {
        let dir = tempfile::tempdir().unwrap();
        let long = dir.path().join("a directory with spaces in its name");
        std::fs::create_dir_all(&long).unwrap();
        let file = long.join("bridge.exe");
        std::fs::write(&file, b"x").unwrap();
        if let Some(short) = short_path(&file) {
            assert!(!short.contains(' '), "{short}");
            assert!(Path::new(&short).is_file(), "{short} must name the same file");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0123456789ab";
    const EXE: &str = "bridge.exe";

    #[test]
    fn a_plain_profile_path_comes_first_with_forward_slashes() {
        let c = command_candidates(r"C:\Users\fei\.claude-switchboard\statusline\bridge.exe", None, ID, EXE);
        assert_eq!(
            c,
            vec![
                format!("C:/Users/fei/.claude-switchboard/statusline/bridge.exe --target {ID}"),
                format!("~/.claude-switchboard/statusline/bridge.exe --target {ID}"),
            ]
        );
    }

    #[test]
    fn a_profile_path_with_a_space_skips_straight_to_the_short_path() {
        let c = command_candidates(
            r"C:\Users\Fei Xu\.claude-switchboard\statusline\bridge.exe",
            Some(r"C:\Users\USERNA~1\.claude-switchboard\statusline\bridge.exe"),
            ID,
            EXE,
        );
        assert_eq!(c.len(), 2);
        assert_eq!(c[0], format!("C:/Users/USERNA~1/.claude-switchboard/statusline/bridge.exe --target {ID}"));
        assert!(c[1].starts_with("~/"));
    }

    #[test]
    fn without_short_names_a_spaced_path_leaves_only_the_tilde_form() {
        let c = command_candidates(r"C:\Users\Fei Xu\.claude-switchboard\statusline\bridge.exe", None, ID, EXE);
        assert_eq!(c, vec![format!("~/.claude-switchboard/statusline/bridge.exe --target {ID}")]);
    }

    #[test]
    fn every_character_in_the_unsafe_set_rules_a_path_out() {
        for ch in UNSAFE_IN_UNQUOTED_PATH {
            let path = format!(r"C:\Users\a{ch}b\bridge.exe");
            let c = command_candidates(&path, None, ID, EXE);
            assert_eq!(c.len(), 1, "{ch:?} should rule out the absolute form");
        }
    }

    #[test]
    fn a_short_path_that_is_unsafe_or_identical_is_not_offered_twice() {
        let same = command_candidates(r"C:\Users\fei\b.exe", Some(r"C:\Users\fei\b.exe"), ID, EXE);
        assert_eq!(same.len(), 2);
        let unsafe_short = command_candidates(r"C:\Users\Fei Xu\b.exe", Some(r"C:\Users\Fei Xu\b.exe"), ID, EXE);
        assert_eq!(unsafe_short.len(), 1);
    }
}
