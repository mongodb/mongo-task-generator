//! Service for generating resmoke tasks.
//!
//! This service will query the historic runtime of tests in the given task and then
//! use that information to divide the tests into sub-suites that can be run in parallel.
//!
//! Each task will contain the generated sub-suites.
use std::{cmp::min, collections::HashMap, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use maplit::hashmap;
use rand::{prelude::SliceRandom, thread_rng};
use shrub_rs::models::{
    commands::{fn_call, fn_call_with_params, EvgCommand},
    params::ParamValue,
    task::{EvgTask, TaskDependency},
    variant::BuildVariant,
};
use tokio::sync::Mutex;
use tracing::{event, warn, Level};

use crate::{
    evergreen::{
        evg_config_utils::MultiversionGenerateTaskConfig,
        evg_task_history::{
            get_test_name, TaskHistoryService, TaskRuntimeHistory, TestRuntimeHistory,
        },
    },
    evergreen_names::{
        ADD_GIT_TAG, CONFIGURE_EVG_API_CREDS, DO_MULTIVERSION_SETUP, DO_SETUP,
        GEN_TASK_CONFIG_LOCATION, GET_PROJECT_WITH_NO_MODULES, MULTIVERSION_EXCLUDE_TAG,
        MULTIVERSION_EXCLUDE_TAGS_FILE, REQUIRE_MULTIVERSION_SETUP, RESMOKE_ARGS, RESMOKE_JOBS_MAX,
        RUN_GENERATED_TESTS, SUITE_NAME,
    },
    resmoke::resmoke_proxy::TestDiscovery,
    utils::{fs_service::FsService, task_name::name_generated_task},
    SubtaskLimits, REQUIRED_PREFIX,
};

use super::{
    generated_suite::{GeneratedSubTask, GeneratedSuite},
    multiversion::MultiversionService,
    resmoke_config_writer::ResmokeConfigActor,
};

/// Parameters describing how a specific resmoke suite should be generated.
#[derive(Clone, Debug, Default)]
pub struct ResmokeGenParams {
    /// Name of task being generated.
    pub task_name: String,
    /// Multiversion tasks to generate.
    pub multiversion_generate_tasks: Option<Vec<MultiversionGenerateTaskConfig>>,
    /// Name of suite being generated.
    pub suite_name: String,
    /// Should the generated tasks run on a 'large' distro.
    pub use_large_distro: bool,
    /// Should the generated tasks run on a 'xlarge' distro.
    pub use_xlarge_distro: bool,
    /// Does this task require multiversion setup.
    pub require_multiversion_setup: bool,
    /// Should multiversion generate tasks exist for this.
    pub require_multiversion_generate_tasks: bool,
    /// Specify how many times resmoke should repeat the suite being tested.
    pub repeat_suites: Option<u64>,
    /// Arguments that should be passed to resmoke.
    pub resmoke_args: String,
    /// Number of jobs to limit resmoke to.
    pub resmoke_jobs_max: Option<u64>,
    /// Location where generated task configuration will be stored in S3.
    pub config_location: String,
    /// List of tasks generated sub-tasks should depend on.
    pub dependencies: Vec<String>,
    /// Is this task for enterprise builds.
    pub is_enterprise: bool,
    /// Arguments to pass to 'run tests' function.
    pub pass_through_vars: Option<HashMap<String, ParamValue>>,
    /// Name of platform the task will run on.
    pub platform: Option<String>,
    /// Name of variant specific suffix to add to tasks
    pub gen_task_suffix: Option<String>,
    /// Number of sub-tasks requested in the task's Evergreen YAML definition.
    pub num_tasks: Option<usize>,
}

impl ResmokeGenParams {
    /// Build the vars to send to the tasks in the 'run tests' function.
    ///
    /// # Arguments
    ///
    /// * `suite_file` - Name of suite file to run.
    ///
    /// # Returns
    ///
    /// Map of arguments to pass to 'run tests' function.
    fn build_run_test_vars(
        &self,
        suite_file: &str,
        sub_suite: &SubSuite,
        exclude_tags: &str,
        suite_override: Option<String>,
    ) -> HashMap<String, ParamValue> {
        let mut run_test_vars: HashMap<String, ParamValue> = hashmap! {};
        if let Some(pass_through_vars) = &self.pass_through_vars {
            run_test_vars.extend(pass_through_vars.clone());
        }

        let resmoke_args = self.build_resmoke_args(exclude_tags, &sub_suite.origin_suite);
        let suite = if let Some(suite_override) = suite_override {
            suite_override
        } else {
            format!("generated_resmoke_config/{}.yml", suite_file)
        };

        run_test_vars.extend(hashmap! {
            REQUIRE_MULTIVERSION_SETUP.to_string() => ParamValue::from(self.require_multiversion_setup),
            RESMOKE_ARGS.to_string() => ParamValue::from(resmoke_args.as_str()),
            SUITE_NAME.to_string() => ParamValue::from(suite.as_str()),
            GEN_TASK_CONFIG_LOCATION.to_string() => ParamValue::from(self.config_location.as_str()),
        });

        if let Some(mv_exclude_tags) = &sub_suite.mv_exclude_tags {
            run_test_vars.insert(
                MULTIVERSION_EXCLUDE_TAG.to_string(),
                ParamValue::from(mv_exclude_tags.as_str()),
            );
        }

        if let Some(resmoke_jobs_max) = self.resmoke_jobs_max {
            run_test_vars.insert(
                RESMOKE_JOBS_MAX.to_string(),
                ParamValue::from(resmoke_jobs_max),
            );
        }

        run_test_vars
    }

    /// Build the resmoke arguments to use for a generate sub-task.
    ///
    /// # Arguments
    ///
    /// * `exclude_tags` - Resmoke tags to exclude.
    /// * `origin_suite` - Suite the generated suite is based on.
    ///
    /// # Returns
    ///
    /// String of arguments to pass to resmoke.
    fn build_resmoke_args(&self, exclude_tags: &str, origin_suite: &str) -> String {
        let suffix = if self.require_multiversion_setup {
            format!(
                "--tagFile=generated_resmoke_config/{} --excludeWithAnyTags={}",
                MULTIVERSION_EXCLUDE_TAGS_FILE, exclude_tags
            )
        } else {
            "".to_string()
        };

        let repeat_arg = if let Some(repeat) = self.repeat_suites {
            format!("--repeatSuites={}", repeat)
        } else {
            "".to_string()
        };

        format!(
            "--originSuite={} {} {} {}",
            origin_suite, repeat_arg, suffix, self.resmoke_args
        )
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

/// Representation of generated sub-suite.
#[derive(Clone, Debug, Default)]
pub struct SubSuite {
    /// Index value of generated suite.
    pub index: usize,

    /// Name of generated sub-suite.
    pub name: String,

    /// List of tests belonging to sub-suite.
    pub test_list: Vec<String>,

    /// Suite the generated suite is based off.
    pub origin_suite: String,

    /// List of tests that should be excluded from sub-suite.
    pub exclude_test_list: Option<Vec<String>>,

    /// Multiversion exclude tags.
    pub mv_exclude_tags: Option<String>,

    /// Is sub-suite for a enterprise build_variant.
    pub is_enterprise: bool,

    /// Platform of build_variant the sub-suite is for.
    pub platform: Option<String>,
}

/// Information needed to generate resmoke configuration files for the generated task.
#[derive(Clone, Debug)]
pub struct ResmokeSuiteGenerationInfo {
    /// Name of task being generated.
    pub task_name: String,

    /// Name of resmoke suite generated task is based on.
    pub origin_suite: String,

    /// List of generated sub-suites comprising task.
    pub sub_suites: Vec<SubSuite>,

    /// If true, sub-tasks should be generated for the multiversion generate tasks.
    pub require_multiversion_generate_tasks: bool,
}

/// Representation of a generated resmoke suite.
#[derive(Clone, Debug, Default)]
pub struct GeneratedResmokeSuite {
    /// Name of display task to create.
    pub task_name: String,

    /// Sub suites that comprise generated task.
    pub sub_suites: Vec<GeneratedSubTask>,
}

impl GeneratedSuite for GeneratedResmokeSuite {
    /// Get the display name to use for the generated task.
    fn display_name(&self) -> String {
        self.task_name.clone()
    }

    /// Get the list of sub-tasks that comprise the generated task.
    fn sub_tasks(&self) -> Vec<GeneratedSubTask> {
        self.sub_suites.clone()
    }
}

/// A service for generating resmoke tasks.
#[async_trait]
pub trait GenResmokeTaskService: Sync + Send {
    /// Generate a task for running the given task in parallel.
    ///
    /// # Arguments
    ///
    /// * `param` - Parameters for how task should be generated.
    /// * `build_variant` - Build variant to base task splitting on.
    ///
    /// # Returns
    ///
    /// A generated suite representing the split task.
    async fn generate_resmoke_task(
        &self,
        params: &ResmokeGenParams,
        build_variant: &BuildVariant,
    ) -> Result<Box<dyn GeneratedSuite>>;

    /// Build a shrub task to execute a split sub-task.
    ///
    /// # Arguments
    ///
    /// * `sub_suite` - Sub task to create task for.
    /// * `params` - Parameters for how task should be generated.
    ///
    /// # Returns
    ///
    /// A shrub task to execute the given sub-suite.
    fn build_resmoke_sub_task(
        &self,
        sub_suite: &SubSuite,
        total_sub_suites: usize,
        params: &ResmokeGenParams,
        suite_override: Option<String>,
    ) -> GeneratedSubTask;
}

#[derive(Debug, Clone)]
pub struct GenResmokeConfig {
    /// Disable evergreen task-history queries and use task splitting fallback.
    use_task_split_fallback: bool,

    /// Enterprise directory.
    enterprise_dir: Option<String>,
}

impl GenResmokeConfig {
    /// Create a new GenResmokeConfig.
    ///
    /// # Arguments
    ///
    /// * `n_suite` - Number of sub-suites to split tasks into.
    /// * `use_task_split_fallback` - Disable evergreen task-history queries and use task
    ///    splitting fallback.
    /// * `enterprise_dir` - Directory enterprise files are stored in.
    ///
    /// # Returns
    ///
    /// New instance of `GenResmokeConfig`.
    pub fn new(use_task_split_fallback: bool, enterprise_dir: Option<String>) -> Self {
        Self {
            use_task_split_fallback,
            enterprise_dir,
        }
    }
}

/// Implementation of service to generate resmoke tasks.
#[derive(Clone)]
pub struct GenResmokeTaskServiceImpl {
    /// Service to query task runtime history.
    task_history_service: Arc<dyn TaskHistoryService>,

    /// Test discovery service.
    test_discovery: Arc<dyn TestDiscovery>,

    /// Actor to create resmoke configuration files.
    resmoke_config_actor: Arc<Mutex<dyn ResmokeConfigActor>>,

    /// Service for generating multiversion configurations.
    multiversion_service: Arc<dyn MultiversionService>,

    /// Service to interact with file system.
    fs_service: Arc<dyn FsService>,

    /// Configuration for generating resmoke tasks.
    config: GenResmokeConfig,

    subtask_limits: SubtaskLimits,
}

impl GenResmokeTaskServiceImpl {
    /// Create a new instance of the service implementation.
    ///
    /// # Arguments
    ///
    /// * `task_history_service` - An instance of the service to query task history.
    /// * `test_discovery` - An instance of the service to query tests belonging to a task.
    /// * `fs_service` - An instance of the service too work with the file system.
    /// * `gen_resmoke_config` - Configuration for how resmoke tasks should be generated.
    ///
    /// # Returns
    ///
    /// New instance of GenResmokeTaskService.
    pub fn new(
        task_history_service: Arc<dyn TaskHistoryService>,
        test_discovery: Arc<dyn TestDiscovery>,
        resmoke_config_actor: Arc<Mutex<dyn ResmokeConfigActor>>,
        multiversion_service: Arc<dyn MultiversionService>,
        fs_service: Arc<dyn FsService>,
        config: GenResmokeConfig,
        subtask_limits: SubtaskLimits,
    ) -> Self {
        Self {
            task_history_service,
            test_discovery,
            resmoke_config_actor,
            multiversion_service,
            fs_service,
            config,
            subtask_limits,
        }
    }
}

impl GenResmokeTaskServiceImpl {
    /// Split the given task into a number of sub-tasks for parallel execution.
    ///
    /// # Arguments
    ///
    /// * `params` - Parameters for how tasks should be generated.
    /// * `task_stats` - Statistics on the historic runtimes of tests in the task.
    /// * `multiversion_name` - Name of task if performing multiversion generation.
    /// * `multiversion_tags` - Tag to include when performing multiversion generation.
    /// * `build_variant` - Build variant to base generation off of.
    ///
    /// # Returns
    ///
    /// A list of sub-suites to run the tests is the given task.
    fn split_task(
        &self,
        params: &ResmokeGenParams,
        task_stats: &TaskRuntimeHistory,
        multiversion_name: Option<&str>,
        multiversion_tags: Option<String>,
        build_variant: &BuildVariant,
    ) -> Result<Vec<SubSuite>> {
        let origin_suite = multiversion_name.unwrap_or(&params.suite_name);
        let test_list = self.get_test_list(params, multiversion_name)?;
        let total_runtime = task_stats
            .test_map
            .iter()
            .filter(|(_, history)| test_list.contains(&history.test_name))
            .fold(0.0, |init, (_, item)| init + item.average_runtime);

        let ideal_num_tasks = match params.num_tasks {
            Some(t) => t,
            None if build_variant
                .display_name
                .as_ref()
                .unwrap()
                .starts_with(REQUIRED_PREFIX)
                && total_runtime > self.subtask_limits.large_required_task_runtime_threshold =>
            {
                self.subtask_limits.default_subtasks_per_task
                    + ((total_runtime - self.subtask_limits.large_required_task_runtime_threshold)
                        / self.subtask_limits.test_runtime_per_required_subtask)
                        as usize
            }
            None => self.subtask_limits.default_subtasks_per_task,
        };

        let num_tasks = *[
            ideal_num_tasks,
            test_list.len(),
            self.subtask_limits.max_subtasks_per_task,
        ]
        .iter()
        .min()
        .unwrap();

        let runtime_per_subtask = total_runtime / num_tasks as f64;
        event!(
            Level::INFO,
            "Splitting task: {}, runtime: {}, tests: {}",
            &params.suite_name,
            runtime_per_subtask,
            test_list.len()
        );

        let sorted_test_list = sort_tests_by_runtime(test_list, task_stats);
        let mut running_tests = vec![vec![]; num_tasks];
        let mut running_runtimes = vec![0.0; num_tasks];
        let mut left_tests = vec![];

        for test in sorted_test_list {
            let min_idx = get_min_index(&running_runtimes);
            let test_name = get_test_name(&test);
            if let Some(test_stats) = task_stats.test_map.get(&test_name) {
                running_runtimes[min_idx] += test_stats.average_runtime;
                running_tests[min_idx].push(test.clone());
            } else {
                left_tests.push(test.clone());
            }
        }

        let min_idx = get_min_index(&running_runtimes);
        for (i, test) in left_tests.iter().enumerate() {
            running_tests[(min_idx + i) % num_tasks].push(test.clone());
        }

        let mut sub_suites = vec![];
        for (i, slice) in running_tests.iter().enumerate() {
            sub_suites.push(SubSuite {
                index: i,
                name: multiversion_name.unwrap_or(&params.task_name).to_string(),
                test_list: slice.clone(),
                origin_suite: origin_suite.to_string(),
                exclude_test_list: None,
                mv_exclude_tags: multiversion_tags.clone(),
                is_enterprise: params.is_enterprise,
                platform: params.platform.clone(),
            });
        }

        Ok(sub_suites)
    }

    /// Get the list of tests belonging to the suite being generated.
    ///
    /// # Arguments
    ///
    /// * `params` - Parameters about the suite being split.
    ///
    /// # Returns
    ///
    /// List of tests belonging to suite being split.
    fn get_test_list(
        &self,
        params: &ResmokeGenParams,
        multiversion_name: Option<&str>,
    ) -> Result<Vec<String>> {
        let suite_name = multiversion_name.unwrap_or(&params.suite_name);
        let mut test_list: Vec<String> = self
            .test_discovery
            .discover_tests(suite_name)?
            .into_iter()
            .filter(|s| self.fs_service.file_exists(s))
            .collect();

        if !params.is_enterprise {
            if let Some(enterprise_dir) = &self.config.enterprise_dir {
                test_list.retain(|s| !s.starts_with(enterprise_dir));
            }
        }

        test_list.shuffle(&mut thread_rng());

        Ok(test_list)
    }

    /// Split a task with no historic runtime information.
    ///
    /// Since we don't have any runtime information, we will just split the tests evenly among
    /// the number of suites we want to create.
    ///
    /// # Arguments
    ///
    /// * `params` - Parameters for how tasks should be generated.
    /// * `multiversion_name` - Name of task if performing multiversion generation.
    /// * `multiversion_tags` - Tag to include when performing multiversion generation.
    ///
    /// # Returns
    ///
    /// A list of sub-suites to run the tests is the given task.
    fn split_task_fallback(
        &self,
        params: &ResmokeGenParams,
        multiversion_name: Option<&str>,
        multiversion_tags: Option<String>,
    ) -> Result<Vec<SubSuite>> {
        let mut sub_suites = vec![];

        let origin_suite = multiversion_name.unwrap_or(&params.suite_name);
        let test_list = self.get_test_list(params, multiversion_name)?;
        if test_list.is_empty() {
            return Ok(sub_suites);
        }

        let requested_num_tasks = match params.num_tasks {
            Some(tasks) => tasks,
            None => self.subtask_limits.default_subtasks_per_task,
        };

        let n = min(test_list.len(), requested_num_tasks);
        let len = test_list.len();
        let (quo, rem) = (len / n, len % n);
        let split = (quo + 1) * rem;
        let iter = test_list[..split]
            .chunks(quo + 1)
            .chain(test_list[split..].chunks(quo));
        for (index, tests) in iter.enumerate() {
            sub_suites.push(SubSuite {
                index,
                name: multiversion_name.unwrap_or(&params.task_name).to_string(),
                test_list: tests.to_vec(),
                origin_suite: origin_suite.to_string(),
                exclude_test_list: None,
                mv_exclude_tags: multiversion_tags.clone(),
                is_enterprise: params.is_enterprise,
                platform: params.platform.clone(),
            });
        }
        Ok(sub_suites)
    }

    /// Create version of the generated sub-tasks for all the multiversion combinations.
    ///
    /// # Arguments
    ///
    /// * `params` - Parameters for how task should be generated.
    /// * `build_variant` - Build variant to base generation off.
    ///
    /// # Returns
    ///
    /// List of all sub-suites for a multiversion task with generate tasks.
    async fn create_multiversion_tasks(
        &self,
        params: &ResmokeGenParams,
        build_variant: &BuildVariant,
    ) -> Result<Vec<SubSuite>> {
        let mut mv_sub_suites = vec![];
        for multiversion_task in params.multiversion_generate_tasks.as_ref().unwrap() {
            let suites = self
                .create_tasks(
                    params,
                    build_variant,
                    Some(&multiversion_task.suite_name.clone()),
                    Some(multiversion_task.old_version.clone()),
                )
                .await?;
            mv_sub_suites.extend_from_slice(&suites);
        }

        Ok(mv_sub_suites)
    }

    /// Create sub-suites based on the given information.
    ///
    /// # Arguments
    ///
    /// * `params` - Parameters for how tasks should be generated.
    /// * `build_variant` - Build variant to base generation off of.
    /// * `multiversion_name` - Name of task if performing multiversion generation.
    /// * `multiversion_tags` - Tag to include when performing multiversion generation.
    ///
    /// # Returns
    ///
    /// List of sub-suites that were generated.
    async fn create_tasks(
        &self,
        params: &ResmokeGenParams,
        build_variant: &BuildVariant,
        multiversion_name: Option<&str>,
        multiversion_tags: Option<String>,
    ) -> Result<Vec<SubSuite>> {
        let sub_suites = if self.config.use_task_split_fallback {
            self.split_task_fallback(params, multiversion_name, multiversion_tags.clone())?
        } else {
            let task_history = self
                .task_history_service
                .get_task_history(&params.task_name, &build_variant.name)
                .await;

            match task_history {
                Ok(task_history) => self.split_task(
                    params,
                    &task_history,
                    multiversion_name,
                    multiversion_tags.clone(),
                    build_variant,
                )?,
                Err(err) => {
                    warn!(
                        build_variant = build_variant.name,
                        task_name = params.task_name.as_str(),
                        error = err.to_string().as_str(),
                        "Could not get task history from S3",
                    );
                    // If we couldn't get the task history, then fallback to splitting the tests evenly
                    // among the desired number of sub-suites.
                    self.split_task_fallback(params, multiversion_name, multiversion_tags.clone())?
                }
            }
        };

        Ok(sub_suites)
    }
}

/// Sort tests by historic runtime descending.
///
/// Tests without historic runtime data will be placed at the end of the list.
///
/// # Arguments
///
/// * `test_list` - List of tests.
/// * `task_stats` - Historic task stats.
///
/// # Returns
///
/// List of sorted tests by historic runtime.
fn sort_tests_by_runtime(
    test_list: Vec<String>,
    task_stats: &TaskRuntimeHistory,
) -> Vec<std::string::String> {
    let mut sorted_test_list = test_list;
    sorted_test_list.sort_by(|test_file_a, test_file_b| {
        let test_name_a = get_test_name(test_file_a);
        let test_name_b = get_test_name(test_file_b);
        let default_runtime = TestRuntimeHistory {
            test_name: "default".to_string(),
            average_runtime: 0.0,
            hooks: vec![],
        };
        let runtime_history_a = task_stats
            .test_map
            .get(&test_name_a)
            .unwrap_or(&default_runtime);
        let runtime_history_b = task_stats
            .test_map
            .get(&test_name_b)
            .unwrap_or(&default_runtime);
        runtime_history_b
            .average_runtime
            .partial_cmp(&runtime_history_a.average_runtime)
            .unwrap()
    });
    sorted_test_list
}

/// Get the index of sub suite with the least total runtime of tests.
///
/// # Arguments
///
/// * `running_runtimes` - Total runtimes of tests of sub suites.
///
/// # Returns
///
/// Index of sub suite with the least total runtime.
fn get_min_index(running_runtimes: &[f64]) -> usize {
    let mut min_idx = 0;
    for (i, value) in running_runtimes.iter().enumerate() {
        if value < &running_runtimes[min_idx] {
            min_idx = i;
        }
    }
    min_idx
}

#[async_trait]
impl GenResmokeTaskService for GenResmokeTaskServiceImpl {
    /// Generate a task for running the given task in parallel.
    ///
    /// # Arguments
    ///
    /// * `params` - Parameters for how task should be generated.
    /// * `build_variant` - Build variant to base task splitting on.
    ///
    /// # Returns
    ///
    /// A generated suite representing the split task.
    async fn generate_resmoke_task(
        &self,
        params: &ResmokeGenParams,
        build_variant: &BuildVariant,
    ) -> Result<Box<dyn GeneratedSuite>> {
        let sub_suites = if params.require_multiversion_generate_tasks {
            self.create_multiversion_tasks(params, build_variant)
                .await?
        } else {
            self.create_tasks(params, build_variant, None, None).await?
        };

        let sub_task_total = sub_suites.len();
        let suite_info = ResmokeSuiteGenerationInfo {
            task_name: params.task_name.to_string(),
            origin_suite: params.suite_name.to_string(),
            sub_suites: sub_suites.clone(),
            require_multiversion_generate_tasks: params.require_multiversion_generate_tasks,
        };
        let mut resmoke_config_actor = self.resmoke_config_actor.lock().await;
        resmoke_config_actor.write_sub_suite(&suite_info).await;

        Ok(Box::new(GeneratedResmokeSuite {
            task_name: params.task_name.clone(),
            sub_suites: sub_suites
                .into_iter()
                .map(|s| self.build_resmoke_sub_task(&s, sub_task_total, params, None))
                .collect(),
        }))
    }

    /// Build a shrub task to execute a split sub-task.
    ///
    /// # Arguments
    ///
    /// * `sub_suite` - Sub task to create task for.
    /// * `params` - Parameters for how task should be generated.
    ///
    /// # Returns
    ///
    /// A shrub task to execute the given sub-suite.
    fn build_resmoke_sub_task(
        &self,
        sub_suite: &SubSuite,
        total_sub_suites: usize,
        params: &ResmokeGenParams,
        suite_override: Option<String>,
    ) -> GeneratedSubTask {
        let exclude_tags = self
            .multiversion_service
            .exclude_tags_for_task(&params.task_name, sub_suite.mv_exclude_tags.clone());
        let suite_file = name_generated_task(
            &sub_suite.name,
            sub_suite.index,
            total_sub_suites,
            params.is_enterprise,
            params.platform.as_deref(),
        );

        let run_test_vars =
            params.build_run_test_vars(&suite_file, sub_suite, &exclude_tags, suite_override);

        let formatted_name = format!(
            "{}{}",
            suite_file,
            params.gen_task_suffix.as_deref().unwrap_or("")
        );
        GeneratedSubTask {
            evg_task: EvgTask {
                name: formatted_name,
                commands: Some(resmoke_commands(
                    RUN_GENERATED_TESTS,
                    run_test_vars,
                    params.require_multiversion_setup,
                )),
                depends_on: params.get_dependencies(),
                ..Default::default()
            },
            use_large_distro: params.use_large_distro,
            use_xlarge_distro: params.use_xlarge_distro,
        }
    }
}

/// Create a list of commands to run a resmoke task in evergreen.
///
/// # Arguments
///
/// * `run_test_fn_name` - Name of function to run tests.
/// * `run_test_vars` - Variable to pass to the run tests function.
/// * `requires_multiversion` - Does this task require multiversion setup.
///
/// # Returns
///
/// A list of evergreen commands comprising the task.
fn resmoke_commands(
    run_test_fn_name: &str,
    run_test_vars: HashMap<String, ParamValue>,
    requires_multiversion_setup: bool,
) -> Vec<EvgCommand> {
    let mut commands = vec![];

    if requires_multiversion_setup {
        commands.push(fn_call(GET_PROJECT_WITH_NO_MODULES));
        commands.push(fn_call(ADD_GIT_TAG));
    }

    commands.push(fn_call(DO_SETUP));
    commands.push(fn_call(CONFIGURE_EVG_API_CREDS));

    if requires_multiversion_setup {
        commands.push(fn_call(DO_MULTIVERSION_SETUP));
    }

    commands.push(fn_call_with_params(run_test_fn_name, run_test_vars));
    commands
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        evergreen::evg_task_history::TestRuntimeHistory,
        resmoke::{resmoke_proxy::MultiversionConfig, resmoke_suite::ResmokeSuiteConfig},
    };

    use super::*;

    const MOCK_ENTERPRISE_DIR: &str = "src/enterprise";

    // ResmokeGenParams tests.
    #[test]
    fn test_build_run_test_vars() {
        let params = ResmokeGenParams {
            suite_name: "my_suite".to_string(),
            resmoke_args: "resmoke args".to_string(),
            pass_through_vars: Some(hashmap! {
                "suite".to_string() => ParamValue::from("my_suite"),
                "resmoke_args".to_string() => ParamValue::from("resmoke args"),
            }),
            ..Default::default()
        };
        let sub_suite = SubSuite {
            ..Default::default()
        };

        let test_vars = params.build_run_test_vars("my_suite_0", &sub_suite, "", None);

        assert_eq!(test_vars.len(), 4);
        assert!(!test_vars.contains_key("resmoke_jobs_max"));
        assert_eq!(
            test_vars.get("suite").unwrap(),
            &ParamValue::from("generated_resmoke_config/my_suite_0.yml")
        );
    }

    #[test]
    fn test_build_run_test_vars_with_resmoke_jobs() {
        let params = ResmokeGenParams {
            suite_name: "my_suite".to_string(),
            resmoke_args: "resmoke args".to_string(),
            resmoke_jobs_max: Some(5),
            pass_through_vars: Some(hashmap! {
                "suite".to_string() => ParamValue::from("my_suite"),
                "resmoke_args".to_string() => ParamValue::from("resmoke args"),
                "resmoke_jobs_max".to_string() => ParamValue::from(5),
            }),
            ..Default::default()
        };
        let sub_suite = SubSuite {
            ..Default::default()
        };

        let test_vars = params.build_run_test_vars("my_suite_0", &sub_suite, "", None);

        assert_eq!(test_vars.len(), 5);
        assert_eq!(
            test_vars.get("resmoke_jobs_max").unwrap(),
            &ParamValue::from(5)
        );
        assert_eq!(
            test_vars.get("suite").unwrap(),
            &ParamValue::from("generated_resmoke_config/my_suite_0.yml")
        );
    }

    #[test]
    fn test_build_run_test_vars_for_multiversion() {
        let params = ResmokeGenParams {
            suite_name: "my_suite".to_string(),
            resmoke_args: "resmoke args".to_string(),
            require_multiversion_setup: true,
            pass_through_vars: Some(hashmap! {
                "suite".to_string() => ParamValue::from("my_suite"),
                "resmoke_args".to_string() => ParamValue::from("resmoke args"),
            }),
            ..Default::default()
        };
        let sub_suite = SubSuite {
            mv_exclude_tags: Some("last_lts".to_string()),
            origin_suite: "my_origin_suite".to_string(),
            ..Default::default()
        };

        let test_vars =
            params.build_run_test_vars("my_suite_0", &sub_suite, "tag_0,tag_1,tag_2", None);

        assert_eq!(test_vars.len(), 5);
        assert_eq!(
            test_vars.get("multiversion_exclude_tags_version").unwrap(),
            &ParamValue::from("last_lts")
        );
        assert_eq!(
            test_vars.get("resmoke_args").unwrap(),
            &ParamValue::from("--originSuite=my_origin_suite  --tagFile=generated_resmoke_config/multiversion_exclude_tags.yml --excludeWithAnyTags=tag_0,tag_1,tag_2 resmoke args")
        );
    }

    #[test]
    fn test_build_run_test_vars_with_pass_through_params() {
        let params = ResmokeGenParams {
            suite_name: "my_suite".to_string(),
            resmoke_args: "resmoke args".to_string(),
            pass_through_vars: Some(hashmap! {
                "suite".to_string() => ParamValue::from("my_suite"),
                "resmoke_args".to_string() => ParamValue::from("resmoke args"),
                "multiversion_exclude_tags_version".to_string() => ParamValue::from("last_lts"),
            }),
            ..Default::default()
        };
        let sub_suite = SubSuite {
            ..Default::default()
        };

        let test_vars = params.build_run_test_vars("my_suite_0", &sub_suite, "", None);

        assert_eq!(test_vars.len(), 5);
        assert_eq!(
            test_vars.get("multiversion_exclude_tags_version").unwrap(),
            &ParamValue::from("last_lts")
        );
        assert_eq!(
            test_vars.get("suite").unwrap(),
            &ParamValue::from("generated_resmoke_config/my_suite_0.yml")
        );
    }

    #[test]
    fn test_build_run_test_vars_pass_through_params_does_are_overridden() {
        let params = ResmokeGenParams {
            suite_name: "my_suite".to_string(),
            resmoke_args: "resmoke args".to_string(),
            pass_through_vars: Some(hashmap! {
                "suite".to_string() => ParamValue::from("my_suite"),
                "resmoke_args".to_string() => ParamValue::from("resmoke args"),
                "multiversion_exclude_tags_version".to_string() => ParamValue::from("last_continuous"),
            }),
            ..Default::default()
        };
        let sub_suite = SubSuite {
            mv_exclude_tags: Some("last_lts".to_string()),
            origin_suite: "my_origin_suite".to_string(),
            ..Default::default()
        };

        let test_vars = params.build_run_test_vars("my_suite_0", &sub_suite, "", None);

        assert_eq!(test_vars.len(), 5);
        assert_eq!(
            test_vars.get("multiversion_exclude_tags_version").unwrap(),
            &ParamValue::from("last_lts")
        );
        assert_eq!(
            test_vars.get("suite").unwrap(),
            &ParamValue::from("generated_resmoke_config/my_suite_0.yml")
        );
    }

    #[test]
    fn test_build_resmoke_args() {
        let params = ResmokeGenParams {
            suite_name: "my_suite".to_string(),
            resmoke_args: "--args to --pass to resmoke".to_string(),
            repeat_suites: Some(3),
            ..Default::default()
        };

        let resmoke_args = params.build_resmoke_args("", "my_origin_suite");

        assert!(resmoke_args.contains("--originSuite=my_origin_suite"));
        assert!(resmoke_args.contains("--args to --pass to resmoke"));
        assert!(resmoke_args.contains("--repeatSuites=3"));
    }

    // GeneratedResmokeSuite tests
    #[rstest]
    #[case(vec![false, false, false])]
    #[case(vec![true, false, false])]
    #[case(vec![false, true, false])]
    #[case(vec![false, false, true])]
    #[case(vec![true, true, false])]
    #[case(vec![true, false, true])]
    #[case(vec![false, true, true])]
    #[case(vec![true, true, true])]
    fn test_build_task_ref(#[case] use_large_distro: Vec<bool>) {
        let distro = "distro".to_string();
        let gen_suite = GeneratedResmokeSuite {
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
                    use_xlarge_distro: false,
                })
                .collect(),
        };

        let task_refs = gen_suite.build_task_ref(Some(distro.clone()));

        for (i, task) in task_refs.iter().enumerate() {
            assert_eq!(task.name, format!("sub_suite_name_{}", i));
            if use_large_distro[i] {
                assert_eq!(task.distros.as_ref().unwrap().len(), 1);
                assert!(task.distros.as_ref().unwrap().contains(&distro));
            } else {
                assert_eq!(task.distros.as_ref(), None);
            }
        }
    }

    // split_task tests
    struct MockTaskHistoryService {
        task_history: TaskRuntimeHistory,
    }

    #[async_trait]
    impl TaskHistoryService for MockTaskHistoryService {
        fn build_url(&self, _task: &str, _variant: &str) -> String {
            todo!()
        }

        async fn get_task_history(
            &self,
            _task: &str,
            _variant: &str,
        ) -> Result<TaskRuntimeHistory> {
            Ok(self.task_history.clone())
        }
    }

    struct MockTestDiscovery {
        test_list: Vec<String>,
    }

    impl TestDiscovery for MockTestDiscovery {
        fn discover_tests(&self, _suite_name: &str) -> Result<Vec<String>> {
            Ok(self.test_list.clone())
        }

        fn get_suite_config(&self, _suite_name: &str) -> Result<ResmokeSuiteConfig> {
            todo!()
        }

        fn get_multiversion_config(&self) -> Result<MultiversionConfig> {
            todo!()
        }
    }

    struct MockFsService {}
    impl FsService for MockFsService {
        fn file_exists(&self, _path: &str) -> bool {
            true
        }

        fn write_file(&self, _path: &std::path::Path, _contents: &str) -> Result<()> {
            Ok(())
        }
    }

    struct MockResmokeConfigActor {}
    #[async_trait]
    impl ResmokeConfigActor for MockResmokeConfigActor {
        async fn write_sub_suite(&mut self, _gen_suite: &ResmokeSuiteGenerationInfo) {}

        async fn flush(&mut self) -> Result<Vec<String>> {
            Ok(vec![])
        }
    }

    struct MockMultiversionService {}
    impl MultiversionService for MockMultiversionService {
        fn exclude_tags_for_task(&self, _task_name: &str, _mv_mode: Option<String>) -> String {
            "tag_0,tag_1".to_string()
        }
        fn filter_multiversion_generate_tasks(
            &self,
            multiversion_generate_tasks: Option<Vec<MultiversionGenerateTaskConfig>>,
            _last_versions_expansion: Option<String>,
        ) -> Option<Vec<MultiversionGenerateTaskConfig>> {
            return multiversion_generate_tasks;
        }
    }

    fn build_mocked_service(
        test_list: Vec<String>,
        task_history: TaskRuntimeHistory,
    ) -> GenResmokeTaskServiceImpl {
        let test_discovery = MockTestDiscovery { test_list };
        let multiversion_service = MockMultiversionService {};
        let task_history_service = MockTaskHistoryService {
            task_history: task_history.clone(),
        };
        let fs_service = MockFsService {};
        let resmoke_config_actor = MockResmokeConfigActor {};

        let config = GenResmokeConfig::new(false, Some(MOCK_ENTERPRISE_DIR.to_string()));

        GenResmokeTaskServiceImpl::new(
            Arc::new(task_history_service),
            Arc::new(test_discovery),
            Arc::new(Mutex::new(resmoke_config_actor)),
            Arc::new(multiversion_service),
            Arc::new(fs_service),
            config,
            SubtaskLimits {
                test_runtime_per_required_subtask: 3600.0,
                large_required_task_runtime_threshold: 7200.0,
                default_subtasks_per_task: 5,
                max_subtasks_per_task: 10,
            },
        )
    }

    fn build_mock_test_runtime(test_name: &str, runtime: f64) -> TestRuntimeHistory {
        TestRuntimeHistory {
            test_name: test_name.to_string(),
            average_runtime: runtime,
            hooks: vec![],
        }
    }
    #[test]
    fn test_split_task_should_split_tasks_by_runtime() {
        // In this test we will create 3 subtasks with 6 tests. The first sub task should contain
        // a single test. The second 2 tests and the third 3 tests. We will set the test runtimes
        // to make this happen.
        let num_tasks = 3;
        let test_list: Vec<String> = (0..6)
            .into_iter()
            .map(|i| format!("test_{}.js", i))
            .collect();
        let task_history = TaskRuntimeHistory {
            task_name: "my task".to_string(),
            test_map: hashmap! {
                "test_0".to_string() => build_mock_test_runtime("test_0.js", 100.0),
                "test_1".to_string() => build_mock_test_runtime("test_1.js", 56.0),
                "test_2".to_string() => build_mock_test_runtime("test_2.js", 50.0),
                "test_3".to_string() => build_mock_test_runtime("test_3.js", 35.0),
                "test_4".to_string() => build_mock_test_runtime("test_4.js", 34.0),
                "test_5".to_string() => build_mock_test_runtime("test_5.js", 30.0),
            },
        };
        let gen_resmoke_service = build_mocked_service(test_list.clone(), task_history.clone());

        let params = ResmokeGenParams {
            num_tasks: Some(num_tasks),
            ..Default::default()
        };

        let sub_suites = gen_resmoke_service
            .split_task(
                &params,
                &task_history,
                None,
                None,
                &BuildVariant {
                    display_name: Some("build-variant".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        assert_eq!(sub_suites.len(), num_tasks);
        let suite_0 = &sub_suites[0];
        assert!(suite_0.test_list.contains(&"test_0.js".to_string()));
        let suite_1 = &sub_suites[1];
        assert!(suite_1.test_list.contains(&"test_1.js".to_string()));
        assert!(suite_1.test_list.contains(&"test_4.js".to_string()));
        let suite_2 = &sub_suites[2];
        assert!(suite_2.test_list.contains(&"test_2.js".to_string()));
        assert!(suite_2.test_list.contains(&"test_3.js".to_string()));
        assert!(suite_2.test_list.contains(&"test_5.js".to_string()));
    }
    #[test]
    fn test_split_task_with_missing_history_should_split_tasks_equally() {
        let num_tasks = 3;
        let test_list: Vec<String> = (0..12)
            .into_iter()
            .map(|i| format!("test_{}.js", i))
            .collect();
        let task_history = TaskRuntimeHistory {
            task_name: "my task".to_string(),
            test_map: hashmap! {
                "test_0".to_string() => build_mock_test_runtime("test_0.js", 100.0),
                "test_1".to_string() => build_mock_test_runtime("test_1.js", 50.0),
                "test_2".to_string() => build_mock_test_runtime("test_2.js", 50.0),
            },
        };
        let gen_resmoke_service = build_mocked_service(test_list, task_history.clone());

        let params = ResmokeGenParams {
            num_tasks: Some(num_tasks),
            ..Default::default()
        };

        let sub_suites = gen_resmoke_service
            .split_task(
                &params,
                &task_history,
                None,
                None,
                &BuildVariant {
                    display_name: Some("build-variant".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        assert_eq!(sub_suites.len(), num_tasks);
        let suite_0 = &sub_suites[0];
        assert_eq!(suite_0.test_list.len(), 4);
        let suite_1 = &sub_suites[1];
        assert_eq!(suite_1.test_list.len(), 4);
        let suite_2 = &sub_suites[2];
        assert_eq!(suite_2.test_list.len(), 4);
    }
    #[test]
    fn test_split_tasks_should_include_multiversion_information() {
        let num_tasks = 3;
        let test_list: Vec<String> = (0..3)
            .into_iter()
            .map(|i| format!("test_{}.js", i))
            .collect();
        let task_history = TaskRuntimeHistory {
            task_name: "my task".to_string(),
            test_map: hashmap! {
                "test_0".to_string() => build_mock_test_runtime("test_0.js", 100.0),
                "test_1".to_string() => build_mock_test_runtime("test_1.js", 50.0),
                "test_2".to_string() => build_mock_test_runtime("test_2.js", 50.0),
            },
        };
        let gen_resmoke_service = build_mocked_service(test_list, task_history.clone());

        let params = ResmokeGenParams {
            num_tasks: Some(num_tasks),
            ..Default::default()
        };

        let sub_suites = gen_resmoke_service
            .split_task(
                &params,
                &task_history,
                Some("multiversion_test"),
                Some("multiversion_tag".to_string()),
                &BuildVariant {
                    display_name: Some("build-variant".to_string()),
                    ..Default::default()
                },
            )
            .unwrap();

        assert_eq!(sub_suites.len(), num_tasks);
        for sub_suite in sub_suites {
            assert_eq!(sub_suite.name, "multiversion_test");
            assert_eq!(
                sub_suite.mv_exclude_tags,
                Some("multiversion_tag".to_string())
            );
        }
    }
    // split_task_fallback tests
    #[test]
    fn test_split_task_fallback_should_split_tasks_count() {
        let num_tasks = 3;
        let n_tests = 6;
        let test_list: Vec<String> = (0..n_tests)
            .into_iter()
            .map(|i| format!("test_{}.js", i))
            .collect();
        let task_history = TaskRuntimeHistory {
            task_name: "my task".to_string(),
            test_map: hashmap! {},
        };
        let gen_resmoke_service = build_mocked_service(test_list.clone(), task_history.clone());

        let params = ResmokeGenParams {
            num_tasks: Some(num_tasks),
            ..Default::default()
        };

        let sub_suites = gen_resmoke_service
            .split_task_fallback(&params, None, None)
            .unwrap();
        assert_eq!(sub_suites.len(), num_tasks);
        for sub_suite in &sub_suites {
            assert_eq!(sub_suite.test_list.len(), n_tests / num_tasks);
        }
        let all_tests: Vec<String> = sub_suites
            .iter()
            .flat_map(|s| s.test_list.clone())
            .collect();
        assert_eq!(all_tests.len(), n_tests);
        for test_name in test_list {
            assert!(all_tests.contains(&test_name.to_string()));
        }
    }
    #[test]
    fn test_split_task_fallback_has_remainder() {
        let num_tasks = 3;
        let n_tests = 4;
        let test_list: Vec<String> = (0..n_tests)
            .into_iter()
            .map(|i| format!("test_{}.js", i))
            .collect();
        let task_history = TaskRuntimeHistory {
            task_name: "my task".to_string(),
            test_map: hashmap! {},
        };
        let gen_resmoke_service = build_mocked_service(test_list.clone(), task_history.clone());

        let params = ResmokeGenParams {
            num_tasks: Some(num_tasks),
            ..Default::default()
        };

        let sub_suites = gen_resmoke_service
            .split_task_fallback(&params, None, None)
            .unwrap();
        assert_eq!(sub_suites.len(), num_tasks);
        let all_tests: Vec<String> = sub_suites
            .iter()
            .flat_map(|s| s.test_list.clone())
            .collect();
        assert_eq!(all_tests.len(), n_tests);
        for test_name in test_list {
            assert!(all_tests.contains(&test_name.to_string()));
        }
    }

    #[test]
    fn test_split_task_fallback_empty_suite() {
        let num_tasks = Some(1);
        let test_list = vec![];
        let task_history = TaskRuntimeHistory {
            task_name: "my task".to_string(),
            test_map: hashmap! {},
        };
        let gen_resmoke_service = build_mocked_service(test_list.clone(), task_history.clone());
        let params = ResmokeGenParams {
            num_tasks,
            ..Default::default()
        };
        let sub_suites = gen_resmoke_service
            .split_task_fallback(&params, None, None)
            .unwrap();
        assert_eq!(sub_suites.len(), 0);
    }
    // tests for get_test_list.
    #[rstest]
    #[case(true, 12)]
    #[case(false, 6)]
    fn test_get_test_list_should_filter_enterprise_tests(
        #[case] is_enterprise: bool,
        #[case] expected_tests: usize,
    ) {
        let num_tasks = Some(3);
        let mut test_list: Vec<String> = (0..6)
            .into_iter()
            .map(|i| format!("test_{}.js", i))
            .collect();
        test_list.extend::<Vec<String>>(
            (6..12)
                .into_iter()
                .map(|i| format!("{}/test_{}.js", MOCK_ENTERPRISE_DIR, i))
                .collect(),
        );
        let task_history = TaskRuntimeHistory {
            task_name: "my task".to_string(),
            test_map: hashmap! {},
        };
        let gen_resmoke_service = build_mocked_service(test_list, task_history.clone());
        let params = ResmokeGenParams {
            is_enterprise,
            num_tasks,
            ..Default::default()
        };
        let sub_suites = gen_resmoke_service
            .split_task_fallback(&params, None, None)
            .unwrap();
        let all_tests: Vec<String> = sub_suites
            .iter()
            .flat_map(|s| s.test_list.clone())
            .collect();
        assert_eq!(expected_tests, all_tests.len());
    }
    #[rstest]
    #[case(true, 12)]
    #[case(false, 12)]
    fn test_get_test_list_should_work_with_missing_enterprise_details(
        #[case] is_enterprise: bool,
        #[case] expected_tests: usize,
    ) {
        let num_tasks = Some(3);
        let mut test_list: Vec<String> = (0..6)
            .into_iter()
            .map(|i| format!("test_{}.js", i))
            .collect();
        test_list.extend::<Vec<String>>(
            (6..12)
                .into_iter()
                .map(|i| format!("{}/test_{}.js", MOCK_ENTERPRISE_DIR, i))
                .collect(),
        );
        let task_history = TaskRuntimeHistory {
            task_name: "my task".to_string(),
            test_map: hashmap! {},
        };
        let mut gen_resmoke_service = build_mocked_service(test_list, task_history.clone());
        gen_resmoke_service.config.enterprise_dir = None;
        let params = ResmokeGenParams {
            is_enterprise,
            num_tasks,
            ..Default::default()
        };
        let sub_suites = gen_resmoke_service
            .split_task_fallback(&params, None, None)
            .unwrap();
        let all_tests: Vec<String> = sub_suites
            .iter()
            .flat_map(|s| s.test_list.clone())
            .collect();
        assert_eq!(expected_tests, all_tests.len());
    }
    // create_multiversion_combinations tests.
    #[tokio::test]
    async fn test_create_multiversion_tasks() {
        let params = ResmokeGenParams {
            multiversion_generate_tasks: Some(vec![
                MultiversionGenerateTaskConfig {
                    suite_name: "suite1_last_lts".to_string(),
                    old_version: "last-lts".to_string(),
                },
                MultiversionGenerateTaskConfig {
                    suite_name: "suite1_last_continuous".to_string(),
                    old_version: "last-continuous".to_string(),
                },
            ]),
            num_tasks: Some(1),
            ..Default::default()
        };
        let task_history = TaskRuntimeHistory {
            task_name: "my task".to_string(),
            test_map: hashmap! {},
        };
        let test_list = vec![
            "test_0.js".to_string(),
            "test_1.js".to_string(),
            "test_2.js".to_string(),
            "test_3.js".to_string(),
        ];
        let gen_resmoke_service = build_mocked_service(test_list.clone(), task_history);

        let suite_list = gen_resmoke_service
            .create_multiversion_tasks(
                &params,
                &BuildVariant {
                    display_name: Some("build-variant".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(suite_list[0].name, "suite1_last_lts".to_string());
        assert_eq!(suite_list[0].mv_exclude_tags, Some("last-lts".to_string()));
        assert!(suite_list[0]
            .test_list
            .iter()
            .all(|test| test_list.contains(test)));
        assert_eq!(suite_list[1].name, "suite1_last_continuous".to_string());
        assert_eq!(
            suite_list[1].mv_exclude_tags,
            Some("last-continuous".to_string())
        );
        assert!(suite_list[1]
            .test_list
            .iter()
            .all(|test| test_list.contains(test)));
    }
    // generate_resmoke_task tests.
    #[tokio::test]
    async fn test_generate_resmoke_tasks_standard() {
        // In this test we will create 3 subtasks with 6 tests. The first sub task should contain
        // a single test. The second 2 tests and the third 3 tests. We will set the test runtimes
        // to make this happen.
        let num_tasks = 3;
        let test_list: Vec<String> = (0..6)
            .into_iter()
            .map(|i| format!("test_{}.js", i))
            .collect();
        let task_history = TaskRuntimeHistory {
            task_name: "my_task".to_string(),
            test_map: hashmap! {
                "test_0".to_string() => build_mock_test_runtime("test_0.js", 100.0),
                "test_1".to_string() => build_mock_test_runtime("test_1.js", 50.0),
                "test_2".to_string() => build_mock_test_runtime("test_2.js", 50.0),
                "test_3".to_string() => build_mock_test_runtime("test_3.js", 34.0),
                "test_4".to_string() => build_mock_test_runtime("test_4.js", 34.0),
                "test_5".to_string() => build_mock_test_runtime("test_5.js", 34.0),
            },
        };
        let gen_resmoke_service = build_mocked_service(test_list, task_history.clone());
        let params = ResmokeGenParams {
            task_name: "my_task".to_string(),
            require_multiversion_generate_tasks: false,
            num_tasks: Some(num_tasks),
            ..Default::default()
        };

        let suite = gen_resmoke_service
            .generate_resmoke_task(
                &params,
                &BuildVariant {
                    display_name: Some("build-variant".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(suite.display_name(), "my_task".to_string());
        assert_eq!(suite.sub_tasks().len(), num_tasks);
    }

    #[tokio::test]
    async fn test_generate_resmoke_tasks_required_variant_large() {
        // Creating tasks based off of a required build variant with large total test runtime,
        // more than the default number of tasks should be used.

        let test_list: Vec<String> = (0..8)
            .into_iter()
            .map(|i| format!("test_{}.js", i))
            .collect();
        let task_history = TaskRuntimeHistory {
            task_name: "my_task".to_string(),
            test_map: hashmap! {
                "test_0".to_string() => build_mock_test_runtime("test_0.js", 1800.0),
                "test_1".to_string() => build_mock_test_runtime("test_1.js", 1800.0),
                "test_2".to_string() => build_mock_test_runtime("test_2.js", 1800.0),
                "test_4".to_string() => build_mock_test_runtime("test_4.js", 1800.0),
                "test_5".to_string() => build_mock_test_runtime("test_5.js", 1800.0),
                "test_6".to_string() => build_mock_test_runtime("test_6.js", 1800.0),
                "test_7".to_string() => build_mock_test_runtime("test_7.js", 1800.0),
                "test_8".to_string() => build_mock_test_runtime("test_8.js", 1800.0),
            },
        };
        let gen_resmoke_service = build_mocked_service(test_list, task_history.clone());

        let params = ResmokeGenParams {
            task_name: "my_task".to_string(),
            require_multiversion_generate_tasks: false,
            ..Default::default()
        };

        let suite = gen_resmoke_service
            .generate_resmoke_task(
                &params,
                &BuildVariant {
                    display_name: Some("! required build variant".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(suite.display_name(), "my_task".to_string());
        assert_eq!(suite.sub_tasks().len(), 6);
    }

    #[tokio::test]
    async fn test_generate_resmoke_tasks_required_variant_medium() {
        // Creating tasks based off of a required build variant with large total test runtime,
        // more than the default number of tasks should be used.

        let test_list: Vec<String> = (0..8)
            .into_iter()
            .map(|i| format!("test_{}.js", i))
            .collect();
        let task_history = TaskRuntimeHistory {
            task_name: "my_task".to_string(),
            test_map: hashmap! {
                "test_0".to_string() => build_mock_test_runtime("test_0.js", 900.0),
                "test_1".to_string() => build_mock_test_runtime("test_1.js", 900.0),
                "test_2".to_string() => build_mock_test_runtime("test_2.js", 900.0),
                "test_3".to_string() => build_mock_test_runtime("test_3.js", 900.0),
                "test_4".to_string() => build_mock_test_runtime("test_4.js", 900.0),
                "test_5".to_string() => build_mock_test_runtime("test_5.js", 900.0),
                "test_6".to_string() => build_mock_test_runtime("test_6.js", 900.0),
                "test_7".to_string() => build_mock_test_runtime("test_7.js", 900.0),
                "test_8".to_string() => build_mock_test_runtime("test_8.js", 900.0),
            },
        };
        let gen_resmoke_service = build_mocked_service(test_list, task_history.clone());

        let params = ResmokeGenParams {
            task_name: "my_task".to_string(),
            require_multiversion_generate_tasks: false,
            ..Default::default()
        };

        let suite = gen_resmoke_service
            .generate_resmoke_task(
                &params,
                &BuildVariant {
                    display_name: Some("! required build variant".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(suite.display_name(), "my_task".to_string());
        assert_eq!(suite.sub_tasks().len(), 5);
    }

    #[tokio::test]
    async fn test_generate_resmoke_tasks_required_variant_small() {
        // Creating tasks based off of a required build variant with small test runtime,
        // the default number of subtasks should be used.

        let test_list: Vec<String> = (0..9)
            .into_iter()
            .map(|i| format!("test_{}.js", i))
            .collect();
        let task_history = TaskRuntimeHistory {
            task_name: "my_task".to_string(),
            test_map: hashmap! {
                "test_0".to_string() => build_mock_test_runtime("test_0.js", 1.0),
                "test_1".to_string() => build_mock_test_runtime("test_1.js", 1.0),
                "test_2".to_string() => build_mock_test_runtime("test_2.js", 1.0),
                "test_3".to_string() => build_mock_test_runtime("test_3.js", 1.0),
                "test_4".to_string() => build_mock_test_runtime("test_4.js", 1.0),
                "test_5".to_string() => build_mock_test_runtime("test_5.js", 1.0),
                "test_6".to_string() => build_mock_test_runtime("test_6.js", 1.0),
                "test_7".to_string() => build_mock_test_runtime("test_7.js", 1.0),
                "test_8".to_string() => build_mock_test_runtime("test_8.js", 1.0),
            },
        };
        let gen_resmoke_service = build_mocked_service(test_list, task_history.clone());

        let params = ResmokeGenParams {
            task_name: "my_task".to_string(),
            require_multiversion_generate_tasks: false,
            ..Default::default()
        };

        let suite = gen_resmoke_service
            .generate_resmoke_task(
                &params,
                &BuildVariant {
                    display_name: Some("! required build variant".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(suite.display_name(), "my_task".to_string());
        assert_eq!(suite.sub_tasks().len(), 5);
    }

    #[tokio::test]
    async fn test_generate_resmoke_tasks_multiversion_success() {
        let num_tasks = 3;
        let test_list = vec![
            "test_0.js".to_string(),
            "test_1.js".to_string(),
            "test_2.js".to_string(),
            "test_3.js".to_string(),
        ];
        let task_history = TaskRuntimeHistory {
            task_name: "my_task".to_string(),
            test_map: hashmap! {},
        };
        let gen_resmoke_service = build_mocked_service(test_list, task_history.clone());
        let generate_tasks = vec![
            MultiversionGenerateTaskConfig {
                suite_name: "suite1_last_lts".to_string(),
                old_version: "last-lts".to_string(),
            },
            MultiversionGenerateTaskConfig {
                suite_name: "suite1_last_continuous".to_string(),
                old_version: "last-continuous".to_string(),
            },
        ];
        let params = ResmokeGenParams {
            task_name: "my_task".to_string(),
            multiversion_generate_tasks: Some(generate_tasks.clone()),
            require_multiversion_generate_tasks: true,
            num_tasks: Some(num_tasks),
            ..Default::default()
        };

        let suite = gen_resmoke_service
            .generate_resmoke_task(
                &params,
                &BuildVariant {
                    display_name: Some("build-variant".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(suite.display_name(), "my_task".to_string());
        assert_eq!(suite.sub_tasks().len(), num_tasks * generate_tasks.len());
    }
    #[tokio::test]
    #[should_panic]
    async fn test_generate_resmoke_tasks_multiversion_fail() {
        let num_tasks = Some(3);
        let test_list = vec![
            "test_0.js".to_string(),
            "test_1.js".to_string(),
            "test_2.js".to_string(),
            "test_3.js".to_string(),
        ];
        let task_history = TaskRuntimeHistory {
            task_name: "my_task".to_string(),
            test_map: hashmap! {},
        };
        let gen_resmoke_service = build_mocked_service(test_list, task_history.clone());
        let params = ResmokeGenParams {
            task_name: "my_task".to_string(),
            multiversion_generate_tasks: None,
            require_multiversion_generate_tasks: true,
            num_tasks,
            ..Default::default()
        };

        gen_resmoke_service
            .generate_resmoke_task(
                &params,
                &BuildVariant {
                    display_name: Some("build-variant".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }
    // resmoke_commands tests.
    fn get_evg_fn_name(evg_command: &EvgCommand) -> Option<&str> {
        if let EvgCommand::Function(func) = evg_command {
            Some(&func.func)
        } else {
            None
        }
    }
    #[test]
    fn test_resmoke_commands() {
        let commands = resmoke_commands("run test", hashmap! {}, false);
        assert_eq!(commands.len(), 3);
        assert_eq!(get_evg_fn_name(&commands[0]), Some("do setup"));
        assert_eq!(get_evg_fn_name(&commands[2]), Some("run test"));
    }
    #[test]
    fn test_resmoke_commands_should_include_multiversion() {
        let commands = resmoke_commands("run test", hashmap! {}, true);
        assert_eq!(commands.len(), 6);
        assert_eq!(get_evg_fn_name(&commands[2]), Some("do setup"));
        assert_eq!(get_evg_fn_name(&commands[4]), Some("do multiversion setup"));
        assert_eq!(get_evg_fn_name(&commands[5]), Some("run test"));
    }
    // sort_tests_by_runtime tests.
    #[rstest]
    #[case(vec![100.0, 50.0, 30.0, 25.0, 20.0, 15.0], vec![0, 1, 2, 3, 4, 5])]
    #[case(vec![15.0, 20.0, 25.0, 30.0, 50.0, 100.0], vec![5, 4, 3, 2, 1, 0])]
    #[case(vec![15.0, 50.0, 25.0, 30.0, 20.0, 100.0], vec![5, 1, 3, 2, 4, 0])]
    #[case(vec![100.0, 50.0, 30.0], vec![0, 1, 2, 3, 4, 5])]
    #[case(vec![30.0, 50.0, 100.0], vec![2, 1, 0, 3, 4, 5])]
    #[case(vec![30.0, 100.0, 50.0], vec![1, 2, 0, 3, 4, 5])]
    #[case(vec![], vec![0, 1, 2, 3, 4, 5])]
    fn test_sort_tests_by_runtime(
        #[case] historic_runtimes: Vec<f64>,
        #[case] sorted_indexes: Vec<i32>,
    ) {
        let test_list: Vec<String> = (0..sorted_indexes.len())
            .into_iter()
            .map(|i| format!("test_{}.js", i))
            .collect();
        let task_stats = TaskRuntimeHistory {
            task_name: "my_task".to_string(),
            test_map: (0..historic_runtimes.len())
                .into_iter()
                .map(|i| {
                    (
                        format!("test_{}", i),
                        build_mock_test_runtime(
                            format!("test_{}.js", i).as_ref(),
                            historic_runtimes[i],
                        ),
                    )
                })
                .collect::<HashMap<_, _>>(),
        };
        let expected_result: Vec<String> = (0..sorted_indexes.len())
            .into_iter()
            .map(|i| format!("test_{}.js", sorted_indexes[i]))
            .collect();
        let result = sort_tests_by_runtime(test_list, &task_stats);
        assert_eq!(result, expected_result);
    }
    // get_min_index tests.
    #[rstest]
    #[case(vec![100.0, 50.0, 30.0, 25.0, 20.0, 15.0], 5)]
    #[case(vec![15.0, 20.0, 25.0, 30.0, 50.0, 100.0], 0)]
    #[case(vec![25.0, 50.0, 15.0, 30.0, 100.0, 20.0], 2)]
    fn test_get_min_index(#[case] running_runtimes: Vec<f64>, #[case] expected_min_idx: usize) {
        let min_idx = get_min_index(&running_runtimes);
        assert_eq!(min_idx, expected_min_idx);
    }
}
