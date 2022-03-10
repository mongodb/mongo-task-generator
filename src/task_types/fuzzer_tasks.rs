use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use maplit::hashmap;
use shrub_rs::models::{
    commands::{fn_call, fn_call_with_params},
    params::ParamValue,
    task::{EvgTask, TaskDependency},
};
use tracing::{event, Level};

use crate::{
    evergreen_names::{
        ADD_GIT_TAG, ARTIFACT_CREATION_TASK, CONFIGURE_EVG_API_CREDS, CONTINUE_ON_FAILURE,
        DO_MULTIVERSION_SETUP, DO_SETUP, FUZZER_PARAMETERS, GEN_TASK_CONFIG_LOCATION,
        GET_PROJECT_WITH_NO_MODULES, IDLE_TIMEOUT, MULTIVERSION_EXCLUDE_TAGS, NPM_COMMAND,
        REQUIRE_MULTIVERSION_SETUP, RESMOKE_ARGS, RESMOKE_JOBS_MAX, RUN_FUZZER,
        RUN_GENERATED_TESTS, SETUP_JSTESTFUZZ, SHOULD_SHUFFLE_TESTS, SUITE_NAME, TASK_NAME,
    },
    utils::task_name::name_generated_task,
};

use super::{generated_suite::GeneratedSuite, multiversion::MultiversionService};

/// Parameters for how a fuzzer task should be generated.
#[derive(Default, Debug, Clone)]
pub struct FuzzerGenTaskParams {
    /// Name of task being generated.
    pub task_name: String,
    /// Name of build variant being generated on.
    pub variant: String,
    /// Resmoke suite for generated tests.
    pub suite: String,
    /// Number of javascript files fuzzer should generate.
    pub num_files: String,
    /// Number of sub-tasks fuzzer should generate.
    pub num_tasks: u64,
    /// Arguments to pass to resmoke invocation.
    pub resmoke_args: String,
    /// NPM command to perform fuzzer execution.
    pub npm_command: String,
    /// Arguments to pass to fuzzer invocation.
    pub jstestfuzz_vars: Option<String>,
    /// Should generated tests continue running after hitting error.
    pub continue_on_failure: bool,
    /// Maximum number of jobs resmoke should execute in parallel.
    pub resmoke_jobs_max: u64,
    /// Should tests be executed out of order.
    pub should_shuffle: bool,
    /// Timeout before test execution is considered hung.
    pub timeout_secs: u64,
    /// Requires downloading multiversion binaries.
    pub require_multiversion_setup: Option<bool>,
    /// Location of generated task configuration.
    pub config_location: String,
}

impl FuzzerGenTaskParams {
    /// Create parameters to send to fuzzer to generate appropriate fuzzer tests.
    fn build_fuzzer_parameters(&self) -> HashMap<String, ParamValue> {
        hashmap! {
            NPM_COMMAND.to_string() => ParamValue::from(self.npm_command.as_str()),
            FUZZER_PARAMETERS.to_string() => ParamValue::String(format!("--numGeneratedFiles {} {}", self.num_files, self.jstestfuzz_vars.clone().unwrap_or_default())),
        }
    }

    /// Determine if these parameters are for a multiversion fuzzer.
    fn is_multiversion(&self) -> bool {
        self.require_multiversion_setup.unwrap_or(false)
    }

    /// Build the vars to send to tasks in the 'run tests' function.
    ///
    /// # Arguments
    ///
    /// * `generated_suite_name` - A generated suite to execute against.
    /// * `version_combination` - Versions to start replica set with.
    ///
    /// # Returns
    ///
    /// Map of arguments to pass to 'run tests' function.
    fn build_run_tests_vars(
        &self,
        generated_suite_name: Option<&str>,
        version_combination: Option<&str>,
    ) -> HashMap<String, ParamValue> {
        let mut vars = hashmap! {
            CONTINUE_ON_FAILURE.to_string() => ParamValue::from(self.continue_on_failure),
            GEN_TASK_CONFIG_LOCATION.to_string() => ParamValue::from(self.config_location.as_str()),
            REQUIRE_MULTIVERSION_SETUP.to_string() => ParamValue::from(self.is_multiversion()),
            RESMOKE_ARGS.to_string() => ParamValue::from(self.resmoke_args.as_str()),
            RESMOKE_JOBS_MAX.to_string() => ParamValue::from(self.resmoke_jobs_max),
            SHOULD_SHUFFLE_TESTS.to_string() => ParamValue::from(self.should_shuffle),
            TASK_NAME.to_string() => ParamValue::from(self.task_name.as_str()),
            IDLE_TIMEOUT.to_string() => ParamValue::from(self.timeout_secs),
        };

        if let Some(suite) = generated_suite_name {
            vars.insert(SUITE_NAME.to_string(), ParamValue::from(suite));
        } else {
            vars.insert(
                SUITE_NAME.to_string(),
                ParamValue::from(self.suite.as_str()),
            );
        }

        if let Some(version_combination) = version_combination {
            vars.insert(
                MULTIVERSION_EXCLUDE_TAGS.to_string(),
                ParamValue::from(version_combination),
            );
        }

        vars
    }
}

/// A Generated Fuzzer task.
#[derive(Debug)]
pub struct FuzzerTask {
    /// Name for generated task.
    pub task_name: String,
    /// Sub-tasks comprising generated task.
    pub sub_tasks: Vec<EvgTask>,
}

impl GeneratedSuite for FuzzerTask {
    /// Get the display name to use for the generated task.
    fn display_name(&self) -> String {
        self.task_name.to_string()
    }

    /// Get the list of sub-tasks that comprise the generated task.
    fn sub_tasks(&self) -> Vec<EvgTask> {
        self.sub_tasks.clone()
    }
}

/// A service for generating fuzzer tasks.
pub trait GenFuzzerService: Sync + Send {
    /// Generate a fuzzer task.
    fn generate_fuzzer_task(&self, params: &FuzzerGenTaskParams)
        -> Result<Box<dyn GeneratedSuite>>;
}

/// Implementation of the GenFuzzerService.
pub struct GenFuzzerServiceImpl {
    /// Service to help generate multiversion test suites.
    multiversion_service: Arc<dyn MultiversionService>,
}

impl GenFuzzerServiceImpl {
    /// Create a new instance of the GenFuzzerService.
    ///
    /// # Arguments
    ///
    /// * `multiversion_service` - Service to help generate multiversion test suites.
    pub fn new(multiversion_service: Arc<dyn MultiversionService>) -> Self {
        Self {
            multiversion_service: multiversion_service.clone(),
        }
    }
}

impl GenFuzzerService for GenFuzzerServiceImpl {
    /// Generate a fuzzer task based on the given parameters.
    ///
    /// # Arguments
    ///
    /// * `params` - Parameters describing how to generate fuzzer.
    ///
    /// # Returns
    ///
    /// GeneratedSuite with details of how shrub task for the suite is built.
    fn generate_fuzzer_task(
        &self,
        params: &FuzzerGenTaskParams,
    ) -> Result<Box<dyn GeneratedSuite>> {
        let task_name = &params.task_name;
        let mut sub_tasks: Vec<EvgTask> = vec![];
        if params.is_multiversion() {
            let version_combinations = self
                .multiversion_service
                .get_version_combinations(&params.suite)?;
            event!(
                Level::INFO,
                task_name = task_name.as_str(),
                "Generating multiversion fuzzer"
            );
            for (old_version, version_combination) in self
                .multiversion_service
                .multiversion_iter(&version_combinations)
            {
                let base_task_name =
                    build_name(&params.task_name, &old_version, &version_combination);
                let base_suite_name = build_name(&params.suite, &old_version, &version_combination);

                sub_tasks.extend(
                    (0..params.num_tasks)
                        .map(|i| {
                            build_fuzzer_sub_task(
                                &base_task_name,
                                i,
                                params,
                                Some(&base_suite_name),
                                Some(&version_combination),
                            )
                        })
                        .collect::<Vec<EvgTask>>(),
                );
            }
        } else {
            sub_tasks = (0..params.num_tasks)
                .map(|i| build_fuzzer_sub_task(&params.task_name, i, params, None, None))
                .collect();
        }

        Ok(Box::new(FuzzerTask {
            task_name: params.task_name.to_string(),
            sub_tasks,
        }))
    }
}

/// Build a sub-task for a fuzzer.
///
/// # Arguments
///
/// * `display_name` - Display name of task being built.
/// * `sub_task_index` - Index of sub-task to build.
/// * `params` - Parameters for how task should be generated.
/// * `generated_suite_name` - Name of suite to execute against.
/// * `version_combination` - Versions to start replica set with.
///
/// # Returns
///
/// A shrub task to generate the sub-task.
fn build_fuzzer_sub_task(
    display_name: &str,
    sub_task_index: u64,
    params: &FuzzerGenTaskParams,
    generated_suite_name: Option<&str>,
    version_combination: Option<&str>,
) -> EvgTask {
    let sub_task_name =
        name_generated_task(display_name, Some(sub_task_index), Some(params.num_tasks));

    let mut commands = vec![];
    if params.is_multiversion() {
        commands.extend(vec![
            fn_call(GET_PROJECT_WITH_NO_MODULES),
            fn_call(ADD_GIT_TAG),
        ]);
    }
    commands.extend(vec![fn_call(DO_SETUP), fn_call(CONFIGURE_EVG_API_CREDS)]);

    if params.is_multiversion() {
        commands.push(fn_call(DO_MULTIVERSION_SETUP));
    }

    commands.extend(vec![
        fn_call(SETUP_JSTESTFUZZ),
        fn_call_with_params(RUN_FUZZER, params.build_fuzzer_parameters()),
        fn_call_with_params(
            RUN_GENERATED_TESTS,
            params.build_run_tests_vars(generated_suite_name, version_combination),
        ),
    ]);

    let dependency = TaskDependency {
        name: ARTIFACT_CREATION_TASK.to_string(),
        variant: None,
    };

    EvgTask {
        name: sub_task_name,
        commands,
        depends_on: Some(vec![dependency]),
        ..Default::default()
    }
}

/// Build the name to use for a sub-task.
///
/// # Arguments
///
/// * `base_name` - Name of task.
/// * `old_version` - Previous version to test against (i.e. lts or continuous).
/// * `version_combination` - Versions to start replica set with.
///
/// # Returns
///
/// Name to use for generated sub-task.
fn build_name(base_name: &str, old_version: &str, version_combination: &str) -> String {
    [base_name, old_version, version_combination]
        .iter()
        .filter_map(|p| {
            if !p.is_empty() {
                Some(p.to_string())
            } else {
                None
            }
        })
        .collect::<Vec<String>>()
        .join("_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;
    use shrub_rs::models::commands::EvgCommand;

    // FuzzerGenTasParams tests
    #[rstest]
    #[case("my_command", None, "5")]
    #[case("my_command", Some("node params"), "20")]
    fn test_build_fuzzer_params(
        #[case] npm_command: &str,
        #[case] jstestfuzz_vars: Option<&str>,
        #[case] num_files: &str,
    ) {
        let gen_params = FuzzerGenTaskParams {
            npm_command: npm_command.to_string(),
            jstestfuzz_vars: jstestfuzz_vars.map(|j| j.to_string()),
            num_files: num_files.to_string(),
            ..Default::default()
        };

        let parameters = gen_params.build_fuzzer_parameters();

        assert_eq!(
            parameters.get("npm_command"),
            Some(&ParamValue::String(npm_command.to_string()))
        );
        let expected_vars = format!(
            "--numGeneratedFiles {} {}",
            num_files,
            jstestfuzz_vars.unwrap_or_default()
        );
        assert_eq!(
            parameters.get("jstestfuzz_vars"),
            Some(&ParamValue::String(expected_vars))
        );
    }

    #[rstest]
    #[case(Some(true), true)]
    #[case(Some(false), false)]
    #[case(None, false)]
    fn test_is_multiversion(
        #[case] require_multiversion_setup: Option<bool>,
        #[case] actual: bool,
    ) {
        let gen_params = FuzzerGenTaskParams {
            require_multiversion_setup,
            ..Default::default()
        };

        assert_eq!(gen_params.is_multiversion(), actual);
    }

    #[rstest]
    #[case("my suite", None, None, "my suite")]
    #[case("my suite", Some("gen suite name"), None, "gen suite name")]
    #[case("my suite", None, Some("bin version"), "my suite")]
    #[case(
        "my suite",
        Some("gen suite name"),
        Some("bin version"),
        "gen suite name"
    )]
    fn test_build_run_tests_vars(
        #[case] suite_name: &str,
        #[case] gen_suite_name: Option<&str>,
        #[case] bin_version: Option<&str>,
        #[case] expected_suite: &str,
    ) {
        let gen_params = FuzzerGenTaskParams {
            task_name: "my task".to_string(),
            suite: suite_name.to_string(),
            ..Default::default()
        };

        let run_tests_vars = gen_params.build_run_tests_vars(gen_suite_name, bin_version);

        assert_eq!(
            run_tests_vars.get("task"),
            Some(&ParamValue::String("my task".to_string()))
        );
        assert_eq!(
            run_tests_vars.get("suite"),
            Some(&ParamValue::String(expected_suite.to_string()))
        );
        assert_eq!(
            run_tests_vars.contains_key("multiversion_exclude_tags_version"),
            bin_version.is_some()
        );
    }

    // FuzzerTask tests
    #[test]
    fn test_display_name() {
        let fuzzer_task = FuzzerTask {
            task_name: "my fuzzer".to_string(),
            sub_tasks: vec![],
        };

        assert_eq!(fuzzer_task.display_name(), "my fuzzer".to_string());
    }

    #[test]
    fn test_sub_tasks() {
        let fuzzer_task = FuzzerTask {
            task_name: "my fuzzer".to_string(),
            sub_tasks: vec![
                EvgTask {
                    ..Default::default()
                },
                EvgTask {
                    ..Default::default()
                },
            ],
        };

        assert_eq!(fuzzer_task.sub_tasks().len(), 2);
    }

    // build_name
    #[rstest]
    #[case(
        "agg_fuzzer",
        "last_lts",
        "new_old_new",
        "agg_fuzzer_last_lts_new_old_new"
    )]
    #[case("agg_fuzzer", "last_lts", "", "agg_fuzzer_last_lts")]
    fn test_build_name(
        #[case] base_name: &str,
        #[case] old_version: &str,
        #[case] version_combination: &str,
        #[case] expected: &str,
    ) {
        let name = build_name(base_name, old_version, version_combination);

        assert_eq!(name, expected);
    }

    // `build_fuzzer_sub_task` tests.

    fn get_evg_fn_name(evg_command: &EvgCommand) -> Option<&str> {
        if let EvgCommand::Function(func) = evg_command {
            Some(&func.func)
        } else {
            None
        }
    }

    #[test]
    fn test_build_fuzzer_sub_task() {
        let display_name = "my_task";
        let sub_task_index = 42_u64;
        let params = FuzzerGenTaskParams {
            task_name: "some task".to_string(),
            ..Default::default()
        };

        let sub_task = build_fuzzer_sub_task(display_name, sub_task_index, &params, None, None);

        assert_eq!(sub_task.name, "my_task_42");
        assert_eq!(get_evg_fn_name(&sub_task.commands[0]), Some("do setup"));
        assert_eq!(
            get_evg_fn_name(&sub_task.commands[3]),
            Some("run jstestfuzz")
        );
        assert_eq!(
            get_evg_fn_name(&sub_task.commands[4]),
            Some("run generated tests")
        );
        assert_eq!(
            sub_task.depends_on.unwrap()[0].name,
            "archive_dist_test_debug"
        )
    }

    #[test]
    fn test_build_multiversion_fuzzer_sub_task() {
        let display_name = "my_task";
        let sub_task_index = 42_u64;
        let params = FuzzerGenTaskParams {
            task_name: "some task".to_string(),
            require_multiversion_setup: Some(true),
            ..Default::default()
        };

        let sub_task = build_fuzzer_sub_task(display_name, sub_task_index, &params, None, None);

        assert_eq!(sub_task.name, "my_task_42");
        assert_eq!(
            get_evg_fn_name(&sub_task.commands[0]),
            Some("git get project no modules")
        );
        assert_eq!(get_evg_fn_name(&sub_task.commands[2]), Some("do setup"));
        assert_eq!(
            get_evg_fn_name(&sub_task.commands[4]),
            Some("do multiversion setup")
        );
        assert_eq!(
            get_evg_fn_name(&sub_task.commands[6]),
            Some("run jstestfuzz")
        );
        assert_eq!(
            get_evg_fn_name(&sub_task.commands[7]),
            Some("run generated tests")
        );
        assert_eq!(
            sub_task.depends_on.unwrap()[0].name,
            "archive_dist_test_debug"
        )
    }
}
