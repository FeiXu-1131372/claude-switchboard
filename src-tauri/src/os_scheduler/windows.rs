//! Per-user Windows Task Scheduler registration, without passwords or elevation.
use super::OsScheduler;
use anyhow::{Context, Result};
use std::{io::Write, os::windows::process::CommandExt, path::Path, process::Command};

const TASK_NAME: &str = "Claude Switchboard Tick";

#[derive(Default)]
pub struct SchTasksScheduler;

fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    command
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn task_xml(binary: &Path, sid: &str, start: &str) -> Result<String> {
    let working_dir = binary
        .parent()
        .context("scheduler executable has no parent")?;
    let binary = xml_escape(&binary.to_string_lossy());
    let working_dir = xml_escape(&working_dir.to_string_lossy());
    let sid = xml_escape(sid);
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo><Description>Warm up enabled Claude accounts on their saved schedules.</Description></RegistrationInfo>
  <Triggers><TimeTrigger><Repetition><Interval>PT1M</Interval><StopAtDurationEnd>false</StopAtDurationEnd></Repetition><StartBoundary>{start}</StartBoundary><Enabled>true</Enabled></TimeTrigger></Triggers>
  <Principals><Principal id="CurrentUser"><UserId>{sid}</UserId><LogonType>InteractiveToken</LogonType><RunLevel>LeastPrivilege</RunLevel></Principal></Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <StartWhenAvailable>true</StartWhenAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand><Enabled>true</Enabled><Hidden>true</Hidden>
    <ExecutionTimeLimit>PT5M</ExecutionTimeLimit>
  </Settings>
  <Actions Context="CurrentUser"><Exec><Command>{binary}</Command><Arguments>--tick</Arguments><WorkingDirectory>{working_dir}</WorkingDirectory></Exec></Actions>
</Task>"#
    ))
}

impl SchTasksScheduler {
    pub fn new() -> Self {
        Self
    }
}

impl OsScheduler for SchTasksScheduler {
    fn register(&self, binary_path: &Path) -> Result<()> {
        anyhow::ensure!(
            binary_path.is_absolute() && binary_path.is_file(),
            "Scheduler executable is missing or is not an absolute path"
        );
        let identity = command("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .context("read current Windows identity")?;
        anyhow::ensure!(
            identity.status.success(),
            "Could not read current Windows identity"
        );
        let identity = String::from_utf8_lossy(&identity.stdout);
        let sid = identity
            .trim()
            .split(',')
            .map(|part| part.trim().trim_matches('"'))
            .find(|part| part.starts_with("S-1-"))
            .context("Windows identity did not include a SID")?;
        let start = chrono::Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
        let xml = task_xml(binary_path, sid, &start)?;
        let path = std::env::temp_dir().join(format!(
            "switchboard-task-{:016x}.xml",
            rand::random::<u64>()
        ));
        let registration = (|| -> Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            file.write_all(&[0xff, 0xfe])?;
            for unit in xml.encode_utf16() {
                file.write_all(&unit.to_le_bytes())?;
            }
            drop(file);
            let result = command("schtasks")
                .args(["/Create", "/F", "/TN", TASK_NAME, "/XML"])
                .arg(&path)
                .output()
                .context("register Windows warm-up task")?;
            anyhow::ensure!(
                result.status.success(),
                "Windows task registration failed ({}): {} {}",
                result.status,
                String::from_utf8_lossy(&result.stderr).trim(),
                String::from_utf8_lossy(&result.stdout).trim()
            );
            Ok(())
        })();
        let _ = std::fs::remove_file(&path);
        registration
    }

    fn unregister(&self) -> Result<()> {
        if !self.is_registered()? {
            return Ok(());
        }
        let result = command("schtasks")
            .args(["/Delete", "/F", "/TN", TASK_NAME])
            .output()
            .context("delete Windows warm-up task")?;
        anyhow::ensure!(
            result.status.success(),
            "Could not remove warm-up task: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        );
        Ok(())
    }

    fn is_registered(&self) -> Result<bool> {
        Ok(command("schtasks")
            .args(["/Query", "/TN", TASK_NAME])
            .output()
            .context("query Windows warm-up task")?
            .status
            .success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_is_per_user_quiet_and_handles_sleep_and_battery() {
        let xml = task_xml(
            Path::new(r"C:\Users\A&B\Claude Switchboard\app.exe"),
            "S-1-5-21-1234",
            "2026-11-10T12:00:00+13:00",
        )
        .unwrap();
        for required in [
            "PT1M",
            "IgnoreNew",
            "InteractiveToken",
            "LeastPrivilege",
            "<StartWhenAvailable>true",
            "<DisallowStartIfOnBatteries>false",
            "<StopIfGoingOnBatteries>false",
            "<Arguments>--tick",
            "<WorkingDirectory>C:\\Users\\A&amp;B\\Claude Switchboard",
        ] {
            assert!(xml.contains(required), "missing {required}");
        }
        assert!(xml.contains("A&amp;B"));
        assert!(!xml.contains("SYSTEM"));
    }
}
