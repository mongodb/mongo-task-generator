//! Service for interacting with the filesystem.
use anyhow::Result;
use std::path::Path;

/// A service for working with the file system.
pub trait FsService: Sync + Send {
    /// Write the given contents to disk at the given location.
    ///
    /// # Arguments
    ///
    /// * `path` - Filesystem path to write to.
    /// * `contents` - Contents to write to file.
    ///
    /// # Returns
    ///
    /// Returns the unit value after contents have been written successfully.
    fn write_file(&self, path: &Path, contents: &str) -> Result<()>;
}

pub struct FsServiceImpl {}

/// Implementation of FsService.
impl FsServiceImpl {
    /// Create a new instance of FsServiceImpl.
    pub fn new() -> Self {
        Self {}
    }
}

impl FsService for FsServiceImpl {
    /// Write the given contents to disk at the given location.
    ///
    /// # Arguments
    ///
    /// * `path` - Filesystem path to write to.
    /// * `contents` - Contents to write to file.
    ///
    /// # Returns
    ///
    /// Returns the unit value after contents have been written successfully.
    fn write_file(&self, path: &Path, contents: &str) -> Result<()> {
        Ok(std::fs::write(path, contents)?)
    }
}
