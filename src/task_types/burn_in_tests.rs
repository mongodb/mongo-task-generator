use anyhow::Result;
use shrub_rs::models::task::EvgTask;
use std::{collections::HashMap, sync::Arc};

use crate::{
    resmoke::burn_in_proxy::{BurnInDiscovery, DiscoveredTask},
    services::config_extraction::ConfigExtractionService,
    task_types::resmoke_tasks::{GeneratedResmokeSuite, SubSuite},
};

use super::{
    generated_suite::GeneratedSuite,
    multiversion::MultiversionService,
    resmoke_tasks::{GenResmokeTaskService, ResmokeGenParams},
};

/// Options to pass to resmoke to enable burn_in repetition.
const BURN_IN_REPEAT_CONFIG: &str =
    "--repeatTestsSecs=600 --repeatTestsMin=2 --repeatTestsMax=1000";
/// How to label burn_in generated sub_tasks.
const BURN_IN_LABEL: &str = "burn_in";

/// A service for generating burn_in tasks.
pub trait BurnInService: Sync + Send {
    /// Generate a burn_in_task for the given build variant.
    ///
    /// # Arguments
    ///
    /// * `build_variant` - Name of build variant to generate burn_in for.
    /// * `task_map` - Map of task definitions found in the evergreen project configuration.
    ///
    /// # Returns
    ///
    /// A generated task for burn_in on the given build variant.
    fn generate_burn_in_suite(
        &self,
        build_variant: &str,
        task_map: Arc<HashMap<String, EvgTask>>,
    ) -> Result<Box<dyn GeneratedSuite>>;
}

pub struct BurnInServiceImpl {
    /// Burn in discovery service.
    burn_in_discovery: Arc<dyn BurnInDiscovery>,

    /// Service to generate resmoke tasks.
    gen_resmoke_task_service: Arc<dyn GenResmokeTaskService>,

    /// Service to extraction configuration from evergreen project data.
    config_extraction_service: Arc<dyn ConfigExtractionService>,

    /// Service to get multiversion configuration.
    multiversion_service: Arc<dyn MultiversionService>,
}

/// Information about a suite being generated in burn_in.
#[derive(Debug, Default)]
struct BurnInSuiteInfo<'a> {
    /// Name of build variant being generated for.
    build_variant: &'a str,

    /// Total number of tests being generated for suite.
    total_tests: usize,

    /// Name of the task being generated.
    task_name: &'a str,

    /// The multiversion name being generated.
    multiversion_name: Option<&'a str>,

    /// Any multiversion tags that should be included.
    multiversion_tags: Option<String>,
}

impl<'a> BurnInSuiteInfo<'a> {
    /// Create the origin suite that should be used.
    ///
    /// # Arguments
    ///
    /// * `suite_name` - Name of suite being used.
    fn build_origin_suite(&self, suite_name: &str) -> String {
        self.multiversion_name.unwrap_or(suite_name).to_string()
    }

    /// Create the task_name to use for this suite.
    fn build_task_name(&self) -> &'a str {
        self.multiversion_name.unwrap_or(self.task_name)
    }

    /// Create the display name to use for this suite.
    fn build_display_name(&self) -> String {
        format!(
            "{}:{}-{}",
            BURN_IN_LABEL,
            self.build_task_name(),
            self.build_variant
        )
    }
}

impl BurnInServiceImpl {
    /// Create a new instance of the burn_in_service.
    ///
    /// # Arguments
    ///
    /// * `burn_in_discovery` - Burn in discovery service.
    /// * `gen_resmoke_task_service` - Service to generate resmoke tasks.
    /// * `config_extraction_service` - Service to extraction configuration from evergreen project data.
    /// * `multiversion_service` - Service to get multiversion configuration.
    pub fn new(
        burn_in_discovery: Arc<dyn BurnInDiscovery>,
        gen_resmoke_task_service: Arc<dyn GenResmokeTaskService>,
        config_extraction_service: Arc<dyn ConfigExtractionService>,
        multiversion_service: Arc<dyn MultiversionService>,
    ) -> Self {
        BurnInServiceImpl {
            burn_in_discovery,
            gen_resmoke_task_service,
            config_extraction_service,
            multiversion_service,
        }
    }

    /// Build the burn_in tests for the given task.
    ///
    /// # Arguments
    ///
    /// * `discovered_task` - Task discovered to pull into resmoke.
    /// * `task_def` - Evergreen project definition of task.
    /// * `build_variant` - Name of build variant being generated for.
    ///
    /// # Returns
    ///
    /// List of sub_tasks to include as part of burn_in.
    fn build_tests_for_task(
        &self,
        discovered_task: &DiscoveredTask,
        task_def: &EvgTask,
        build_variant: &str,
    ) -> Result<Vec<EvgTask>> {
        let mut sub_suites = vec![];
        for (index, test) in discovered_task.test_list.iter().enumerate() {
            let mut params = self
                .config_extraction_service
                .task_def_to_resmoke_params(task_def, false)?;
            update_resmoke_params_for_burn_in(&mut params, test);

            if params.generate_multiversion_combos {
                for (old_version, version_combination) in self
                    .multiversion_service
                    .multiversion_iter(&params.suite_name)?
                {
                    let multiversion_name = self.multiversion_service.name_multiversion_suite(
                        &params.suite_name,
                        &old_version,
                        &version_combination,
                    );
                    let multiversion_tags = Some(old_version.clone());

                    let burn_in_suite_info = BurnInSuiteInfo {
                        build_variant,
                        total_tests: discovered_task.test_list.len(),
                        task_name: &discovered_task.task_name,
                        multiversion_name: Some(&multiversion_name),
                        multiversion_tags,
                    };

                    sub_suites.push(self.create_task(&params, index, test, &burn_in_suite_info));
                }
            } else {
                let burn_in_suite_info = BurnInSuiteInfo {
                    build_variant,
                    total_tests: discovered_task.test_list.len(),
                    task_name: &discovered_task.task_name,
                    multiversion_name: None,
                    multiversion_tags: None,
                };
                sub_suites.push(self.create_task(&params, index, test, &burn_in_suite_info))
            }
        }

        Ok(sub_suites)
    }

    /// Create an individual execution task for burn_in.
    ///
    /// # Arguments
    ///
    /// * `params` - Configuration for how suite should be configured.
    /// * `index` - Index of sub-task in list of sub-tasks being created for the task.
    /// * `test` - Name of test to generate sub-suite for.
    /// * `suite_info` - Information about the suite being generated.
    ///
    /// # Returns
    ///
    /// Shrub task representing the given sub-task.
    fn create_task(
        &self,
        params: &ResmokeGenParams,
        index: usize,
        test: &str,
        suite_info: &BurnInSuiteInfo,
    ) -> EvgTask {
        let origin_suite = suite_info.build_origin_suite(&params.suite_name);

        let sub_suite = SubSuite {
            index: Some(index),
            name: suite_info.build_display_name(),
            test_list: vec![test.to_string()],
            exclude_test_list: None,
            origin_suite: origin_suite.clone(),
            mv_exclude_tags: suite_info.multiversion_tags.clone(),
            is_enterprise: false,
        };

        self.gen_resmoke_task_service.build_resmoke_sub_task(
            &sub_suite,
            suite_info.total_tests,
            params,
            Some(origin_suite),
        )
    }
}

impl BurnInService for BurnInServiceImpl {
    /// Generate the burn_in_tests task for the given build_variant.
    ///
    /// # Arguments
    ///
    /// * `build_variant` - Name of build variant to generate burn_in_tests for.
    /// * `task_map` - Map of task definitions in evergreen project configuration.
    ///
    /// # Returns
    ///
    /// A generated suite to use for generating burn_in_tests.
    fn generate_burn_in_suite(
        &self,
        build_variant: &str,
        task_map: Arc<HashMap<String, EvgTask>>,
    ) -> Result<Box<dyn GeneratedSuite>> {
        let mut sub_suites = vec![];
        let discovered_tasks = self.burn_in_discovery.discover_tasks(build_variant)?;
        for discovered_task in discovered_tasks {
            let task_name = &discovered_task.task_name;
            if let Some(task_def) = task_map.get(task_name) {
                sub_suites.extend(self.build_tests_for_task(
                    &discovered_task,
                    task_def,
                    build_variant,
                )?);
            }
        }

        Ok(Box::new(GeneratedResmokeSuite {
            task_name: "burn_in_tests".to_string(),
            sub_suites,
            use_large_distro: false,
        }))
    }
}

/// Update the given resmoke parameters to include burn_in configuration for the given test.
///
/// # Arguments
///
/// * `params` - resmoke parameters to update.
/// * `test_name` - Name of test to run.
fn update_resmoke_params_for_burn_in(params: &mut ResmokeGenParams, test_name: &str) {
    params.resmoke_args = format!(
        "{} {} {}",
        params.resmoke_args, BURN_IN_REPEAT_CONFIG, test_name
    );
}

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use shrub_rs::models::variant::BuildVariant;

    use crate::task_types::{
        fuzzer_tasks::FuzzerGenTaskParams, multiversion::MultiversionIterator,
    };

    use super::*;

    // build_origin_suite tests.
    #[test]
    fn test_build_origin_suite_should_use_suite_name_when_no_mv() {
        let suite_name = "my suite";
        let suite_info = BurnInSuiteInfo {
            ..Default::default()
        };

        assert_eq!(suite_info.build_origin_suite(suite_name), suite_name);
    }

    #[test]
    fn test_build_origin_suite_should_use_multiversion_name_when_provided() {
        let suite_name = "my suite";
        let suite_info = BurnInSuiteInfo {
            multiversion_name: Some("multiversion_suite"),
            ..Default::default()
        };

        assert_eq!(
            suite_info.build_origin_suite(suite_name),
            "multiversion_suite"
        );
    }

    // build_task_name tests.
    #[test]
    fn test_build_task_name_should_use_task_name_if_no_mv() {
        let task_name = "my task";
        let suite_info = BurnInSuiteInfo {
            task_name,
            ..Default::default()
        };

        assert_eq!(suite_info.build_task_name(), task_name);
    }

    #[test]
    fn test_build_task_name_should_use_multiversion_name_when_provided() {
        let task_name = "my task";
        let suite_info = BurnInSuiteInfo {
            task_name,
            multiversion_name: Some("multiversion_suite"),
            ..Default::default()
        };

        assert_eq!(suite_info.build_task_name(), "multiversion_suite");
    }

    // build_display_name tests.
    #[test]
    fn test_display_name_should_include_all_components() {
        let task_name = "my_task";
        let build_variant = "my_build_variant";
        let suite_info = BurnInSuiteInfo {
            task_name,
            build_variant,
            ..Default::default()
        };

        let display_name = suite_info.build_display_name();

        assert!(display_name.contains(BURN_IN_LABEL));
        assert!(display_name.contains(task_name));
        assert!(display_name.contains(build_variant));
    }

    // Mocks
    struct MockBurnInDiscovery {}
    impl BurnInDiscovery for MockBurnInDiscovery {
        fn discover_tasks(&self, _build_variant: &str) -> Result<Vec<DiscoveredTask>> {
            todo!()
        }
    }

    struct MockGenResmokeTasksService {}
    #[async_trait]
    impl GenResmokeTaskService for MockGenResmokeTasksService {
        async fn generate_resmoke_task(
            &self,
            _params: &ResmokeGenParams,
            _build_variant: &str,
        ) -> Result<Box<dyn GeneratedSuite>> {
            todo!()
        }

        fn build_resmoke_sub_task(
            &self,
            _sub_suite: &SubSuite,
            _total_sub_suites: usize,
            _params: &ResmokeGenParams,
            _suite_override: Option<String>,
        ) -> EvgTask {
            EvgTask {
                ..Default::default()
            }
        }
    }

    struct MockConfigExtractionService {
        pub is_multiversion: bool,
    }
    impl ConfigExtractionService for MockConfigExtractionService {
        fn task_def_to_fuzzer_params(
            &self,
            _task_def: &EvgTask,
            _build_variant: &BuildVariant,
        ) -> Result<FuzzerGenTaskParams> {
            todo!()
        }

        fn task_def_to_resmoke_params(
            &self,
            _task_def: &EvgTask,
            _is_enterprise: bool,
        ) -> Result<ResmokeGenParams> {
            Ok(ResmokeGenParams {
                generate_multiversion_combos: self.is_multiversion,
                ..Default::default()
            })
        }
    }

    struct MockMultiversionService {
        old_version: Vec<String>,
        version_combos: Vec<String>,
    }
    impl MultiversionService for MockMultiversionService {
        fn get_version_combinations(&self, _suite_name: &str) -> Result<Vec<String>> {
            todo!()
        }

        fn multiversion_iter(&self, _suite_name: &str) -> Result<MultiversionIterator> {
            Ok(MultiversionIterator::new(
                &self.old_version,
                &self.version_combos,
            ))
        }

        fn name_multiversion_suite(
            &self,
            base_name: &str,
            _old_version: &str,
            _version_combination: &str,
        ) -> String {
            base_name.to_string()
        }

        fn exclude_tags_for_task(&self, _task_name: &str, _mv_mode: Option<String>) -> String {
            todo!()
        }
    }

    fn build_mocked_service() -> BurnInServiceImpl {
        BurnInServiceImpl::new(
            Arc::new(MockBurnInDiscovery {}),
            Arc::new(MockGenResmokeTasksService {}),
            Arc::new(MockConfigExtractionService {
                is_multiversion: false,
            }),
            Arc::new(MockMultiversionService {
                old_version: vec![],
                version_combos: vec![],
            }),
        )
    }

    fn build_mv_mocked_service(
        old_version: Vec<String>,
        version_combos: Vec<String>,
    ) -> BurnInServiceImpl {
        BurnInServiceImpl::new(
            Arc::new(MockBurnInDiscovery {}),
            Arc::new(MockGenResmokeTasksService {}),
            Arc::new(MockConfigExtractionService {
                is_multiversion: true,
            }),
            Arc::new(MockMultiversionService {
                old_version,
                version_combos,
            }),
        )
    }

    // build_tests_for_task tests.
    #[test]
    fn test_build_test_for_tasks_creates_task_for_each_test() {
        let discovered_task = DiscoveredTask {
            task_name: "my task".to_string(),
            test_list: vec!["test_0.js".to_string(), "test_1.js".to_string()],
        };
        let task_def = EvgTask {
            ..Default::default()
        };
        let build_variant = "my_build_variant";
        let burn_in_service = build_mocked_service();

        let tasks = burn_in_service
            .build_tests_for_task(&discovered_task, &task_def, build_variant)
            .unwrap();

        assert_eq!(tasks.len(), discovered_task.test_list.len());
    }

    #[test]
    fn test_build_test_for_tasks_creates_task_for_each_multiversion_iteration_and_test() {
        let discovered_task = DiscoveredTask {
            task_name: "my task".to_string(),
            test_list: vec!["test_0.js".to_string(), "test_1.js".to_string()],
        };
        let task_def = EvgTask {
            ..Default::default()
        };
        let build_variant = "my_build_variant";
        let old_version = vec!["lts".to_string(), "continuous".to_string()];
        let version_combos = vec!["new_old_new".to_string(), "old_new_old".to_string()];
        let burn_in_service = build_mv_mocked_service(old_version.clone(), version_combos.clone());

        let tasks = burn_in_service
            .build_tests_for_task(&discovered_task, &task_def, build_variant)
            .unwrap();

        assert_eq!(
            tasks.len(),
            discovered_task.test_list.len() * old_version.len() * version_combos.len()
        );
    }
}
