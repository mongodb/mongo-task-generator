use std::{path::Path, str::FromStr, time::Instant};

use anyhow::Result;
use cmd_lib::run_fun;
use serde::Deserialize;
use tracing::{event, Level};

use super::resmoke_suite::ResmokeSuiteConfig;

/// Interface for discovering details about test suites.
pub trait TestDiscovery: Send + Sync {
    /// Get a list of tests that belong to the given suite.
    ///
    /// # Arguments
    ///
    /// * `suite_name` - Name of test suite to query.
    ///
    /// # Returns
    ///
    /// A list of tests belonging to given suite.
    fn discover_tests(&self, suite_name: &str) -> Result<Vec<String>>;

    /// Get the configuration for the given suite.
    ///
    /// # Arguments
    ///
    /// * `suite_name` - Name of test suite to query.
    ///
    /// # Return
    ///
    /// Resmoke configuration for the given suite.
    fn get_suite_config(&self, suite_name: &str) -> Result<ResmokeSuiteConfig>;

    /// Get the multiversion configuration to generate against.
    fn get_multiversion_config(&self) -> Result<MultiversionConfig>;
}

/// Implementation of `TestDiscovery` that queries details from resmoke.
#[derive(Debug, Clone)]
pub struct ResmokeProxy {}

impl ResmokeProxy {
    /// Create a new `ResmokeProxy` instance.
    pub fn new() -> Self {
        Self {}
    }
}

/// Details about tests comprising a test suite.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TestDiscoveryOutput {
    /// Name of suite.
    pub suite_name: String,

    /// Name of tests comprising suite.
    pub tests: Vec<String>,
}

impl TestDiscovery for ResmokeProxy {
    /// Get a list of tests that belong to the given suite.
    ///
    /// # Arguments
    ///
    /// * `suite_name` - Name of test suite to query.
    ///
    /// # Returns
    ///
    /// A list of tests belonging to given suite.
    fn discover_tests(&self, suite_name: &str) -> Result<Vec<String>> {
        let start = Instant::now();
        let cmd_output = run_fun!(
            python buildscripts/resmoke.py test-discovery --suite $suite_name
        )?;
        event!(
            Level::INFO,
            suite_name,
            duration_ms = start.elapsed().as_millis() as u64,
            "Resmoke test discovery finished"
        );

        let output: TestDiscoveryOutput = serde_yaml::from_str(&cmd_output)?;
        Ok(output
            .tests
            .into_iter()
            .filter(|f| Path::new(f).exists())
            .collect())
    }

    /// Get the configuration for the given suite.
    ///
    /// # Arguments
    ///
    /// * `suite_name` - Name of test suite to query.
    ///
    /// # Return
    ///
    /// Resmoke configuration for the given suite.
    fn get_suite_config(&self, suite_name: &str) -> Result<ResmokeSuiteConfig> {
        let cmd_output = run_fun!(
            python buildscripts/resmoke.py suiteconfig --suite $suite_name
        )?;
        Ok(ResmokeSuiteConfig::from_str(&cmd_output)?)
    }

    /// Get the multiversion configuration to generate against.
    fn get_multiversion_config(&self) -> Result<MultiversionConfig> {
        MultiversionConfig::from_resmoke()
    }
}

/// Multiversion configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct MultiversionConfig {
    /// Previous version of MongoDB to test against.
    pub last_versions: Vec<String>,
    /// Tags for required FCV version.
    pub requires_fcv_tag: String,
}

impl MultiversionConfig {
    /// Query the multiversion configuration from resmoke.
    pub fn from_resmoke() -> Result<MultiversionConfig> {
        let cmd_output = run_fun!(
            python buildscripts/resmoke.py multiversion-config
        )?;
        Ok(serde_yaml::from_str(&cmd_output)?)
    }
}
