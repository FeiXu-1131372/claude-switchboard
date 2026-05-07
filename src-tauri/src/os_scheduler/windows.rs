// Filled in Plan B Task 14.
use anyhow::Result;
use std::path::Path;
use super::OsScheduler;

pub struct SchTasksScheduler;
impl SchTasksScheduler {
    pub fn new() -> Self { Self }
}
impl OsScheduler for SchTasksScheduler {
    fn register(&self, _: &Path) -> Result<()> { unimplemented!("Plan B Task 14") }
    fn unregister(&self) -> Result<()> { unimplemented!("Plan B Task 14") }
    fn is_registered(&self) -> Result<bool> { unimplemented!("Plan B Task 14") }
}
