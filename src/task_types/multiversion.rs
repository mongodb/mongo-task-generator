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

use std::sync::Arc;

use anyhow::Result;

use crate::resmoke_proxy::{SuiteFixtureType, TestDiscovery};

/// A service for helping generating multiversion tasks.
pub trait MultiversionService: Sync + Send {
    /// Get a list of multiversion combinations to generate tests for.
    ///
    /// # Arguments
    ///
    /// * `suite_name` - Name of suite being generated.
    ///
    /// # Returns
    ///
    /// List of version combinations to create tests for.
    fn get_version_combinations(&self, suite_name: &str) -> Result<Vec<String>>;

    /// Get an iterator over the multiversion combinations to generate.
    ///
    /// # Arguments
    ///
    /// * `version_combination` - Version combinations that should be included for the suite.
    ///
    /// # Returns
    ///
    /// An iterator over the multiversion configurations to generate. Each iteration will
    /// include a tuple with the `old_version` and the `version_combinations` to use.
    fn multiversion_iter(&self, version_combinations: &[String]) -> MultiversionIterator;
}

/// Implementation of Multiversion service.
pub struct MultiversionServiceImpl {
    /// Service to gather details about test suites.
    discovery_service: Arc<dyn TestDiscovery>,
    /// Old versions that need to be tested against.
    old_versions: Vec<String>,
}

/// Implementation of Multiversion service.
impl MultiversionServiceImpl {
    /// Create a new instance of Multiversion service.
    ///
    /// # Arguments
    ///
    /// * `discovery_service` - Instance of service to query details about test suites.
    pub fn new(discovery_service: Arc<dyn TestDiscovery>) -> Result<Self> {
        let old_versions = discovery_service.get_multiversion_config()?.last_versions;
        Ok(Self {
            discovery_service,
            old_versions,
        })
    }
}

impl MultiversionService for MultiversionServiceImpl {
    /// Get a list of multiversion combinations to generate tests for.
    ///
    /// # Arguments
    ///
    /// * `suite_name` - Name of suite being generated.
    ///
    /// # Returns
    ///
    /// List of version combinations to create tests for.
    fn get_version_combinations(&self, suite_name: &str) -> Result<Vec<String>> {
        let suite_config = self.discovery_service.get_suite_config(suite_name)?;
        let fixture_type = suite_config.get_fixture_type()?;
        Ok(get_version_combinations(&fixture_type))
    }

    /// Get an iterator over the multiversion combinations to generate.
    ///
    /// # Arguments
    ///
    /// * `version_combination` - Version combinations that should be included for the suite.
    ///
    /// # Returns
    ///
    /// An iterator over the multiversion configurations to generate. Each iteration will
    /// include a tuple with the `old_version` and the `version_combinations` to use.
    fn multiversion_iter(&self, version_combinations: &[String]) -> MultiversionIterator {
        MultiversionIterator::new(&self.old_versions, version_combinations)
    }
}

/// Iterator over multiversion configurations to generate.
pub struct MultiversionIterator {
    /// Multiversion combinations.
    combinations: Vec<(String, String)>,
}

impl MultiversionIterator {
    /// Create a new multiversion iterator for the given old_version and version_combinations.
    ///
    /// # Arguments
    ///
    /// * `old_versions` - Old versions to generate sub-tasks for.
    /// * `version_combinations` - Version combinations to generate sub-tasks for.
    pub fn new(old_versions: &[String], version_combinations: &[String]) -> Self {
        let mut combinations = vec![];
        for version in old_versions {
            for combination in version_combinations {
                combinations.push((version.to_string(), combination.to_string()));
            }
        }

        MultiversionIterator { combinations }
    }
}

impl Iterator for MultiversionIterator {
    type Item = (String, String);

    fn next(&mut self) -> Option<Self::Item> {
        self.combinations.pop()
    }
}

/// Get the version combinations to use for the given fixture type.
///
/// # Arguments
///
/// * `fixture_type` - Fixture type to query.
///
/// # Returns
///
/// List of version combinations to generate tests for.
fn get_version_combinations(fixture_type: &SuiteFixtureType) -> Vec<String> {
    match fixture_type {
        SuiteFixtureType::Shard => vec!["new_old_old_new".to_string()],
        SuiteFixtureType::Repl => ["new_new_old", "new_old_new", "old_new_new"]
            .iter()
            .map(|v| v.to_string())
            .collect(),
        _ => vec!["".to_string()],
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::resmoke_proxy::ResmokeSuiteConfig;

    use super::*;
    use rstest::*;

    struct MockTestDiscovery {
        old_versions: Vec<String>,
        suite_config: Option<ResmokeSuiteConfig>,
    }

    impl TestDiscovery for MockTestDiscovery {
        fn discover_tests(&self, _suite_name: &str) -> Result<Vec<String>> {
            todo!()
        }

        fn get_suite_config(
            &self,
            _suite_name: &str,
        ) -> Result<crate::resmoke_proxy::ResmokeSuiteConfig> {
            if let Some(suite_config) = &self.suite_config {
                Ok(suite_config.clone())
            } else {
                todo!()
            }
        }

        fn get_multiversion_config(&self) -> Result<crate::resmoke_proxy::MultiversionConfig> {
            Ok(crate::resmoke_proxy::MultiversionConfig {
                last_versions: self.old_versions.clone(),
            })
        }
    }

    #[test]
    fn test_multiversion_iterator() {
        let discovery_service = Arc::new(MockTestDiscovery {
            old_versions: vec!["lts".to_string(), "continuous".to_string()],
            suite_config: None,
        });
        let multiversion_service = MultiversionServiceImpl::new(discovery_service).unwrap();

        let mut seen_combos = 0;
        for _ in multiversion_service
            .multiversion_iter(&["new_old_new".to_string(), "old_new_old".to_string()])
        {
            seen_combos += 1;
        }

        assert_eq!(seen_combos, 2 * 2); // 2 old_versions * 2 version_combinations.
    }

    #[test]
    fn test_mv_get_version_combinations() {
        let suite_config_yaml = "
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
        let discovery_service = Arc::new(MockTestDiscovery {
            old_versions: vec!["lts".to_string(), "continuous".to_string()],
            suite_config: Some(ResmokeSuiteConfig::from_str(suite_config_yaml).unwrap()),
        });
        let multiversion_service = MultiversionServiceImpl::new(discovery_service).unwrap();

        let combos = multiversion_service
            .get_version_combinations("my suite")
            .unwrap();

        assert_eq!(combos, vec!["new_new_old", "new_old_new", "old_new_new"]);
    }

    #[rstest]
    #[case(SuiteFixtureType::Shard, vec!["new_old_old_new"])]
    #[case(SuiteFixtureType::Repl, vec!["new_new_old", "new_old_new", "old_new_new"])]
    #[case(SuiteFixtureType::Shell, vec![""])]
    #[case(SuiteFixtureType::Other, vec![""])]
    fn test_get_version_combinations(
        #[case] fixture_type: SuiteFixtureType,
        #[case] expected_combos: Vec<&str>,
    ) {
        let combos = get_version_combinations(&fixture_type);

        assert_eq!(combos, expected_combos);
    }
}
