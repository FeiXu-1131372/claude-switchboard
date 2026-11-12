//! The rule that picks the command form to install (spec §4.5). Pure, so it is
//! tested on every platform with fakes; `windows_tests` runs it for real.

use super::*;

fn list(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

/// `(command, per-shell verdicts)`: what a preflight attempt returns here.
type Attempt = (String, Vec<(&'static str, bool)>);

fn passed(a: &Attempt) -> bool {
    !a.1.is_empty() && a.1.iter().all(|(_, ok)| *ok)
}

#[test]
fn stops_at_the_first_candidate_that_passes_everywhere() {
    let mut tried = Vec::new();
    let got = first_passing(
        &list(&["a", "b", "c"]),
        |c| {
            tried.push(c.to_string());
            Ok::<Attempt, ()>((c.to_string(), vec![("sh", c == "b")]))
        },
        passed,
    );
    assert_eq!(got.unwrap().unwrap().0, "b");
    assert_eq!(tried, ["a", "b"], "c is never tried once b passed");
}

#[test]
fn a_candidate_that_fails_in_one_of_two_shells_is_skipped() {
    let got = first_passing(
        &list(&["C:\\x\\bridge.exe", "C:/x/bridge.exe"]),
        |c| {
            // Backslashes survive PowerShell but not bash.
            let bash_ok = !c.contains('\\');
            Ok::<Attempt, ()>((c.to_string(), vec![("Git Bash", bash_ok), ("PowerShell 7", true)]))
        },
        passed,
    );
    assert_eq!(got.unwrap().unwrap().0, "C:/x/bridge.exe");
}

#[test]
fn a_single_shell_machine_needs_only_that_shell() {
    let got = first_passing(
        &list(&["a"]),
        |c| Ok::<Attempt, ()>((c.to_string(), vec![("Windows PowerShell", true)])),
        passed,
    );
    assert!(got.unwrap().is_ok_and(|a| passed(&a)));
}

#[test]
fn a_spaced_profile_falls_through_to_the_tilde_form() {
    // Review focus: C:\Users\Fei Xu, no 8.3 names on the volume.
    let candidates = windows::command_candidates(
        r"C:\Users\Fei Xu\.claude-switchboard\statusline\bridge.exe",
        None,
        "0123456789ab",
        "bridge.exe",
    );
    let got = first_passing(
        &candidates,
        |c| Ok::<Attempt, ()>((c.to_string(), vec![("Git Bash", !c.contains(' '))])),
        passed,
    );
    let chosen = got.unwrap().unwrap().0;
    assert!(chosen.starts_with("~/.claude-switchboard/statusline/bridge.exe --target "), "{chosen}");
}

#[test]
fn when_none_pass_the_last_attempt_is_returned_so_its_failure_can_be_shown() {
    let got = first_passing(
        &list(&["a", "b"]),
        |c| Ok::<Attempt, ()>((c.to_string(), vec![("sh", false)])),
        passed,
    );
    let last = got.unwrap().unwrap();
    assert_eq!(last.0, "b");
    assert!(!passed(&last));
}

#[test]
fn an_error_stops_the_search_at_once() {
    let mut tried = 0;
    let got = first_passing(
        &list(&["a", "b", "c"]),
        |_| {
            tried += 1;
            Err::<Attempt, &str>("disk full")
        },
        passed,
    );
    assert_eq!(got, Some(Err("disk full")));
    assert_eq!(tried, 1);
}

#[test]
fn no_candidates_gives_none() {
    let got = first_passing(&[], |c| Ok::<Attempt, ()>((c.to_string(), vec![])), passed);
    assert!(got.is_none());
}
