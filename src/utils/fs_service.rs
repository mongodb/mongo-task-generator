//! Service for interacting with the filesystem.
use anyhow::Result;
use std::path::Path;

/// A service for working with the file system.
pub trait FsService: Sync + Send {
    /// Determine whether the given file path points to a file.
    ///
    /// # Arguments
    ///
    /// * `path` - Filesystem path to check.
    ///
    /// # Returns
    ///
    /// true if there is a file at the given path.
    fn file_exists(&self, path: &str) -> bool;

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
    /// Determine whether the given file path points to a file.
    ///
    /// # Arguments
    ///
    /// * `path` - Filesystem path to check.
    ///
    /// # Returns
    ///
    /// true if there is a file at the given path.
    fn file_exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }

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
