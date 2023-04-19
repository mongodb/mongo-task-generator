//! Multiversion task generation utilities.
//!
//! # Understanding multiversion task generation
//!
//! In multiversion testing, we want to generate tasks that run against different
//! versions of MongoDB.
//!
//! To understand what is being tested, you should be familiar with the following terms:
//!
//! - `lts` - Long-Term Support. This refers to the yearly, major release of MongoDB (e.g. 5.0, 6.0, ...).
//! - `continuous` - Continuous release. This refers to the quarterly releases of MongoDB (e.g. 5.1, 5.2, 5.3, ...).
//! - `old versions` - The previous releases on MongoDB to test against. If the previous release was
//!   a `lts` release, only that needs to be tested against. If the previous release was not
//!   a `lts` release, then we should test against both that release and the last `lts` release.
//! - `version combinations` - When creating a replica set to test against, the version combinations
//!   refer what version each node in the replica set should be. The version value will be either
//!   `new` or `old`. `new` refers to the version of MongoDB being tested. `old` refers to the
//!   previous `old_version` being tests (`lts` or `continuous`).

use anyhow::Result;

use crate::{
    evergreen_names::{
        BACKPORT_REQUIRED_TAG, MULTIVERSION_INCOMPATIBLE, MULTIVERSION_LAST_CONTINUOUS,
        MULTIVERSION_LAST_LTS,
    },
    resmoke::resmoke_proxy::MultiversionConfig,
};

/// A service for helping generating multiversion tasks.
pub trait MultiversionService: Sync + Send {
    /// Get the exclude tags for the given task.
    ///
    /// # Arguments
    ///
    /// * `task_name` - Name of task to query.
    /// * `mv_mode` - Type of multiversion task being generated (last_lts, continuous).
    ///
    /// # Returns
    ///
    /// Exclude tags as a comma-separated string.
    fn exclude_tags_for_task(&self, task_name: &str, mv_mode: Option<String>) -> String;
}

/// Implementation of Multiversion service.
pub struct MultiversionServiceImpl {
    /// Multiversion Configuration.
    multiversion_config: MultiversionConfig,
}

/// Implementation of Multiversion service.
impl MultiversionServiceImpl {
    /// Create a new instance of Multiversion service.
    ///
    /// # Arguments
    ///
    /// * `discovery_service` - Instance of service to query details about test suites.
    pub fn new(multiversion_config: MultiversionConfig) -> Result<Self> {
        Ok(Self {
            multiversion_config,
        })
    }
}

impl MultiversionService for MultiversionServiceImpl {
    /// Get the exclude tags for the given task.
    ///
    /// # Arguments
    ///
    /// * `task_name` - Name of task to query.
    ///
    /// # Returns
    ///
    /// Exclude tags as a comma-separated string.
    fn exclude_tags_for_task(&self, task_name: &str, mv_mode: Option<String>) -> String {
        let task_tag = format!("{}_{}", task_name, BACKPORT_REQUIRED_TAG);
        let exclude_tags = if let Some(mode) = mv_mode {
            match mode.as_str() {
                MULTIVERSION_LAST_LTS => self.multiversion_config.get_fcv_tags_for_lts(),
                MULTIVERSION_LAST_CONTINUOUS => {
                    self.multiversion_config.get_fcv_tags_for_continuous()
                }
                _ => panic!("Unknown multiversion mode: {}", &mode),
            }
        } else {
            self.multiversion_config.requires_fcv_tag.clone()
        };
        let tags = vec![
            MULTIVERSION_INCOMPATIBLE.to_string(),
            BACKPORT_REQUIRED_TAG.to_string(),
            task_tag,
            exclude_tags,
        ];

        tags.join(",")
    }
}
