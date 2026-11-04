//! Windows command forms for `statusLine.command` (spec §4.5).
//!
//! Pure string work, tested on every platform. Nothing calls it yet: the app
//! reports `Unsupported` on Windows until the VM matrix in spec §9 passes, and
//! the preflight that tries these candidates in each shell is wired in when
//! that gate comes off.

// Wired in when the Windows gate comes off.
#![cfg_attr(not(windows), allow(dead_code))]

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
