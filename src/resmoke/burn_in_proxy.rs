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

pub struct BurnInProxy {
    /// Primary command to invoke burn_in_tests (usually `python`).
    burn_in_tests_cmd: String,
    /// Script to invoke burn_in_tests.
    burn_in_tests_script: Vec<String>,
}

impl BurnInProxy {
    /// Create a new `BurnInProxy` instance.
    ///
    /// # Arguments
    ///
    /// * `burn_in_tests_cmd` - Command to invoke resmoke.
    pub fn new(burn_in_tests_cmd: &str) -> Self {
        let cmd_parts: Vec<_> = burn_in_tests_cmd.split(' ').collect();
        let cmd = cmd_parts[0];
        let script = cmd_parts[1..].iter().map(|s| s.to_string()).collect();
        Self {
            burn_in_tests_cmd: cmd.to_string(),
            burn_in_tests_script: script,
        }
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
        let mut cmd = vec![self.burn_in_tests_cmd.as_str()];
        cmd.append(
            &mut self
                .burn_in_tests_script
                .iter()
                .map(|s| s.as_str())
                .collect(),
        );
        cmd.append(&mut vec!["--build-variant", build_variant, "--yaml"]);
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
