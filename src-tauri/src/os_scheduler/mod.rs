//! OS-level scheduler abstraction. Platform-specific implementations live in
//! sibling modules.

use anyhow::Result;
use std::path::Path;

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

/// Platform-agnostic OS scheduler interface. Per spec §7 install flow.
pub trait OsScheduler: Send + Sync {
    /// Register a recurring fire that invokes `binary_path --tick` every minute.
    fn register(&self, binary_path: &Path) -> Result<()>;

    /// Remove the registration.
    fn unregister(&self) -> Result<()>;

    /// True if a registration currently exists.
    fn is_registered(&self) -> Result<bool>;
}

/// Returns the appropriate platform implementation, or None on unsupported OSes.
#[cfg(target_os = "macos")]
pub fn for_current_platform() -> Option<Box<dyn OsScheduler>> {
    Some(Box::new(macos::LaunchdScheduler::new()))
}

#[cfg(target_os = "windows")]
pub fn for_current_platform() -> Option<Box<dyn OsScheduler>> {
    Some(Box::new(windows::SchTasksScheduler::new()))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn for_current_platform() -> Option<Box<dyn OsScheduler>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn for_current_platform_returns_some_on_supported_os() {
        let s = for_current_platform();
        if cfg!(target_os = "macos") || cfg!(target_os = "windows") {
            assert!(s.is_some());
        } else {
            assert!(s.is_none());
        }
    }
}
