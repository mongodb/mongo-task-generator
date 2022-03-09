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
    fn file_exists(&self, path: &str) -> bool {
        Path::new(path).exists()
    }
}
