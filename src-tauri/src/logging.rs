use std::path::{Path, PathBuf};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

pub fn init(log_dir: PathBuf) -> tracing_appender::non_blocking::WorkerGuard {
    std::fs::create_dir_all(&log_dir).ok();
    if let Err(e) = restrict_permissions(&log_dir) {
        tracing::warn!("could not restrict log directory permissions: {e}");
    }

    let file_appender = RollingFileAppender::new(
        Rotation::DAILY,
        &log_dir,
        &format!("{}.log", crate::branding::USER_AGENT_PREFIX),
    );
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let file_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,claude_limits_lib=debug"));
    // Only forward warnings and above to stderr — avoids every line being
    // written twice (file + stderr) in the common case.
    let stderr_filter = EnvFilter::new("warn");

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(non_blocking).with_ansi(false).with_filter(file_filter))
        .with(fmt::layer().with_writer(std::io::stderr).with_filter(stderr_filter))
        .init();

    tracing::info!("Logging initialized at {:?}", log_dir);
    guard
}

pub fn log_dir() -> PathBuf {
    directories::ProjectDirs::from(
        crate::branding::PROJECT_DIRS_QUALIFIER,
        crate::branding::PROJECT_DIRS_ORG,
        crate::branding::PROJECT_DIRS_APP,
    )
    .map(|p| p.data_local_dir().join("logs"))
    .unwrap_or_else(|| PathBuf::from(".claude-monitor/logs"))
}

#[cfg(unix)]
fn restrict_permissions(p: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(p)?.permissions();
    perms.set_mode(0o700);
    std::fs::set_permissions(p, perms)?;
    Ok(())
}

#[cfg(windows)]
fn restrict_permissions(p: &Path) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::process::Command;
    let status = Command::new("icacls")
        .arg(p)
        .args([
            "/inheritance:r",
            "/grant:r",
            &format!("{}:(OI)(CI)F", std::env::var("USERNAME").unwrap_or_else(|_| "Administrator".to_string())),
        ])
        .status()
        .context("icacls failed to run")?;
    if !status.success() {
        anyhow::bail!("icacls returned non-zero");
    }
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn restrict_permissions(_: &Path) -> anyhow::Result<()> {
    Ok(())
}
