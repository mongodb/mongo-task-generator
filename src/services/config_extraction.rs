use std::sync::Arc;

use anyhow::Result;
use shrub_rs::models::{task::EvgTask, variant::BuildVariant};

use crate::{
    evergreen::evg_config_utils::EvgConfigUtils,
    evergreen_names::{
        CONTINUE_ON_FAILURE, FUZZER_PARAMETERS, IDLE_TIMEOUT, MULTIVERSION,
        NO_MULTIVERSION_ITERATION, NPM_COMMAND, NUM_FUZZER_FILES, NUM_FUZZER_TASKS, REPEAT_SUITES,
        RESMOKE_ARGS, RESMOKE_JOBS_MAX, SHOULD_SHUFFLE_TESTS, USE_LARGE_DISTRO,
    },
    task_types::{fuzzer_tasks::FuzzerGenTaskParams, resmoke_tasks::ResmokeGenParams},
    utils::task_name::remove_gen_suffix,
};

/// Interface for performing extractions of evergreen project configuration.
pub trait ConfigExtractionService: Sync + Send {
    /// Build the configuration for generated a fuzzer based on the evergreen task definition.
    ///
    /// # Arguments
    ///
    /// * `task-def` - Task definition of fuzzer to generate.
    /// * `build_variant` - Name of build variant being generated.
    ///
    /// # Returns
    ///
    /// Parameters to configure how fuzzer task should be generated.
    fn task_def_to_fuzzer_params(
        &self,
        task_def: &EvgTask,
        build_variant: &BuildVariant,
    ) -> Result<FuzzerGenTaskParams>;

    /// Build the configuration for generated a resmoke based on the evergreen task definition.
    ///
    /// # Arguments
    ///
    /// * `task-def` - Task definition of task to generate.
    ///
    /// # Returns
    ///
    /// Parameters to configure how resmoke task should be generated.
    fn task_def_to_resmoke_params(
        &self,
        task_def: &EvgTask,
        is_enterprise: bool,
    ) -> Result<ResmokeGenParams>;
}

/// Implementation for performing extractions of evergreen project configuration.
pub struct ConfigExtractionServiceImpl {
    evg_config_utils: Arc<dyn EvgConfigUtils>,
    generating_task: String,
    config_location: String,
}

impl ConfigExtractionServiceImpl {
    /// Create a new instance of the config extraction service.
    ///
    /// # Arguments
    ///
    /// * `evg_config_utils` - Utilities for looking up evergreen project configuration.
    /// * `generating_task` - Name of task running task generation.
    /// * `config_location` - Location where generated configuration will be stored.
    ///
    pub fn new(
        evg_config_utils: Arc<dyn EvgConfigUtils>,
        generating_task: String,
        config_location: String,
    ) -> Self {
        Self {
            evg_config_utils,
            generating_task,
            config_location,
        }
    }

    /// Determine the dependencies to add to tasks generated from the given task definition.
    ///
    /// A generated tasks should depend on all tasks listed in its "_gen" tasks depends_on
    /// section except for the task generated the configuration.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Definition of task being generated from.
    ///
    /// # Returns
    ///
    /// List of tasks that should be included as dependencies.
    fn determine_task_dependencies(&self, task_def: &EvgTask) -> Vec<String> {
        let depends_on = self.evg_config_utils.get_task_dependencies(task_def);

        depends_on
            .into_iter()
            .filter(|t| t != &self.generating_task)
            .collect()
    }
}

impl ConfigExtractionService for ConfigExtractionServiceImpl {
    /// Build the configuration for generated a fuzzer based on the evergreen task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition of fuzzer to generate.
    /// * `build_variant` - Build variant task is being generated based off.
    ///
    /// # Returns
    ///
    /// Parameters to configure how fuzzer task should be generated.
    fn task_def_to_fuzzer_params(
        &self,
        task_def: &EvgTask,
        build_variant: &BuildVariant,
    ) -> Result<FuzzerGenTaskParams> {
        let evg_config_utils = self.evg_config_utils.clone();
        let is_enterprise = evg_config_utils.is_enterprise_build_variant(build_variant);
        let task_name = remove_gen_suffix(&task_def.name).to_string();
        let num_files = evg_config_utils
            .translate_run_var(
                evg_config_utils
                    .get_gen_task_var(task_def, NUM_FUZZER_FILES)
                    .unwrap_or_else(|| {
                        panic!(
                            "`{}` missing for task: '{}'",
                            NUM_FUZZER_FILES, task_def.name
                        )
                    }),
                build_variant,
            )
            .unwrap();

        let suite = evg_config_utils.find_suite_name(task_def).to_string();
        Ok(FuzzerGenTaskParams {
            task_name,
            variant: build_variant.name.to_string(),
            suite,
            num_files,
            num_tasks: evg_config_utils.lookup_required_param_u64(task_def, NUM_FUZZER_TASKS)?,
            resmoke_args: evg_config_utils.lookup_required_param_str(task_def, RESMOKE_ARGS)?,
            npm_command: evg_config_utils.lookup_default_param_str(
                task_def,
                NPM_COMMAND,
                "jstestfuzz",
            ),
            jstestfuzz_vars: evg_config_utils
                .get_gen_task_var(task_def, FUZZER_PARAMETERS)
                .map(|j| j.to_string()),
            continue_on_failure: evg_config_utils
                .lookup_required_param_bool(task_def, CONTINUE_ON_FAILURE)?,
            resmoke_jobs_max: evg_config_utils
                .lookup_required_param_u64(task_def, RESMOKE_JOBS_MAX)?,
            should_shuffle: evg_config_utils
                .lookup_required_param_bool(task_def, SHOULD_SHUFFLE_TESTS)?,
            timeout_secs: evg_config_utils.lookup_required_param_u64(task_def, IDLE_TIMEOUT)?,
            require_multiversion_setup: evg_config_utils
                .get_task_tags(task_def)
                .contains(MULTIVERSION),
            config_location: self.config_location.clone(),
            dependencies: self.determine_task_dependencies(task_def),
            is_enterprise,
        })
    }

    /// Build the configuration for generated a resmoke based on the evergreen task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition of task to generate.
    /// * `is_enterprise` - Is this being generated for an enterprise build variant.
    ///
    /// # Returns
    ///
    /// Parameters to configure how resmoke task should be generated.
    fn task_def_to_resmoke_params(
        &self,
        task_def: &EvgTask,
        is_enterprise: bool,
    ) -> Result<ResmokeGenParams> {
        let task_name = remove_gen_suffix(&task_def.name).to_string();
        let suite = self.evg_config_utils.find_suite_name(task_def).to_string();
        let task_tags = self.evg_config_utils.get_task_tags(task_def);
        let require_multiversion_setup = task_tags.contains(MULTIVERSION);
        let no_multiversion_iteration = task_tags.contains(NO_MULTIVERSION_ITERATION);

        Ok(ResmokeGenParams {
            task_name,
            suite_name: suite,
            use_large_distro: self.evg_config_utils.lookup_default_param_bool(
                task_def,
                USE_LARGE_DISTRO,
                false,
            )?,
            require_multiversion_setup,
            generate_multiversion_combos: require_multiversion_setup && !no_multiversion_iteration,
            repeat_suites: self
                .evg_config_utils
                .lookup_optional_param_u64(task_def, REPEAT_SUITES)?,
            resmoke_args: self.evg_config_utils.lookup_default_param_str(
                task_def,
                RESMOKE_ARGS,
                "",
            ),
            resmoke_jobs_max: self
                .evg_config_utils
                .lookup_optional_param_u64(task_def, RESMOKE_JOBS_MAX)?,
            config_location: self.config_location.clone(),
            dependencies: self.determine_task_dependencies(task_def),
            is_enterprise,
            pass_through_vars: self.evg_config_utils.get_gen_task_vars(task_def),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evergreen::evg_config_utils::EvgConfigUtilsImpl;
    use rstest::rstest;
    use shrub_rs::models::task::TaskDependency;

    fn build_mocked_config_extraction_service() -> ConfigExtractionServiceImpl {
        ConfigExtractionServiceImpl::new(
            Arc::new(EvgConfigUtilsImpl::new()),
            "generating_task".to_string(),
            "config_location".to_string(),
        )
    }

    // Tests for determine_task_dependencies.
    #[rstest]
    #[case(
        vec![], vec![]
    )]
    #[case(vec!["dependency_0", "dependency_1"], vec!["dependency_0", "dependency_1"])]
    #[case(vec!["dependency_0", "generating_task"], vec!["dependency_0"])]
    fn test_determine_task_dependencies(
        #[case] depends_on: Vec<&str>,
        #[case] expected_deps: Vec<&str>,
    ) {
        let config_extraction_service = build_mocked_config_extraction_service();
        let evg_task = EvgTask {
            depends_on: Some(
                depends_on
                    .into_iter()
                    .map(|d| TaskDependency {
                        name: d.to_string(),
                        variant: None,
                    })
                    .collect(),
            ),
            ..Default::default()
        };

        let deps = config_extraction_service.determine_task_dependencies(&evg_task);

        assert_eq!(
            deps,
            expected_deps
                .into_iter()
                .map(|d| d.to_string())
                .collect::<Vec<String>>()
        );
    }
}
