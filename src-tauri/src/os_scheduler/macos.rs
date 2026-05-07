// Filled in Plan B Task 13.
use anyhow::Result;
use std::path::Path;
use super::OsScheduler;

pub struct LaunchdScheduler;
impl LaunchdScheduler {
    pub fn new() -> Self { Self }
}
impl OsScheduler for LaunchdScheduler {
    fn register(&self, _: &Path) -> Result<()> { unimplemented!("Plan B Task 13") }
    fn unregister(&self) -> Result<()> { unimplemented!("Plan B Task 13") }
    fn is_registered(&self) -> Result<bool> { unimplemented!("Plan B Task 13") }
}
