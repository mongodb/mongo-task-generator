use std::time::Instant;

use anyhow::Result;
use serde::Deserialize;
use tracing::{event, Level};

use crate::resmoke::external_cmd::run_command;

/// Task that burn_in discovered should be run.
#[derive(Debug, Clone, Deserialize)]
pub struct DiscoveredTask {
    /// Name of task to run.
    pub task_name: String,
    /// List of tests to run as part of task.
    pub test_list: Vec<String>,
}

/// List of tasks that should be run as part of burn_in.
#[derive(Debug, Clone, Deserialize)]
pub struct DiscoveredTaskList {
    /// List of tasks that should be run as part of burn_in.
    pub discovered_tasks: Vec<DiscoveredTask>,
}

/// Interface to query information from burn_in_tests.
pub trait BurnInDiscovery: Send + Sync {
    /// Discover what tasks/tests should be run as part of burn_in.
    ///
    /// # Arguments
    ///
    /// * `build_variant` - Build variant to query information about.
    ///
    /// # Returns
    ///
    /// A list of tasks/tests that were discovered by burn_in_tests.
    fn discover_tasks(&self, build_variant: &str) -> Result<Vec<DiscoveredTask>>;
}

pub struct BurnInProxy {}

impl BurnInProxy {
    pub fn new() -> Self {
        BurnInProxy {}
    }
}

impl BurnInDiscovery for BurnInProxy {
    /// Discover what tasks/tests should be run as part of burn_in.
    ///
    /// # Arguments
    ///
    /// * `build_variant` - Build variant to query information about.
    ///
    /// # Returns
    ///
    /// A list of tasks/tests that were discovered by burn_in_tests.
    fn discover_tasks(&self, build_variant: &str) -> Result<Vec<DiscoveredTask>> {
        let cmd = vec![
            "python",
            "buildscripts/burn_in_tests.py",
            "--build-variant",
            build_variant,
            "--yaml",
        ];
        let start = Instant::now();

        let cmd_output = run_command(&cmd)?;

        event!(
            Level::INFO,
            duration_ms = start.elapsed().as_millis() as u64,
            "Burn In Discovery Finished"
        );

        let output: DiscoveredTaskList = serde_yaml::from_str(&cmd_output)?;
        Ok(output.discovered_tasks)
    }
}
