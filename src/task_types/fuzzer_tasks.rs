use std::collections::HashMap;

use anyhow::Result;
use maplit::hashmap;
use shrub_rs::models::{
    commands::{fn_call, fn_call_with_params},
    params::ParamValue,
    task::{EvgTask, TaskDependency},
};
use tracing::{event, Level};

use crate::{
    evergreen::evg_config_utils::MultiversionGenerateTaskConfig,
    evergreen_names::{
        ADD_GIT_TAG, CONFIGURE_EVG_API_CREDS, CONTINUE_ON_FAILURE, DO_MULTIVERSION_SETUP, DO_SETUP,
        FUZZER_PARAMETERS, GEN_TASK_CONFIG_LOCATION, GET_PROJECT_WITH_NO_MODULES, IDLE_TIMEOUT,
        MULTIVERSION_EXCLUDE_TAGS, NPM_COMMAND, REQUIRE_MULTIVERSION_SETUP, RESMOKE_ARGS,
        RESMOKE_JOBS_MAX, RUN_FUZZER, RUN_GENERATED_TESTS, SETUP_JSTESTFUZZ, SHOULD_SHUFFLE_TESTS,
        SUITE_NAME, TASK_NAME,
    },
    utils::task_name::name_generated_task,
};

use super::generated_suite::{GeneratedSubTask, GeneratedSuite};

/// Parameters for how a fuzzer task should be generated.
#[derive(Default, Debug, Clone)]
pub struct FuzzerGenTaskParams {
    /// Name of task being generated.
    pub task_name: String,
    /// Multiversion tasks to generate.
    pub multiversion_generate_tasks: Option<Vec<MultiversionGenerateTaskConfig>>,
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
    pub require_multiversion_setup: bool,
    /// Location of generated task configuration.
    pub config_location: String,
    /// List of tasks generated sub-tasks should depend on.
    pub dependencies: Vec<String>,
    /// Is this task for enterprise builds.
    pub is_enterprise: bool,
    /// Name of platform the task will run on.
    pub platform: Option<String>,
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
        self.require_multiversion_setup
    }

    /// Build the vars to send to tasks in the 'run tests' function.
    ///
    /// # Arguments
    ///
    /// * `generated_suite_name` - A generated suite to execute against.
    /// * `old_version` - Previous version of mongo to test against.
    ///
    /// # Returns
    ///
    /// Map of arguments to pass to 'run tests' function.
    fn build_run_tests_vars(
        &self,
        generated_suite_name: Option<&str>,
        old_version: Option<&str>,
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

        if let Some(old_version) = old_version {
            vars.insert(
                MULTIVERSION_EXCLUDE_TAGS.to_string(),
                ParamValue::from(old_version),
            );
        }

        vars
    }

    /// Build the dependency structure to use the the generated sub-tasks.
    ///
    /// # Returns
    ///
    /// List of `TaskDependency`s for generated tasks.
    fn get_dependencies(&self) -> Option<Vec<TaskDependency>> {
        if self.dependencies.is_empty() {
            None
        } else {
            Some(
                self.dependencies
                    .iter()
                    .map(|d| TaskDependency {
                        name: d.to_string(),
                        variant: None,
                    })
                    .collect(),
            )
        }
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
    fn sub_tasks(&self) -> Vec<GeneratedSubTask> {
        self.sub_tasks
            .clone()
            .into_iter()
            .map(|sub_task| GeneratedSubTask {
                evg_task: sub_task,
                use_large_distro: false,
                use_xlarge_distro: false,
            })
            .collect()
    }
}

/// A service for generating fuzzer tasks.
pub trait GenFuzzerService: Sync + Send {
    /// Generate a fuzzer task.
    fn generate_fuzzer_task(&self, params: &FuzzerGenTaskParams)
        -> Result<Box<dyn GeneratedSuite>>;
}

/// Implementation of the GenFuzzerService.
pub struct GenFuzzerServiceImpl {}

impl GenFuzzerServiceImpl {
    /// Create a new instance of the GenFuzzerService.
    pub fn new() -> Self {
        Self {}
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
            event!(
                Level::INFO,
                task_name = task_name.as_str(),
                "Generating multiversion fuzzer"
            );
            for multiversion_task in params.multiversion_generate_tasks.as_ref().unwrap() {
                sub_tasks.extend(
                    (0..params.num_tasks as usize)
                        .map(|i| {
                            build_fuzzer_sub_task(
                                &multiversion_task.suite_name,
                                i,
                                params,
                                Some(&multiversion_task.suite_name),
                                Some(&multiversion_task.old_version),
                            )
                        })
                        .collect::<Vec<EvgTask>>(),
                );
            }
        } else {
            sub_tasks = (0..params.num_tasks as usize)
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
/// * `old_version` - Previous version of mongo to test against.
///
/// # Returns
///
/// A shrub task to generate the sub-task.
fn build_fuzzer_sub_task(
    display_name: &str,
    sub_task_index: usize,
    params: &FuzzerGenTaskParams,
    generated_suite_name: Option<&str>,
    old_version: Option<&str>,
) -> EvgTask {
    let sub_task_name = name_generated_task(
        display_name,
        sub_task_index,
        params.num_tasks as usize,
        params.is_enterprise,
        params.platform.as_deref(),
    );

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
            params.build_run_tests_vars(generated_suite_name, old_version),
        ),
    ]);

    EvgTask {
        name: sub_task_name,
        commands: Some(commands),
        depends_on: params.get_dependencies(),
        ..Default::default()
    }
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
    #[case(true, true)]
    #[case(false, false)]
    fn test_is_multiversion(#[case] require_multiversion_setup: bool, #[case] actual: bool) {
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

    #[test]
    fn test_build_task_ref() {
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

        let task_refs = fuzzer_task.build_task_ref(Some("distro".to_string()));

        for task in task_refs {
            assert_eq!(task.distros.as_ref(), None);
        }
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
        let sub_task_index = 42;
        let params = FuzzerGenTaskParams {
            task_name: "some task".to_string(),
            dependencies: vec!["archive_dist_test_debug".to_string()],
            ..Default::default()
        };

        let sub_task = build_fuzzer_sub_task(display_name, sub_task_index, &params, None, None);

        assert_eq!(sub_task.name, "my_task_42");
        assert!(sub_task.commands.is_some());
        let commands = sub_task.commands.unwrap();
        assert_eq!(get_evg_fn_name(&commands[0]), Some("do setup"));
        assert_eq!(get_evg_fn_name(&commands[3]), Some("run jstestfuzz"));
        assert_eq!(get_evg_fn_name(&commands[4]), Some("run generated tests"));
        assert_eq!(
            sub_task.depends_on.unwrap()[0].name,
            "archive_dist_test_debug"
        )
    }

    #[test]
    fn test_build_multiversion_fuzzer_sub_task() {
        let display_name = "my_task";
        let sub_task_index = 42;
        let params = FuzzerGenTaskParams {
            task_name: "some task".to_string(),
            require_multiversion_setup: true,
            dependencies: vec!["archive_dist_test_debug".to_string()],
            ..Default::default()
        };

        let sub_task = build_fuzzer_sub_task(display_name, sub_task_index, &params, None, None);

        assert_eq!(sub_task.name, "my_task_42");
        assert!(sub_task.commands.is_some());
        let commands = sub_task.commands.unwrap();
        assert_eq!(
            get_evg_fn_name(&commands[0]),
            Some("git get project no modules")
        );
        assert_eq!(get_evg_fn_name(&commands[2]), Some("do setup"));
        assert_eq!(get_evg_fn_name(&commands[4]), Some("do multiversion setup"));
        assert_eq!(get_evg_fn_name(&commands[6]), Some("run jstestfuzz"));
        assert_eq!(get_evg_fn_name(&commands[7]), Some("run generated tests"));
        assert_eq!(
            sub_task.depends_on.unwrap()[0].name,
            "archive_dist_test_debug"
        )
    }
}
