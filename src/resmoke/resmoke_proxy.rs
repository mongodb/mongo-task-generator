use std::{path::Path, str::FromStr, time::Instant};

use anyhow::Result;
use serde::Deserialize;
use tracing::{error, event, Level};

use crate::evergreen::evg_config_utils::is_bazel_suite;

use super::{external_cmd::run_command, resmoke_suite::ResmokeSuiteConfig};
use std::collections::HashMap;

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
    /// True if the generator should skip tests already run in more complex suites.
    skip_covered_tests: bool,
    /// True if test discovery should include tests that are tagged with fully disabled features.
    include_fully_disabled_feature_tests: bool,
    bazel_suite_configs: BazelConfigs,
}

impl ResmokeProxy {
    /// Create a new `ResmokeProxy` instance.
    ///
    /// # Arguments
    ///
    /// * `resmoke_cmd` - Command to invoke resmoke.
    /// * `skip_covered_tests` - Whether the generator should skip tests run in more complex suites.
    /// * `include_fully_disabled_feature_tests` - If the generator should include tests that are tagged with fully disabled features.
    /// * `bazel_suite_configs` - Optional bazel suite configurations.
    pub fn new(
        resmoke_cmd: &str,
        skip_covered_tests: bool,
        include_fully_disabled_feature_tests: bool,
        bazel_suite_configs: BazelConfigs,
    ) -> Self {
        let cmd_parts: Vec<_> = resmoke_cmd.split(' ').collect();
        let cmd = cmd_parts[0];
        let script = cmd_parts[1..].iter().map(|s| s.to_string()).collect();
        Self {
            resmoke_cmd: cmd.to_string(),
            resmoke_script: script,
            skip_covered_tests,
            include_fully_disabled_feature_tests,
            bazel_suite_configs,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BazelConfigs {
    /// Map of bazel resmoke config targets to their generated suite config YAMLs.
    configs: HashMap<String, String>,
}

impl BazelConfigs {
    pub fn from_yaml_file(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let configs: Result<HashMap<String, String>, serde_yaml::Error> =
            serde_yaml::from_str(&contents);
        if configs.is_err() {
            error!(
                file = path.display().to_string(),
                contents = &contents,
                "Failed to parse bazel configs from yaml file",
            );
        }
        Ok(Self { configs: configs? })
    }

    /// Get the generated suite config for a bazel resmoke target.
    ///
    /// # Arguments
    ///
    /// * `target` - Bazel resmoke test target, like "//buildscripts/resmoke:core".
    ///
    /// # Returns
    ///
    /// The path the the generated suite config YAML, like "bazel-out/buildscripts/resmoke/core_config.yml".
    pub fn get(&self, target: &str) -> &str {
        match self.configs.get(&format!("{}_config", target)) {
            Some(config) => config,
            None => {
                panic!("No bazel config found for target: {}", target);
            }
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
        let suite_config = if is_bazel_suite(suite_name) {
            self.bazel_suite_configs.get(suite_name)
        } else {
            suite_name
        };

        let mut cmd = vec![&*self.resmoke_cmd];
        cmd.append(&mut self.resmoke_script.iter().map(|s| s.as_str()).collect());
        cmd.append(&mut vec!["test-discovery", "--suite", suite_config]);

        // When running in a patch build, we use the --skipTestsCoveredByMoreComplexSuites
        // flag to tell Resmoke to exclude any tests in the given suite that will
        // also be run on a more complex suite.
        if self.skip_covered_tests {
            cmd.append(&mut vec!["--skipTestsCoveredByMoreComplexSuites"]);
        }

        if self.include_fully_disabled_feature_tests {
            cmd.append(&mut vec!["--includeFullyDisabledFeatureTests"]);
        }

        let start = Instant::now();
        let cmd_output = run_command(&cmd).unwrap();

        event!(
            Level::INFO,
            suite_config,
            duration_ms = start.elapsed().as_millis() as u64,
            "Resmoke test discovery finished"
        );

        let output: Result<TestDiscoveryOutput, serde_yaml::Error> =
            serde_yaml::from_str(&cmd_output);
        if output.is_err() {
            error!(
                command = cmd.join(" "),
                command_output = &cmd_output,
                "Failed to parse yaml from discover tests command output",
            );
        }

        Ok(output?
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
        let suite_config = if is_bazel_suite(suite_name) {
            self.bazel_suite_configs.get(suite_name)
        } else {
            suite_name
        };

        let mut cmd = vec![&*self.resmoke_cmd];
        cmd.append(&mut self.resmoke_script.iter().map(|s| s.as_str()).collect());
        cmd.append(&mut vec!["suiteconfig", "--suite", suite_config]);
        let cmd_output = run_command(&cmd).unwrap();

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
        let mut cmd = vec![cmd];
        let file_name = "multiversion-config.yml";
        cmd.append(&mut script.iter().map(|s| s.as_str()).collect());
        cmd.append(&mut vec!["multiversion-config"]);
        let file_arg = format!("--config-file-output={}", file_name);
        cmd.append(&mut vec![&file_arg]);
        run_command(&cmd).unwrap();
        let multiversion_config_output =
            std::fs::read_to_string(file_name).expect("Multiversion config file not found.");
        let multiversion_config: Result<MultiversionConfig, serde_yaml::Error> =
            serde_yaml::from_str(&multiversion_config_output);
        if multiversion_config.is_err() {
            error!(
                command = cmd.join(" "),
                command_output = &multiversion_config_output,
                "Failed to parse yaml from multiversion config command output",
            );
        }
        Ok(multiversion_config?)
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
