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
pub struct ResmokeProxy {
    /// Primary command to invoke resmoke (usually `python`).
    resmoke_cmd: String,
    /// Script to invoke resmoke.
    resmoke_script: Vec<String>,
}

impl ResmokeProxy {
    /// Create a new `ResmokeProxy` instance.
    ///
    /// # Arguments
    ///
    /// * `resmoke_cmd` - Command to invoke resmoke.
    pub fn new(resmoke_cmd: &str) -> Self {
        let cmd_parts: Vec<_> = resmoke_cmd.split(' ').collect();
        let cmd = cmd_parts[0];
        let script = cmd_parts[1..].iter().map(|s| s.to_string()).collect();
        Self {
            resmoke_cmd: cmd.to_string(),
            resmoke_script: script,
        }
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
        let cmd = &self.resmoke_cmd;
        let script = &self.resmoke_script;
        let start = Instant::now();
        let cmd_output = run_fun!(
            $cmd $[script] test-discovery --suite $suite_name
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
        let cmd = &self.resmoke_cmd;
        let script = &self.resmoke_script;
        let cmd_output = run_fun!(
            $cmd $[script] suiteconfig --suite $suite_name
        )?;
        Ok(ResmokeSuiteConfig::from_str(&cmd_output)?)
    }

    /// Get the multiversion configuration to generate against.
    fn get_multiversion_config(&self) -> Result<MultiversionConfig> {
        MultiversionConfig::from_resmoke(&self.resmoke_cmd, &self.resmoke_script)
    }
}

/// Multiversion configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct MultiversionConfig {
    /// Previous version of MongoDB to test against.
    pub last_versions: Vec<String>,

    /// Tags for required FCV version.
    pub requires_fcv_tag: String,

    /// Tags for last LTS FCV versions.
    pub requires_fcv_tag_lts: Option<String>,

    /// Tags for last continuous FCV versions.
    pub requires_fcv_tag_continuous: Option<String>,
}

impl MultiversionConfig {
    /// Query the multiversion configuration from resmoke.
    pub fn from_resmoke(cmd: &str, script: &[String]) -> Result<MultiversionConfig> {
        let cmd_output = run_fun!(
            $cmd $[script] multiversion-config
        )?;
        Ok(serde_yaml::from_str(&cmd_output)?)
    }

    /// Get the required FCV tag for the lts version.
    pub fn get_fcv_tags_for_lts(&self) -> String {
        if let Some(requires_fcv_tag_lts) = &self.requires_fcv_tag_lts {
            requires_fcv_tag_lts.clone()
        } else {
            self.requires_fcv_tag.clone()
        }
    }

    /// Get the required FCV tag for the continuous version.
    pub fn get_fcv_tags_for_continuous(&self) -> String {
        if let Some(requires_fcv_tag_continuous) = &self.requires_fcv_tag_continuous {
            requires_fcv_tag_continuous.clone()
        } else {
            self.requires_fcv_tag.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // tests for get_fcv_tags_for_lts.
    #[test]
    fn test_get_fcv_tags_for_lts_should_use_lts_if_provided() {
        let mv_config = MultiversionConfig {
            last_versions: vec![],
            requires_fcv_tag: "fcv_fallback".to_string(),
            requires_fcv_tag_lts: Some("fcv_lts_explicit".to_string()),
            requires_fcv_tag_continuous: None,
        };

        assert_eq!(&mv_config.get_fcv_tags_for_lts(), "fcv_lts_explicit")
    }

    #[test]
    fn test_get_fcv_tags_for_lts_should_fallback_if_no_lts_provided() {
        let mv_config = MultiversionConfig {
            last_versions: vec![],
            requires_fcv_tag: "fcv_fallback".to_string(),
            requires_fcv_tag_lts: None,
            requires_fcv_tag_continuous: None,
        };

        assert_eq!(&mv_config.get_fcv_tags_for_lts(), "fcv_fallback")
    }

    // tests for get_fcv_tags_for_continuous.
    #[test]
    fn test_get_fcv_tags_for_continuous_should_use_continuous_if_provided() {
        let mv_config = MultiversionConfig {
            last_versions: vec![],
            requires_fcv_tag: "fcv_fallback".to_string(),
            requires_fcv_tag_lts: None,
            requires_fcv_tag_continuous: Some("fcv_continuous_explicit".to_string()),
        };

        assert_eq!(
            &mv_config.get_fcv_tags_for_continuous(),
            "fcv_continuous_explicit"
        )
    }

    #[test]
    fn test_get_fcv_tags_for_continuous_should_fallback_if_no_continuous_provided() {
        let mv_config = MultiversionConfig {
            last_versions: vec![],
            requires_fcv_tag: "fcv_fallback".to_string(),
            requires_fcv_tag_lts: None,
            requires_fcv_tag_continuous: None,
        };

        assert_eq!(&mv_config.get_fcv_tags_for_continuous(), "fcv_fallback")
    }
}
