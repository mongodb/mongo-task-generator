use std::sync::Arc;

use anyhow::{bail, Result};
use shrub_rs::models::{task::EvgTask, variant::BuildVariant};

use crate::{
    evergreen::evg_config_utils::EvgConfigUtils,
    evergreen_names::{
        CONTINUE_ON_FAILURE, FUZZER_PARAMETERS, IDLE_TIMEOUT, LARGE_DISTRO_EXPANSION, MULTIVERSION,
        NO_MULTIVERSION_GENERATE_TASKS, NPM_COMMAND, NUM_FUZZER_FILES, NUM_FUZZER_TASKS, REPEAT_SUITES,
        RESMOKE_ARGS, RESMOKE_JOBS_MAX, SHOULD_SHUFFLE_TESTS, USE_LARGE_DISTRO,
    },
    generate_sub_tasks_config::GenerateSubTasksConfig,
    task_types::{
        fuzzer_tasks::FuzzerGenTaskParams, generated_suite::GeneratedSuite,
        resmoke_tasks::ResmokeGenParams,
    },
    utils::task_name::remove_gen_suffix,
};

/// Interface for performing extractions of evergreen project configuration.
pub trait ConfigExtractionService: Sync + Send {
    /// Build the configuration for generated a fuzzer based on the evergreen task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition of fuzzer to generate.
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
    /// * `task_def` - Task definition of task to generate.
    /// * `is_enterprise` - Is this being generated for an enterprise build variant.
    /// * `platform` - Platform that task will run on.
    ///
    /// # Returns
    ///
    /// Parameters to configure how resmoke task should be generated.
    fn task_def_to_resmoke_params(
        &self,
        task_def: &EvgTask,
        is_enterprise: bool,
        platform: Option<String>,
    ) -> Result<ResmokeGenParams>;

    /// Determine large distro name if the given sub-tasks should run on it.
    ///
    /// By default, we won't specify a distro and they will just use the default for the build
    /// variant. If they specify `use_large_distro` then we should instead use the large distro
    /// configured for the build variant. If that is not defined, then throw an error unless
    /// the build variant is configured to be ignored.
    ///
    /// # Arguments
    ///
    /// * `generated_task` - Generated task.
    /// * `build_variant` - Build Variant to run generated task on.
    ///
    /// # Returns
    ///
    /// Large distro name if needed.
    fn determine_large_distro(
        &self,
        generated_task: &dyn GeneratedSuite,
        build_variant: &BuildVariant,
    ) -> Result<Option<String>>;
}

/// Implementation for performing extractions of evergreen project configuration.
pub struct ConfigExtractionServiceImpl {
    evg_config_utils: Arc<dyn EvgConfigUtils>,
    generating_task: String,
    config_location: String,
    gen_sub_tasks_config: Option<GenerateSubTasksConfig>,
}

impl ConfigExtractionServiceImpl {
    /// Create a new instance of the config extraction service.
    ///
    /// # Arguments
    ///
    /// * `evg_config_utils` - Utilities for looking up evergreen project configuration.
    /// * `generating_task` - Name of task running task generation.
    /// * `config_location` - Location where generated configuration will be stored.
    /// * `gen_sub_tasks_config` - Configuration for generating sub-tasks.
    ///
    pub fn new(
        evg_config_utils: Arc<dyn EvgConfigUtils>,
        generating_task: String,
        config_location: String,
        gen_sub_tasks_config: Option<GenerateSubTasksConfig>,
    ) -> Self {
        Self {
            evg_config_utils,
            generating_task,
            config_location,
            gen_sub_tasks_config,
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
            multiversion_generate_tasks: evg_config_utils.get_multiversion_generate_tasks(task_def),
            config_location: self.config_location.clone(),
            dependencies: self.determine_task_dependencies(task_def),
            is_enterprise,
            platform: Some(evg_config_utils.infer_build_variant_platform(build_variant)),
        })
    }

    /// Build the configuration for generated a resmoke based on the evergreen task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition of task to generate.
    /// * `is_enterprise` - Is this being generated for an enterprise build variant.
    /// * `platform` - Platform that task will run on.
    ///
    /// # Returns
    ///
    /// Parameters to configure how resmoke task should be generated.
    fn task_def_to_resmoke_params(
        &self,
        task_def: &EvgTask,
        is_enterprise: bool,
        platform: Option<String>,
    ) -> Result<ResmokeGenParams> {
        let task_name = remove_gen_suffix(&task_def.name).to_string();
        let suite = self.evg_config_utils.find_suite_name(task_def).to_string();
        let task_tags = self.evg_config_utils.get_task_tags(task_def);
        let require_multiversion_setup = task_tags.contains(MULTIVERSION);
        let no_multiversion_generate_tasks = task_tags.contains(NO_MULTIVERSION_GENERATE_TASKS);

        Ok(ResmokeGenParams {
            task_name,
            suite_name: suite,
            use_large_distro: self.evg_config_utils.lookup_default_param_bool(
                task_def,
                USE_LARGE_DISTRO,
                false,
            )?,
            require_multiversion_setup,
            require_multiversion_generate_tasks: require_multiversion_setup && !no_multiversion_generate_tasks,
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
            multiversion_generate_tasks: self.evg_config_utils.get_multiversion_generate_tasks(task_def),
            config_location: self.config_location.clone(),
            dependencies: self.determine_task_dependencies(task_def),
            is_enterprise,
            pass_through_vars: self.evg_config_utils.get_gen_task_vars(task_def),
            platform,
        })
    }

    /// Determine large distro name if the given sub-tasks should run on it.
    ///
    /// By default, we won't specify a distro and they will just use the default for the build
    /// variant. If they specify `use_large_distro` then we should instead use the large distro
    /// configured for the build variant. If that is not defined, then throw an error unless
    /// the build variant is configured to be ignored.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Generated task.
    /// * `build_variant` - Build Variant to run generated task on.
    ///
    /// # Returns
    ///
    /// Large distro name if needed.
    fn determine_large_distro(
        &self,
        generated_task: &dyn GeneratedSuite,
        build_variant: &BuildVariant,
    ) -> Result<Option<String>> {
        let large_distro_name = self
            .evg_config_utils
            .lookup_build_variant_expansion(LARGE_DISTRO_EXPANSION, build_variant);
        let build_variant_name = build_variant.name.as_str();

        if generated_task.use_large_distro() {
            if large_distro_name.is_some() {
                return Ok(large_distro_name);
            }

            if let Some(gen_task_config) = &self.gen_sub_tasks_config {
                if gen_task_config.ignore_missing_large_distro(build_variant_name) {
                    return Ok(None);
                }
            }

            bail!(
                r#"
***************************************************************************************
It appears we are trying to generate a task marked as requiring a large distro, but the
build variant has not specified a large build variant. In order to resolve this error,
you need to:

(1) add a 'large_distro_name' expansion to this build variant ('{build_variant_name}').

-- or --

(2) add this build variant ('{build_variant_name}') to the 'build_variant_large_distro_exception'
list in the 'etc/generate_subtasks_config.yml' file.
***************************************************************************************
"#
            );
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        evergreen::evg_config_utils::EvgConfigUtilsImpl,
        task_types::{generated_suite::GeneratedSubTask, resmoke_tasks::GeneratedResmokeSuite},
    };
    use maplit::{btreemap, hashset};
    use rstest::rstest;
    use shrub_rs::models::task::TaskDependency;

    fn build_mocked_config_extraction_service() -> ConfigExtractionServiceImpl {
        ConfigExtractionServiceImpl::new(
            Arc::new(EvgConfigUtilsImpl::new()),
            "generating_task".to_string(),
            "config_location".to_string(),
            None,
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

    // Tests for determine_large_distro.
    #[rstest]
    #[case(vec![false, false], None, None)]
    #[case(vec![false, false], Some("large_distro".to_string()), None)]
    #[case(vec![true, false], Some("large_distro".to_string()), Some("large_distro".to_string()))]
    #[case(vec![false, true], Some("large_distro".to_string()), Some("large_distro".to_string()))]
    #[case(vec![true, true], Some("large_distro".to_string()), Some("large_distro".to_string()))]
    fn test_determine_large_distro_should_return_large_distro_name(
        #[case] use_large_distro: Vec<bool>,
        #[case] large_distro_name: Option<String>,
        #[case] expected_distro: Option<String>,
    ) {
        let config_extraction_service = build_mocked_config_extraction_service();
        let generated_task: &dyn GeneratedSuite = &GeneratedResmokeSuite {
            task_name: "display_task_name".to_string(),
            sub_suites: use_large_distro
                .iter()
                .enumerate()
                .map(|(i, value)| GeneratedSubTask {
                    evg_task: EvgTask {
                        name: format!("sub_suite_name_{}", i),
                        ..Default::default()
                    },
                    use_large_distro: *value,
                })
                .collect(),
        };
        let mut build_variant = BuildVariant {
            ..Default::default()
        };
        if let Some(distro_name) = large_distro_name {
            build_variant.expansions = Some(btreemap! {
                "large_distro_name".to_string() => distro_name,
            });
        };

        let large_distro = config_extraction_service
            .determine_large_distro(generated_task, &build_variant)
            .unwrap();

        assert_eq!(large_distro, expected_distro);
    }

    #[test]
    fn test_determine_large_distro_should_fail_if_no_large_distro() {
        let config_extraction_service = build_mocked_config_extraction_service();
        let generated_task: &dyn GeneratedSuite = &GeneratedResmokeSuite {
            task_name: "display_task_name".to_string(),
            sub_suites: vec![GeneratedSubTask {
                evg_task: EvgTask {
                    name: "sub_suite_name".to_string(),
                    ..Default::default()
                },
                use_large_distro: true,
            }],
        };
        let build_variant = BuildVariant {
            ..Default::default()
        };

        let large_distro =
            config_extraction_service.determine_large_distro(generated_task, &build_variant);

        assert!(large_distro.is_err());
    }

    #[test]
    fn test_determine_large_distro_respects_ignore_missing_large_distro() {
        let mut config_extraction_service = build_mocked_config_extraction_service();
        config_extraction_service.gen_sub_tasks_config = Some(GenerateSubTasksConfig {
            build_variant_large_distro_exceptions: hashset! {
                "build_variant_0".to_string(),
                "my_build_variant".to_string(),
                "build_variant_1".to_string(),
            },
        });
        let generated_task: &dyn GeneratedSuite = &GeneratedResmokeSuite {
            task_name: "display_task_name".to_string(),
            sub_suites: vec![GeneratedSubTask {
                evg_task: EvgTask {
                    name: "sub_suite_name".to_string(),
                    ..Default::default()
                },
                use_large_distro: true,
            }],
        };
        let build_variant = BuildVariant {
            name: "my_build_variant".to_string(),
            ..Default::default()
        };

        let large_distro =
            config_extraction_service.determine_large_distro(generated_task, &build_variant);

        assert!(large_distro.is_ok());
    }
}
