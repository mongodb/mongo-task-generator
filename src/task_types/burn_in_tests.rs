use anyhow::Result;
use shrub_rs::models::task::TaskDependency;
use shrub_rs::models::{
    task::{EvgTask, TaskRef},
    variant::{BuildVariant, DisplayTask},
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use crate::evergreen::evg_config_utils::EvgConfigUtils;
use crate::evergreen_names::{
    BURN_IN_TASKS, BURN_IN_TASK_NAME, COMPILE_VARIANT, VERSION_BURN_IN_GEN_TASK,
    VERSION_GEN_VARIANT,
};
use crate::{
    evergreen_names::BURN_IN_BYPASS,
    resmoke::burn_in_proxy::{BurnInDiscovery, DiscoveredTask},
    services::config_extraction::ConfigExtractionService,
    task_types::resmoke_tasks::{GeneratedResmokeSuite, SubSuite},
};

use super::generated_suite::GeneratedSubTask;
use super::{
    generated_suite::GeneratedSuite,
    resmoke_tasks::{GenResmokeTaskService, ResmokeGenParams},
};

/// Options to pass to resmoke to enable burn_in repetition.
const BURN_IN_REPEAT_CONFIG: &str =
    "--repeatTestsSecs=600 --repeatTestsMin=2 --repeatTestsMax=1000";
/// How to label burn_in generated sub_tasks.
const BURN_IN_LABEL: &str = "burn_in";
/// How to label burn_in generated sub_tasks.
const BURN_IN_TASK_LABEL: &str = "burn_in_task";
/// Number of tasks to generate for burn_in_tasks.
const BURN_IN_REPEAT_TASK_NUM: usize = 10;
/// Burn in display name prefix
const BURN_IN_DISPLAY_NAME_PREFIX: &str = "[jstests_affected]";

/// A service for generating burn_in tasks.
pub trait BurnInService: Sync + Send {
    /// Generate a burn_in_tests task for the given build variant.
    ///
    /// # Arguments
    ///
    /// * `build_variant` - Build variant to discover tasks for burn_in_tests.
    /// * `run_build_variant_name` - Name of build variant to generate burn_in_tests for.
    /// * `task_map` - Map of task definitions found in the evergreen project configuration.
    ///
    /// # Returns
    ///
    /// A generated task for burn_in_tests on the given build variant.
    fn generate_burn_in_suite(
        &self,
        build_variant: &BuildVariant,
        run_build_variant_name: &str,
        task_map: Arc<HashMap<String, EvgTask>>,
    ) -> Result<Box<dyn GeneratedSuite>>;

    /// Generate a burn_in_tags build variant for the given base build variant.
    ///
    /// # Arguments
    ///
    /// * `base_build_variant` - Build variant to generate burn_in_tags build variant based on.
    /// * `run_build_variant_name` - Build variant name to run burn_in_tests task on.
    /// * `generated_task` - Generated burn_in_tests task.
    /// * `compile_task_dependency` - Compile task name generated build variant should depend on.
    ///
    /// # Returns
    ///
    /// A generated burn_in_tags build variant based on another build variant.
    fn generate_burn_in_tags_build_variant(
        &self,
        base_build_variant: &BuildVariant,
        run_build_variant_name: String,
        generated_task: &dyn GeneratedSuite,
        compile_task_dependency: String,
    ) -> Result<BuildVariant>;

    /// Generate a burn_in_tasks task for the given build variant.
    ///
    /// # Arguments
    ///
    /// * `build_variant` - Build variant to discover tasks for burn_in_tasks.
    /// * `task_map` - Map of task definitions found in the evergreen project configuration.
    ///
    /// # Returns
    ///
    /// A generated task for burn_in_tasks on the given build variant.
    fn generate_burn_in_tasks_suite(
        &self,
        build_variant: &BuildVariant,
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

    /// Utilities to work with evergreen project configuration.
    evg_config_utils: Arc<dyn EvgConfigUtils>,
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

    /// How to label burn_in generated sub_tasks.
    burn_in_label: &'a str,

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
            self.burn_in_label,
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
    /// * `evg_config_utils` - Utilities to work with evergreen project configuration.
    pub fn new(
        burn_in_discovery: Arc<dyn BurnInDiscovery>,
        gen_resmoke_task_service: Arc<dyn GenResmokeTaskService>,
        config_extraction_service: Arc<dyn ConfigExtractionService>,
        evg_config_utils: Arc<dyn EvgConfigUtils>,
    ) -> Self {
        BurnInServiceImpl {
            burn_in_discovery,
            gen_resmoke_task_service,
            config_extraction_service,
            evg_config_utils,
        }
    }

    /// Build the burn_in_tests for the given task.
    ///
    /// # Arguments
    ///
    /// * `discovered_task` - Task discovered to pull into resmoke.
    /// * `task_def` - Evergreen project definition of task.
    /// * `run_build_variant` - Name of build variant to run burn_in_tests task on.
    ///
    /// # Returns
    ///
    /// List of sub_tasks to include as part of burn_in_tests.
    fn build_tests_for_task(
        &self,
        discovered_task: &DiscoveredTask,
        task_def: &EvgTask,
        run_build_variant: &str,
    ) -> Result<Vec<GeneratedSubTask>> {
        let mut sub_suites = vec![];
        for (index, test) in discovered_task.test_list.iter().enumerate() {
            let mut params = self
                .config_extraction_service
                .task_def_to_resmoke_params(task_def, false, None, None)?;
            update_resmoke_params_for_burn_in(&mut params, test);

            if params.require_multiversion_generate_tasks {
                for multiversion_task in params.multiversion_generate_tasks.as_ref().unwrap() {
                    let burn_in_suite_info = BurnInSuiteInfo {
                        build_variant: run_build_variant,
                        total_tests: discovered_task.test_list.len(),
                        task_name: &discovered_task.task_name,
                        burn_in_label: BURN_IN_LABEL,
                        multiversion_name: Some(&multiversion_task.suite_name),
                        multiversion_tags: Some(multiversion_task.old_version.clone()),
                    };

                    sub_suites.push(self.create_task(&params, index, &burn_in_suite_info));
                }
            } else {
                let burn_in_suite_info = BurnInSuiteInfo {
                    build_variant: run_build_variant,
                    total_tests: discovered_task.test_list.len(),
                    task_name: &discovered_task.task_name,
                    burn_in_label: BURN_IN_LABEL,
                    multiversion_name: None,
                    multiversion_tags: None,
                };
                sub_suites.push(self.create_task(&params, index, &burn_in_suite_info))
            }
        }

        Ok(sub_suites)
    }

    /// Build the burn_in_tasks for the given task.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Evergreen project definition of task.
    /// * `build_variant` - Name of build variant being generated for.
    ///
    /// # Returns
    ///
    /// List of sub_tasks to include as part of burn_in_tasks.
    fn build_burn_in_tasks_for_task(
        &self,
        task_def: &EvgTask,
        build_variant: &BuildVariant,
    ) -> Result<Vec<GeneratedSubTask>> {
        let mut sub_suites = vec![];
        for index in 0..BURN_IN_REPEAT_TASK_NUM {
            let params = self
                .config_extraction_service
                .task_def_to_resmoke_params(task_def, false, None, None)?;

            if params.require_multiversion_generate_tasks {
                for multiversion_task in params.multiversion_generate_tasks.as_ref().unwrap() {
                    let burn_in_suite_info = BurnInSuiteInfo {
                        build_variant: &build_variant.name,
                        total_tests: BURN_IN_REPEAT_TASK_NUM,
                        task_name: &task_def.name,
                        burn_in_label: BURN_IN_TASK_LABEL,
                        multiversion_name: Some(&multiversion_task.suite_name),
                        multiversion_tags: Some(multiversion_task.old_version.clone()),
                    };

                    sub_suites.push(self.create_task(&params, index, &burn_in_suite_info));
                }
            } else {
                let burn_in_suite_info = BurnInSuiteInfo {
                    build_variant: &build_variant.name,
                    total_tests: BURN_IN_REPEAT_TASK_NUM,
                    burn_in_label: BURN_IN_TASK_LABEL,
                    task_name: &task_def.name,
                    multiversion_name: None,
                    multiversion_tags: None,
                };
                sub_suites.push(self.create_task(&params, index, &burn_in_suite_info))
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
    /// * `suite_info` - Information about the suite being generated.
    ///
    /// # Returns
    ///
    /// Shrub task representing the given sub-task.
    fn create_task(
        &self,
        params: &ResmokeGenParams,
        index: usize,
        suite_info: &BurnInSuiteInfo,
    ) -> GeneratedSubTask {
        let origin_suite = suite_info.build_origin_suite(&params.suite_name);

        let sub_suite = SubSuite {
            index,
            name: suite_info.build_display_name(),
            test_list: vec![],
            exclude_test_list: None,
            origin_suite: origin_suite.clone(),
            mv_exclude_tags: suite_info.multiversion_tags.clone(),
            is_enterprise: false,
            platform: None,
        };

        self.gen_resmoke_task_service.build_resmoke_sub_task(
            &sub_suite,
            suite_info.total_tests,
            params,
            Some(origin_suite),
        )
    }
}

/// A container for configuration generated for a burn_in_tags build variant.
#[derive(Debug, Clone)]
struct BurnInTagsGeneratedConfig {
    /// Name of burn_in_tags build variant.
    pub build_variant_name: String,
    /// References to generated tasks that should be included.
    pub gen_task_specs: Vec<TaskRef>,
    /// Display name of burn_in_tags build variant.
    pub build_variant_display_name: Option<String>,
    /// Display tasks that should be created.
    pub display_tasks: Vec<DisplayTask>,
    /// Expansions that should be added to build variant.
    pub expansions: BTreeMap<String, String>,
}

impl BurnInTagsGeneratedConfig {
    /// Create an empty instance of generated configuration.
    pub fn new() -> Self {
        Self {
            build_variant_name: String::new(),
            gen_task_specs: vec![],
            build_variant_display_name: Some(String::new()),
            display_tasks: vec![],
            expansions: BTreeMap::new(),
        }
    }
}

impl BurnInService for BurnInServiceImpl {
    /// Generate the burn_in_tests task for the given build_variant.
    ///
    /// # Arguments
    ///
    /// * `build_variant` - Build variant to discover tasks for burn_in_tests.
    /// * `run_build_variant_name` - Name of build variant to generate burn_in_tests for.
    /// * `task_map` - Map of task definitions in evergreen project configuration.
    ///
    /// # Returns
    ///
    /// A generated suite to use for generating burn_in_tests.
    fn generate_burn_in_suite(
        &self,
        build_variant: &BuildVariant,
        run_build_variant_name: &str,
        task_map: Arc<HashMap<String, EvgTask>>,
    ) -> Result<Box<dyn GeneratedSuite>> {
        let mut sub_suites = vec![];
        let discovered_tasks = self.burn_in_discovery.discover_tasks(&build_variant.name)?;
        for discovered_task in discovered_tasks {
            let task_name = &discovered_task.task_name;
            if let Some(task_def) = task_map.get(task_name) {
                sub_suites.extend(self.build_tests_for_task(
                    &discovered_task,
                    task_def,
                    run_build_variant_name,
                )?);
            }
        }

        Ok(Box::new(GeneratedResmokeSuite {
            task_name: "burn_in_tests".to_string(),
            sub_suites,
        }))
    }

    /// Generate a burn_in_tags build variant for the given base build variant.
    ///
    /// # Arguments
    ///
    /// * `base_build_variant` - Build variant to generate burn_in_tags build variant based on.
    /// * `run_build_variant_name` - Build variant name to run burn_in_tests task on.
    /// * `generated_task` - Generated burn_in_tests task.
    /// * `compile_task_dependency` - Compile task name generated build variant should depend on.
    ///
    /// # Returns
    ///
    /// A generated burn_in_tags build variant based on another build variant.
    fn generate_burn_in_tags_build_variant(
        &self,
        base_build_variant: &BuildVariant,
        run_build_variant_name: String,
        generated_task: &dyn GeneratedSuite,
        compile_task_dependency: String,
    ) -> Result<BuildVariant> {
        let mut gen_config = BurnInTagsGeneratedConfig::new();

        gen_config.build_variant_name = run_build_variant_name;
        gen_config.build_variant_display_name = base_build_variant
            .display_name
            .as_ref()
            .map(|s| format!("{} {}", BURN_IN_DISPLAY_NAME_PREFIX, s));

        gen_config.expansions = base_build_variant.expansions.clone().unwrap_or_default();
        gen_config.expansions.insert(
            BURN_IN_BYPASS.to_string(),
            base_build_variant.name.to_string(),
        );

        let large_distro = self
            .config_extraction_service
            .determine_large_distro(generated_task, base_build_variant)?;

        gen_config
            .gen_task_specs
            .extend(generated_task.build_task_ref(large_distro));
        gen_config
            .display_tasks
            .push(generated_task.build_display_task());

        let compile_variant = self
            .evg_config_utils
            .lookup_build_variant_expansion(COMPILE_VARIANT, base_build_variant)
            .unwrap_or_else(|| base_build_variant.name.clone());

        let variant_task_dependencies = vec![
            TaskDependency {
                name: compile_task_dependency,
                variant: Some(compile_variant),
            },
            TaskDependency {
                name: VERSION_BURN_IN_GEN_TASK.to_string(),
                variant: Some(VERSION_GEN_VARIANT.to_string()),
            },
        ];

        Ok(BuildVariant {
            name: gen_config.build_variant_name.clone(),
            tasks: gen_config.gen_task_specs.clone(),
            display_name: gen_config.build_variant_display_name.clone(),
            run_on: base_build_variant.run_on.clone(),
            display_tasks: Some(gen_config.display_tasks.clone()),
            modules: base_build_variant.modules.clone(),
            expansions: Some(gen_config.expansions.clone()),
            depends_on: Some(variant_task_dependencies.to_vec()),
            activate: Some(false),
            ..Default::default()
        })
    }

    /// Generate a burn_in_tasks task for the given build variant.
    ///
    /// # Arguments
    ///
    /// * `build_variant` - Build variant to generate burn_in_tasks for.
    /// * `task_map` - Map of task definitions found in the evergreen project configuration.
    ///
    /// # Returns
    ///
    /// A generated task for burn_in_tasks on the given build variant.
    fn generate_burn_in_tasks_suite(
        &self,
        build_variant: &BuildVariant,
        task_map: Arc<HashMap<String, EvgTask>>,
    ) -> Result<Box<dyn GeneratedSuite>> {
        let mut sub_suites = vec![];

        let burn_in_task_name = self
            .evg_config_utils
            .lookup_build_variant_expansion(BURN_IN_TASK_NAME, build_variant)
            .unwrap_or_else(|| {
                panic!(
                    "`{}` build variant is missing the `{}` expansion to run `{}`. Set the expansion in your project's config to continue.",
                    build_variant.name, BURN_IN_TASK_NAME, BURN_IN_TASKS
                )
            });

        if let Some(task_def) = task_map.get(&burn_in_task_name) {
            sub_suites.extend(self.build_burn_in_tasks_for_task(task_def, build_variant)?);
        }

        Ok(Box::new(GeneratedResmokeSuite {
            task_name: "burn_in_tasks".to_string(),
            sub_suites,
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
    use maplit::{btreemap, hashmap};
    use rstest::rstest;
    use shrub_rs::models::{
        commands::{fn_call, fn_call_with_params},
        params::ParamValue,
        variant::BuildVariant,
    };

    use crate::{
        evergreen::evg_config_utils::{EvgConfigUtilsImpl, MultiversionGenerateTaskConfig},
        evergreen_names::{GENERATE_RESMOKE_TASKS, INITIALIZE_MULTIVERSION_TASKS},
        services::config_extraction::ConfigExtractionServiceImpl,
        task_types::{fuzzer_tasks::FuzzerGenTaskParams, multiversion::MultiversionService},
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
        let burn_in_label = "my_burn_in_label";
        let suite_info = BurnInSuiteInfo {
            task_name,
            build_variant,
            burn_in_label,
            ..Default::default()
        };

        let display_name = suite_info.build_display_name();

        assert!(display_name.contains(burn_in_label));
        assert!(display_name.contains(task_name));
        assert!(display_name.contains(build_variant));
    }

    fn build_mocked_config_extraction_service() -> ConfigExtractionServiceImpl {
        ConfigExtractionServiceImpl::new(
            Arc::new(EvgConfigUtilsImpl::new()),
            Arc::new(MockMultiversionService {}),
            "generating_task".to_string(),
            "config_location".to_string(),
            None,
        )
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
        ) -> GeneratedSubTask {
            GeneratedSubTask {
                evg_task: EvgTask {
                    ..Default::default()
                },
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
            _build_variant: Option<&BuildVariant>,
            _platform: Option<String>,
        ) -> Result<ResmokeGenParams> {
            Ok(ResmokeGenParams {
                require_multiversion_generate_tasks: self.is_multiversion,
                ..Default::default()
            })
        }

        fn determine_large_distro(
            &self,
            _generated_task: &dyn GeneratedSuite,
            _build_variant: &BuildVariant,
        ) -> Result<Option<String>> {
            Ok(None)
        }
    }

    struct MockMultiversionService {}
    impl MultiversionService for MockMultiversionService {
        fn exclude_tags_for_task(&self, _task_name: &str, _mv_mode: Option<String>) -> String {
            todo!()
        }
        fn filter_multiversion_generate_tasks(
            &self,
            multiversion_generate_tasks: Option<Vec<MultiversionGenerateTaskConfig>>,
            _last_versions_expansion: Option<String>,
        ) -> Option<Vec<MultiversionGenerateTaskConfig>> {
            return multiversion_generate_tasks;
        }
    }

    struct MockEvgConfigUtils {
        burn_in_task_name: Option<String>,
    }
    impl EvgConfigUtils for MockEvgConfigUtils {
        fn get_multiversion_generate_tasks(
            &self,
            _task: &EvgTask,
        ) -> Option<Vec<MultiversionGenerateTaskConfig>> {
            todo!()
        }

        fn is_task_generated(&self, _task: &EvgTask) -> bool {
            todo!()
        }

        fn is_task_fuzzer(&self, _task: &EvgTask) -> bool {
            todo!()
        }

        fn find_suite_name<'a>(&self, _task: &'a EvgTask) -> &'a str {
            todo!()
        }

        fn get_task_tags(&self, _task: &EvgTask) -> std::collections::HashSet<String> {
            todo!()
        }

        fn get_task_dependencies(&self, _task: &EvgTask) -> Vec<String> {
            todo!()
        }

        fn get_gen_task_var<'a>(&self, _task: &'a EvgTask, _var: &str) -> Option<&'a str> {
            todo!()
        }

        fn get_gen_task_vars(
            &self,
            _task: &EvgTask,
        ) -> Option<HashMap<String, shrub_rs::models::params::ParamValue>> {
            todo!()
        }

        fn translate_run_var(
            &self,
            _run_var: &str,
            _build_variant: &BuildVariant,
        ) -> Option<String> {
            todo!()
        }

        fn lookup_build_variant_expansion(
            &self,
            _name: &str,
            _build_variant: &BuildVariant,
        ) -> Option<String> {
            self.burn_in_task_name.clone()
        }

        fn lookup_and_split_by_whitespace_build_variant_expansion(
            &self,
            _name: &str,
            _build_variant: &BuildVariant,
        ) -> Vec<String> {
            todo!()
        }

        fn resolve_burn_in_tag_build_variants(
            &self,
            _build_variant: &BuildVariant,
            _build_variant_map: &HashMap<String, &BuildVariant>,
        ) -> Vec<String> {
            todo!()
        }

        fn lookup_required_param_str(&self, _task_def: &EvgTask, _run_var: &str) -> Result<String> {
            todo!()
        }

        fn lookup_required_param_u64(&self, _task_def: &EvgTask, _run_var: &str) -> Result<u64> {
            todo!()
        }

        fn lookup_required_param_bool(&self, _task_def: &EvgTask, _run_var: &str) -> Result<bool> {
            todo!()
        }

        fn lookup_default_param_bool(
            &self,
            _task_def: &EvgTask,
            _run_var: &str,
            _default: bool,
        ) -> Result<bool> {
            todo!()
        }

        fn lookup_default_param_str(
            &self,
            _task_def: &EvgTask,
            _run_var: &str,
            _default: &str,
        ) -> String {
            todo!()
        }

        fn lookup_optional_param_u64(
            &self,
            _task_def: &EvgTask,
            _run_var: &str,
        ) -> Result<Option<u64>> {
            todo!()
        }

        fn is_enterprise_build_variant(&self, _build_variant: &BuildVariant) -> bool {
            todo!()
        }

        fn infer_build_variant_platform(&self, _build_variant: &BuildVariant) -> String {
            todo!()
        }
    }

    fn build_mocked_service(burn_in_task_name: Option<String>) -> BurnInServiceImpl {
        BurnInServiceImpl::new(
            Arc::new(MockBurnInDiscovery {}),
            Arc::new(MockGenResmokeTasksService {}),
            Arc::new(MockConfigExtractionService {
                is_multiversion: false,
            }),
            Arc::new(MockEvgConfigUtils { burn_in_task_name }),
        )
    }

    fn build_mv_mocked_service(burn_in_task_name: Option<String>) -> BurnInServiceImpl {
        BurnInServiceImpl::new(
            Arc::new(MockBurnInDiscovery {}),
            Arc::new(MockGenResmokeTasksService {}),
            Arc::new(build_mocked_config_extraction_service()),
            Arc::new(MockEvgConfigUtils { burn_in_task_name }),
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
        let run_build_variant = "my_build_variant";
        let burn_in_service = build_mocked_service(None);

        let tasks = burn_in_service
            .build_tests_for_task(&discovered_task, &task_def, run_build_variant)
            .unwrap();

        assert_eq!(tasks.len(), discovered_task.test_list.len());
    }

    #[test]
    fn test_build_test_for_tasks_creates_task_for_each_multiversion_iteration_and_test() {
        let discovered_task = DiscoveredTask {
            task_name: "my task".to_string(),
            test_list: vec!["test_0.js".to_string(), "test_1.js".to_string()],
        };
        let vars = hashmap! {
                        "mv_suite1_last_continuous_new_old_new".to_string() => ParamValue::from("last-continuous"),
                        "mv_suite1_last_lts_new_old_new".to_string() => ParamValue::from("last-lts"),
                        "mv_suite1_last_continuous_old_new_old".to_string() => ParamValue::from("last-continuous"),
                        "mv_suite1_last_lts_old_new_old".to_string() => ParamValue::from("last-lts"),
        };
        let task_def = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call_with_params(INITIALIZE_MULTIVERSION_TASKS, vars),
                fn_call(GENERATE_RESMOKE_TASKS),
                fn_call("run tests"),
            ]),
            tags: Some(vec!["multiversion".to_string()]),
            ..Default::default()
        };
        let run_build_variant = "my_build_variant";
        let burn_in_service = build_mv_mocked_service(None);

        let tasks = burn_in_service
            .build_tests_for_task(&discovered_task, &task_def, run_build_variant)
            .unwrap();

        assert_eq!(tasks.len(), 8);
    }

    // build_burn_in_tasks_for_task tests.
    #[test]
    fn test_build_burn_in_tasks_for_task_creates_tasks() {
        let task_def = EvgTask {
            ..Default::default()
        };
        let build_variant = BuildVariant {
            ..Default::default()
        };
        let burn_in_service = build_mocked_service(None);

        let tasks = burn_in_service
            .build_burn_in_tasks_for_task(&task_def, &build_variant)
            .unwrap();

        assert_eq!(tasks.len(), BURN_IN_REPEAT_TASK_NUM);
    }

    #[test]
    fn test_build_burn_in_tasks_for_task_creates_tasks_for_each_multiversion_iteration() {
        let vars = hashmap! {
                        "mv_suite1_last_continuous_new_old_new".to_string() => ParamValue::from("last-continuous"),
                        "mv_suite1_last_lts_new_old_new".to_string() => ParamValue::from("last-lts"),
                        "mv_suite1_last_continuous_old_new_old".to_string() => ParamValue::from("last-continuous"),
                        "mv_suite1_last_lts_old_new_old".to_string() => ParamValue::from("last-lts"),
        };
        let task_def = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call_with_params(INITIALIZE_MULTIVERSION_TASKS, vars),
                fn_call(GENERATE_RESMOKE_TASKS),
                fn_call("run tests"),
            ]),
            tags: Some(vec!["multiversion".to_string()]),
            ..Default::default()
        };
        let build_variant = BuildVariant {
            ..Default::default()
        };
        let burn_in_service = build_mv_mocked_service(None);

        let tasks = burn_in_service
            .build_burn_in_tasks_for_task(&task_def, &build_variant)
            .unwrap();

        assert_eq!(tasks.len(), BURN_IN_REPEAT_TASK_NUM * 4);
    }

    // generate_burn_in_tags_build_variant tests.
    #[test]
    fn test_generate_burn_in_tags_build_variant() {
        let base_build_variant = BuildVariant {
            name: "base-build-variant-name".to_string(),
            display_name: Some("base build variant display name".to_string()),
            run_on: Some(vec!["base_distro_name".to_string()]),
            modules: Some(vec!["base_module_name".to_string()]),
            expansions: Some(btreemap! {
                "compile_variant".to_string() => "compile-build-variant-name".to_string(),
            }),
            ..Default::default()
        };
        let run_build_variant_name = "run-build-variant-name".to_string();

        let generated_task: &dyn GeneratedSuite = &GeneratedResmokeSuite {
            task_name: "display_task_name".to_string(),
            sub_suites: vec![GeneratedSubTask {
                evg_task: EvgTask {
                    name: "sub_suite_name".to_string(),
                    ..Default::default()
                },
                ..Default::default()
            }],
        };
        let burn_in_service = build_mocked_service(None);
        let compile_task_dependency = "mock_dependency".to_string();

        let burn_in_tags_build_variant = burn_in_service
            .generate_burn_in_tags_build_variant(
                &base_build_variant,
                run_build_variant_name,
                generated_task,
                compile_task_dependency,
            )
            .unwrap();

        let expansions = burn_in_tags_build_variant.expansions.unwrap_or_default();

        assert_eq!(burn_in_tags_build_variant.name, "run-build-variant-name");

        assert_eq!(
            burn_in_tags_build_variant.display_name,
            Some("[jstests_affected] base build variant display name".to_string())
        );
        assert_eq!(
            burn_in_tags_build_variant.run_on,
            Some(vec!["base_distro_name".to_string()])
        );
        assert_eq!(
            burn_in_tags_build_variant.modules,
            Some(vec!["base_module_name".to_string()])
        );
        assert_eq!(
            expansions.get(BURN_IN_BYPASS),
            Some(&"base-build-variant-name".to_string())
        );
        assert_eq!(
            expansions.get(COMPILE_VARIANT),
            Some(&"compile-build-variant-name".to_string())
        );

        assert_eq!(
            burn_in_tags_build_variant.display_tasks.unwrap_or_default()[0].name,
            "display_task_name"
        );
        assert_eq!(burn_in_tags_build_variant.tasks[0].name, "sub_suite_name");
    }

    // generate_burn_in_tasks_suite tests.
    #[rstest]
    #[case(Some("task_1".to_string()), BURN_IN_REPEAT_TASK_NUM)]
    #[should_panic(
        expected = "`bv_name` build variant is missing the `burn_in_task_name` expansion to run `burn_in_tasks_gen`. Set the expansion in your project's config to continue."
    )]
    #[case::panic_with_message(None, 0)]
    fn test_generate_burn_in_tasks_suite(
        #[case] burn_in_task_name: Option<String>,
        #[case] expected_num_tasks: usize,
    ) {
        let build_variant = BuildVariant {
            name: "bv_name".to_string(),
            ..Default::default()
        };
        let task_map = Arc::new(hashmap! {
            "task_1".to_string() => EvgTask {
                ..Default::default()
            },
            "task_2".to_string() => EvgTask {
                ..Default::default()
            },
        });
        let burn_in_service = build_mocked_service(burn_in_task_name);

        let suite = burn_in_service
            .generate_burn_in_tasks_suite(&build_variant, task_map)
            .unwrap();

        assert_eq!(suite.sub_tasks().len(), expected_num_tasks);
    }
}
