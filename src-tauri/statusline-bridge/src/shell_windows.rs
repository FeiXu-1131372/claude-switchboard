//! Windows: which shell Claude Code runs a status line in, and the exact
//! command it hands that shell (spec §2.1 and §4.3).
//!
//! Everything here is pure: it reads an injected environment and file system,
//! so it compiles and is tested on every platform. Paths are plain `String`s
//! with Windows separators, because a `PathBuf` on macOS would not split on
//! `\`. The code that actually spawns a process is at the bottom, behind
//! `cfg(windows)`, and is **UNTESTED(windows)**: it was written to the spec on
//! a machine without Windows.

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

/// The shell Claude Code picks, with its path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shell {
    Bash(String),
    PowerShell(String),
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
    // UNTESTED(windows): everything in this module.
    use super::*;
    use std::io::Write;
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Command, Stdio};
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

    struct RealFs;
    impl Fs for RealFs {
        fn is_file(&self, path: &str) -> bool {
            std::path::Path::new(path).is_file()
        }
    }

    /// A Job object that kills everything in it when its last handle closes.
    /// Held for the life of the bridge process: when the bridge dies, so does
    /// the original's whole tree.
    struct Job(HANDLE);

    impl Job {
        fn new() -> std::io::Result<Job> {
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

        fn assign(&self, child: &Child) -> std::io::Result<()> {
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

    pub fn spawn_original(
        original: &str,
        stdin_bytes: &[u8],
        depth_env: &str,
    ) -> std::io::Result<Child> {
        let env = WinEnv::from_process();
        let shell = resolve_shell(&env, &RealFs).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "neither Git Bash nor PowerShell was found",
            )
        })?;
        let mut command = match &shell {
            Shell::Bash(bash) => {
                let mut c = Command::new(bash);
                c.arg("-c").arg(bash_command(original));
                c
            }
            Shell::PowerShell(ps) => {
                let respect = env.get("CLAUDE_CODE_POWERSHELL_RESPECT_EXECUTION_POLICY").is_some();
                let (flags, text) = powershell_command(original, respect);
                let mut c = Command::new(ps);
                c.args(flags).arg(text);
                c
            }
        };
        // The std quoting is the CommandLineToArgvW convention libuv uses. The
        // spec asks for a routine pinned against Bun's own quoting; that
        // corpus has not been run, so a hostile command may differ.
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
pub use spawn::spawn_original;

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
}
