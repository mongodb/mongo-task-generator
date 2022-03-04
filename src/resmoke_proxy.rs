use std::{path::Path, str::FromStr, time::Instant};

use anyhow::{bail, Result};
use cmd_lib::run_fun;
use serde::Deserialize;
use tracing::{event, Level};
use yaml_rust::{ScanError, Yaml, YamlLoader};

const SHARDED_CLUSTER_FIXTURE_NAME: &str = "ShardedClusterFixture";
const REPLICA_SET_FIXTURE_NAME: &str = "ReplicaSetFixture";

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

/// Types of fixtures used by resmoke suites.
#[derive(Debug, PartialEq, Clone)]
pub enum SuiteFixtureType {
    /// A suite with no fixtures defined.
    Shell,
    /// A ReplicaSet fixture.
    Repl,
    /// A Sharded fixture.
    Shard,
    /// Some other fixture.
    Other,
}

/// Configuration of a resmoke test suite.
#[derive(Debug, Clone)]
pub struct ResmokeSuiteConfig {
    /// Yaml contents of the configuration.
    config: Yaml,
}

impl Default for ResmokeSuiteConfig {
    fn default() -> Self {
        Self { config: Yaml::Null }
    }
}

impl FromStr for ResmokeSuiteConfig {
    type Err = ScanError;

    /// Read Resmoke suite configuration from the given string.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let suite_config = YamlLoader::load_from_str(s)?;
        Ok(Self {
            config: suite_config[0].to_owned(),
        })
    }
}

impl ResmokeSuiteConfig {
    /// Get the fixture type of this suite.
    pub fn get_fixture_type(&self) -> Result<SuiteFixtureType> {
        let executor = self.get_executor()?;
        match executor {
            Yaml::Hash(executor) => {
                if let Some(fixture) = executor.get(&Yaml::from_str("fixture")) {
                    Ok(Self::get_type_from_fixture_class(fixture))
                } else {
                    Ok(SuiteFixtureType::Shell)
                }
            }
            _ => bail!("Expected map as executor"),
        }
    }

    /// Get the type of the given fixture class.
    ///
    /// # Arguments
    ///
    /// * `fixture` - Yaml representation of the fixture configuration.
    ///
    /// # Returns
    ///
    /// Type of fixture the suite uses.
    fn get_type_from_fixture_class(fixture: &Yaml) -> SuiteFixtureType {
        if let Yaml::Hash(fixture) = fixture {
            if let Some(Yaml::String(fixture_class)) = fixture.get(&Yaml::from_str("class")) {
                return match fixture_class.as_str() {
                    SHARDED_CLUSTER_FIXTURE_NAME => SuiteFixtureType::Shard,
                    REPLICA_SET_FIXTURE_NAME => SuiteFixtureType::Repl,
                    _ => SuiteFixtureType::Other,
                };
            }
        }
        SuiteFixtureType::Other
    }

    /// Get the executor section of this configuration.
    fn get_executor(&self) -> Result<&Yaml> {
        match &self.config {
            Yaml::Hash(map) => Ok(map.get(&Yaml::from_str("executor")).unwrap()),
            _ => bail!("Expected map at root of resmoke config"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // get_fixture_type tests.
    #[test]
    fn test_no_fixture_defined_should_return_shell() {
        let config_yaml = "
            test_kind: js_test

            selector:
              roots:
                - jstests/auth/*.js
              exclude_files:
                - jstests/auth/repl.js
        
            executor:
              config:
                shell_options:
                  global_vars:
                    TestData:
                      roleGraphInvalidationIsFatal: true
                  nodb: '' 
        ";

        let config = ResmokeSuiteConfig::from_str(config_yaml).unwrap();

        assert_eq!(config.get_fixture_type().unwrap(), SuiteFixtureType::Shell);
    }

    #[test]
    fn test_shared_cluster_fixture_should_return_sharded() {
        let config_yaml = "
            test_kind: js_test

            selector:
              roots:
                - jstests/auth/*.js
              exclude_files:
                - jstests/auth/repl.js
        
            executor:
              config:
                shell_options:
                  global_vars:
                    TestData:
                      roleGraphInvalidationIsFatal: true
                  nodb: '' 
              fixture:
                class: ShardedClusterFixture
                num_shards: 2
        ";

        let config = ResmokeSuiteConfig::from_str(config_yaml).unwrap();

        assert_eq!(config.get_fixture_type().unwrap(), SuiteFixtureType::Shard);
    }

    #[test]
    fn test_replica_set_fixture_should_return_repl() {
        let config_yaml = "
            test_kind: js_test

            selector:
              roots:
                - jstests/auth/*.js
              exclude_files:
                - jstests/auth/repl.js
        
            executor:
              config:
                shell_options:
                  global_vars:
                    TestData:
                      roleGraphInvalidationIsFatal: true
                  nodb: '' 
              fixture:
                class: ReplicaSetFixture
                num_nodes: 3
        ";

        let config = ResmokeSuiteConfig::from_str(config_yaml).unwrap();

        assert_eq!(config.get_fixture_type().unwrap(), SuiteFixtureType::Repl);
    }

    #[test]
    fn test_other_fixture_should_return_other() {
        let config_yaml = "
            test_kind: js_test

            selector:
              roots:
                - jstests/auth/*.js
              exclude_files:
                - jstests/auth/repl.js
        
            executor:
              config:
                shell_options:
                  global_vars:
                    TestData:
                      roleGraphInvalidationIsFatal: true
                  nodb: '' 
              fixture:
                num_nodes: 3
        ";

        let config = ResmokeSuiteConfig::from_str(config_yaml).unwrap();

        assert_eq!(config.get_fixture_type().unwrap(), SuiteFixtureType::Other);
    }
}
