//! launchd-backed OS scheduler. Writes a per-user LaunchAgent plist that
//! invokes `<binary> --tick` every 60 seconds. Per spec §7.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use super::OsScheduler;

const LABEL: &str = "com.claude-switchboard.scheduler";

pub struct LaunchdScheduler {
    plist_path: PathBuf,
}

impl LaunchdScheduler {
    pub fn new() -> Self {
        let home = directories::UserDirs::new()
            .map(|u| u.home_dir().to_path_buf())
            .expect("UserDirs::new returned None");
        Self {
            plist_path: home
                .join("Library")
                .join("LaunchAgents")
                .join(format!("{LABEL}.plist")),
        }
    }

    fn build_plist_xml(&self, binary_path: &Path) -> String {
        let bin_str = binary_path.to_string_lossy();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key><string>{label}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{bin_str}</string>
        <string>--tick</string>
    </array>
    <key>StartInterval</key><integer>60</integer>
    <key>RunAtLoad</key><false/>
    <key>StandardOutPath</key><string>/tmp/{label}.out.log</string>
    <key>StandardErrorPath</key><string>/tmp/{label}.err.log</string>
</dict>
</plist>
"#,
            label = LABEL,
            bin_str = bin_str,
        )
    }
}

impl OsScheduler for LaunchdScheduler {
    fn register(&self, binary_path: &Path) -> Result<()> {
        if let Some(parent) = self.plist_path.parent() {
            std::fs::create_dir_all(parent).context("create LaunchAgents dir")?;
        }
        let xml = self.build_plist_xml(binary_path);
        std::fs::write(&self.plist_path, xml).context("write plist")?;

        let _ = std::process::Command::new("launchctl")
            .arg("unload")
            .arg(&self.plist_path)
            .status();
        let load = std::process::Command::new("launchctl")
            .arg("load")
            .arg(&self.plist_path)
            .status()
            .context("launchctl load")?;
        if !load.success() {
            anyhow::bail!("launchctl load failed: status {load}");
        }
        Ok(())
    }

    fn unregister(&self) -> Result<()> {
        if !self.plist_path.exists() {
            return Ok(());
        }
        let _ = std::process::Command::new("launchctl")
            .arg("unload")
            .arg(&self.plist_path)
            .status();
        std::fs::remove_file(&self.plist_path).context("remove plist")?;
        Ok(())
    }

    fn is_registered(&self) -> Result<bool> {
        Ok(self.plist_path.exists())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_plist_xml_includes_tick_arg_and_60s_interval() {
        let s = LaunchdScheduler::new();
        let xml = s.build_plist_xml(Path::new("/Applications/Claude Switchboard.app/Contents/MacOS/claude-switchboard"));
        assert!(xml.contains("<string>--tick</string>"));
        assert!(xml.contains("<integer>60</integer>"));
        assert!(xml.contains("/Applications/Claude Switchboard.app"));
        assert!(xml.contains(LABEL));
    }
}
