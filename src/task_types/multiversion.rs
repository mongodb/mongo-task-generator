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

#[cfg(test)]
mod tests {
    // use std::str::FromStr;

    use crate::resmoke::{
        resmoke_proxy::{MultiversionConfig, TestDiscovery},
        resmoke_suite::ResmokeSuiteConfig,
    };

    use super::*;
    // use rstest::*;

    struct MockTestDiscovery {
        old_versions: Vec<String>,
        suite_config: Option<ResmokeSuiteConfig>,
    }

    impl TestDiscovery for MockTestDiscovery {
        fn discover_tests(&self, _suite_name: &str) -> Result<Vec<String>> {
            todo!()
        }

        fn get_suite_config(&self, _suite_name: &str) -> Result<ResmokeSuiteConfig> {
            if let Some(suite_config) = &self.suite_config {
                Ok(suite_config.clone())
            } else {
                todo!()
            }
        }

        fn get_multiversion_config(&self) -> Result<MultiversionConfig> {
            Ok(MultiversionConfig {
                last_versions: self.old_versions.clone(),
                requires_fcv_tag: "requires_fallback".to_string(),
                requires_fcv_tag_lts: Some("requires_v6.0,requires_v5.3,requires_v5.2".to_string()),
                requires_fcv_tag_continuous: Some("requires_v6.0".to_string()),
            })
        }
    }

    // #[test]
    // fn test_multiversion_iterator() {
    //     let suite_config_yaml = "
    //         test_kind: js_test

    //         selector:
    //           roots:
    //             - jstests/auth/*.js
    //           exclude_files:
    //             - jstests/auth/repl.js
    //
    //         executor:
    //           config:
    //             shell_options:
    //               global_vars:
    //                 TestData:
    //                   roleGraphInvalidationIsFatal: true
    //               nodb: ''
    //           fixture:
    //             class: ReplicaSetFixture
    //             num_nodes: 3
    //     ";
    //     let discovery_service = Arc::new(MockTestDiscovery {
    //         old_versions: vec!["lts".to_string(), "continuous".to_string()],
    //         suite_config: Some(ResmokeSuiteConfig::from_str(suite_config_yaml).unwrap()),
    //     });
    //     let multiversion_service = MultiversionServiceImpl::new(discovery_service).unwrap();

    //     let mut seen_combos = 0;
    //     for _ in multiversion_service.multiversion_iter("my suite").unwrap() {
    //         seen_combos += 1;
    //     }

    //     assert_eq!(seen_combos, 2 * 3); // 2 old_versions * 3 version_combinations.
    // }

    // #[test]
    // fn test_mv_get_version_combinations() {
    //     let suite_config_yaml = "
    //         test_kind: js_test

    //         selector:
    //           roots:
    //             - jstests/auth/*.js
    //           exclude_files:
    //             - jstests/auth/repl.js
    //
    //         executor:
    //           config:
    //             shell_options:
    //               global_vars:
    //                 TestData:
    //                   roleGraphInvalidationIsFatal: true
    //               nodb: ''
    //           fixture:
    //             class: ReplicaSetFixture
    //             num_nodes: 3
    //     ";
    //     let discovery_service = Arc::new(MockTestDiscovery {
    //         old_versions: vec!["lts".to_string(), "continuous".to_string()],
    //         suite_config: Some(ResmokeSuiteConfig::from_str(suite_config_yaml).unwrap()),
    //     });
    //     let multiversion_service = MultiversionServiceImpl::new(discovery_service).unwrap();

    //     let combos = multiversion_service
    //         .get_version_combinations("my suite")
    //         .unwrap();

    //     assert_eq!(combos, vec!["new_new_old", "new_old_new", "old_new_new"]);
    // }

    // #[rstest]
    // #[case(SuiteFixtureType::Shard, vec!["new_old_old_new"])]
    // #[case(SuiteFixtureType::Repl, vec!["new_new_old", "new_old_new", "old_new_new"])]
    // #[case(SuiteFixtureType::Shell, vec![""])]
    // #[case(SuiteFixtureType::Other, vec![""])]
    // fn test_get_version_combinations(
    //     #[case] fixture_type: SuiteFixtureType,
    //     #[case] expected_combos: Vec<&str>,
    // ) {
    //     let combos = get_version_combinations(&fixture_type);

    //     assert_eq!(combos, expected_combos);
    // }

    // #[rstest]
    // #[case("the_suite", "lts", "new_old_old_new", "the_suite_lts_new_old_old_new")]
    // #[case("the_suite", "lts", "", "the_suite_lts")]
    // fn test_name_multiversion_suite(
    //     #[case] base_name: &str,
    //     #[case] old_version: &str,
    //     #[case] version_combination: &str,
    //     #[case] expected_name: &str,
    // ) {
    //     let discovery_service = Arc::new(MockTestDiscovery {
    //         old_versions: vec![],
    //         suite_config: None,
    //     });
    //     let multiversion_service = MultiversionServiceImpl::new(discovery_service.get_multiversion_config()?);

    //     let suite_name = multiversion_service.name_multiversion_suite(
    //         base_name,
    //         old_version,
    //         version_combination,
    //     );

    //     assert_eq!(suite_name, expected_name);
    // }

    // tests for exclude_tags_for_task.

    // #[rstest]
    // #[case(None, "sharding_backport_required_multiversion,requires_fallback")]
    // #[case(Some("last_lts".to_string()), "sharding_backport_required_multiversion,requires_v6.0,requires_v5.3,requires_v5.2")]
    // #[case(Some("last_continuous".to_string()), "sharding_backport_required_multiversion,requires_v6.0")]
    // fn test_exclude_tags_for_task(#[case] mv_mode: Option<String>, #[case] extra_tags: &str) {
    //     let discovery_service = Arc::new(MockTestDiscovery {
    //         old_versions: vec![],
    //         suite_config: None,
    //     });
    //     let multiversion_service = MultiversionServiceImpl::new(discovery_service.get_multiversion_config()?);

    //     let task_name = "sharding";
    //     let tags = multiversion_service.exclude_tags_for_task(task_name, mv_mode);

    //     assert_eq!(
    //         tags,
    //         format!(
    //             "multiversion_incompatible,backport_required_multiversion,{}",
    //             extra_tags
    //         )
    //     );
    // }
}
