//! Run a child process to completion, with a timeout, without ever hanging the
//! caller. Shared by the status-line preflight (`statusline_setup::shells`) and
//! the warm-up that runs Claude Code (`warmup::claude_cli`).

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The app has no console, so a console child would flash a window.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub(crate) struct RunOutput {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
    pub(crate) elapsed: Duration,
}

/// After the process is gone, how long to wait for a pipe reader before
/// leaving it behind. Something the command started may still hold the pipe.
const DRAIN_GRACE: Duration = Duration::from_secs(3);

/// Join `handle`, but not for longer than [`DRAIN_GRACE`]. An abandoned thread
/// ends by itself when the pipe finally closes.
fn join_within<T: Default>(handle: std::thread::JoinHandle<T>) -> T {
    let deadline = Instant::now() + DRAIN_GRACE;
    while !handle.is_finished() {
        if Instant::now() >= deadline {
            return T::default();
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    handle.join().unwrap_or_default()
}

/// Start `cmd` with piped stdio, feed it `stdin`, collect its output, and kill
/// it at `timeout`. `code` is `None` when it was killed.
///
/// On Windows the child runs in a Job object that kills everything in it when
/// closed. The direct child is often only a launcher (Git's `bin\bash.exe`
/// starts `usr\bin\bash.exe`, npm's `claude.cmd` starts node), so killing it
/// alone would leave the real process running with our pipes open.
pub(crate) fn run_with_timeout(
    mut cmd: Command,
    stdin: &[u8],
    timeout: Duration,
) -> std::io::Result<RunOutput> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let started = Instant::now();
    #[cfg(windows)]
    let mut job = statusline_bridge::shell_windows::Job::new().ok();
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    #[cfg(windows)]
    if let Some(job) = &job {
        let _ = job.assign(&child);
    }
    let mut stdin_pipe = child.stdin.take();
    let bytes = stdin.to_vec();
    let writer = std::thread::spawn(move || {
        if let Some(pipe) = stdin_pipe.as_mut() {
            let _ = pipe.write_all(&bytes);
        }
    });
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let read_out = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_end(&mut buf);
        }
        buf
    });
    let read_err = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_end(&mut buf);
        }
        buf
    });
    let code = loop {
        match child.try_wait()? {
            Some(status) => break status.code(),
            None if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    };
    // Whatever the command left running dies with the job, which closes the
    // pipes the readers are waiting on.
    #[cfg(windows)]
    drop(job.take());
    join_within(writer);
    Ok(RunOutput {
        code,
        stdout: join_within(read_out),
        stderr: join_within(read_err),
        elapsed: started.elapsed(),
    })
}
