//! Service for generating resmoke tasks.
//!
//! This service will query the historic runtime of tests in the given task and then
//! use that information to divide the tests into sub-suites that can be run in parallel.
//!
//! Each task will contain the generated sub-suites and a '_misc' suite. The '_misc' suite
//! tries to run all the tests for the original suite minus tests that were added to generated
//! suites. This catches tests that were not included in the historic runtime data. For example,
//! newly added tests that have not yet be run.
use std::{cmp::min, collections::HashMap, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use maplit::hashmap;
use shrub_rs::models::{
    commands::{fn_call, fn_call_with_params, EvgCommand},
    params::ParamValue,
    task::EvgTask,
};
use tokio::sync::Mutex;
use tracing::{event, warn, Level};

use crate::{
    evergreen::evg_task_history::{get_test_name, TaskHistoryService, TaskRuntimeHistory},
    evergreen_names::{
        ADD_GIT_TAG, CONFIGURE_EVG_API_CREDS, DO_MULTIVERSION_SETUP, DO_SETUP,
        GEN_TASK_CONFIG_LOCATION, GET_PROJECT_WITH_NO_MODULES, REQUIRE_MULTIVERSION_SETUP,
        RESMOKE_ARGS, RESMOKE_JOBS_MAX, RUN_GENERATED_TESTS, SUITE_NAME,
    },
    resmoke::resmoke_proxy::TestDiscovery,
    utils::{fs_service::FsService, task_name::name_generated_task},
};

use super::{
    generated_suite::GeneratedSuite, multiversion::MultiversionService,
    resmoke_config_writer::ResmokeConfigActor,
};

/// Parameters describing how a specific resmoke suite should be generated.
#[derive(Clone, Debug, Default)]
pub struct ResmokeGenParams {
    /// Name of task being generated.
    pub task_name: String,
    /// Name of suite being generated.
    pub suite_name: String,
    /// Should the generated tasks run on a 'large' distro.
    pub use_large_distro: bool,
    /// Does this task require multiversion setup.
    pub require_multiversion_setup: bool,
    /// Should multiversion combinations be used for this task.
    pub generate_multiversion_combos: bool,
    /// Arguments that should be passed to resmoke.
    pub resmoke_args: String,
    /// Number of jobs to limit resmoke to.
    pub resmoke_jobs_max: Option<u64>,
    /// Location where generated task configuration will be stored in S3.
    pub config_location: String,
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
    fn build_run_test_vars(&self, suite_file: &str) -> HashMap<String, ParamValue> {
        let mut run_test_vars = hashmap! {
            REQUIRE_MULTIVERSION_SETUP.to_string() => ParamValue::from(self.require_multiversion_setup),
            RESMOKE_ARGS.to_string() => ParamValue::from(self.build_resmoke_args().as_str()),
            SUITE_NAME.to_string() => ParamValue::from(format!("generated_resmoke_config/{}.yml", suite_file).as_str()),
            GEN_TASK_CONFIG_LOCATION.to_string() => ParamValue::from(self.config_location.as_str()),
        };

        if let Some(resmoke_jobs_max) = &self.resmoke_jobs_max {
            run_test_vars.insert(
                RESMOKE_JOBS_MAX.to_string(),
                ParamValue::from(*resmoke_jobs_max),
            );
        }

        run_test_vars
    }

    /// Build the resmoke arguments to use for a generate sub-task.
    ///
    /// # Returns
    ///
    /// String of arguments to pass to resmoke.
    fn build_resmoke_args(&self) -> String {
        format!("--originSuite={} {}", self.suite_name, self.resmoke_args)
    }
}

/// Representation of generated sub-suite.
#[derive(Clone, Debug)]
pub struct SubSuite {
    /// Index value of generated suite (None for the '_misc' sub-task).
    pub index: Option<usize>,
    /// Name of generated sub-suite.
    pub name: String,
    /// List of tests belonging to sub-suite.
    pub test_list: Vec<String>,
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

    /// If true, sub-tasks should be generated for multiversion combinations.
    pub generate_multiversion_combos: bool,
}

/// Representation of a generated resmoke suite.
#[derive(Clone, Debug)]
pub struct GeneratedResmokeSuite {
    /// Name of display task to create.
    pub task_name: String,

    /// Sub suites that comprise generated task.
    pub sub_suites: Vec<EvgTask>,

    /// If true, run generated task on a large distro.
    use_large_distro: bool,
}

impl GeneratedSuite for GeneratedResmokeSuite {
    /// Get the display name to use for the generated task.
    fn display_name(&self) -> String {
        self.task_name.clone()
    }

    /// Get the list of sub-tasks that comprise the generated task.
    fn sub_tasks(&self) -> Vec<EvgTask> {
        self.sub_suites.clone()
    }

    // If true, run generated task on a large distro.
    fn use_large_distro(&self) -> bool {
        self.use_large_distro
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
        build_variant: &str,
    ) -> Result<Box<dyn GeneratedSuite>>;
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

    /// Max number of suites to split tasks into.
    n_suites: usize,
}

impl GenResmokeTaskServiceImpl {
    /// Create a new instance of the service implementation.
    ///
    /// # Arguments
    ///
    /// * `task_history_service` - An instance of the service to query task history.
    /// * `test_discovery` - An instance of the service to query tests belonging to a task.
    /// * `fs_service` - An instance of the service too work with the file system.
    /// * `n_suite` - Number of sub-suites to split tasks into.
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
        n_suites: usize,
    ) -> Self {
        Self {
            task_history_service,
            test_discovery,
            resmoke_config_actor,
            multiversion_service,
            fs_service,
            n_suites,
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
    ///
    /// # Returns
    ///
    /// A list of sub-suites to run the tests is the given task.
    fn split_task(
        &self,
        params: &ResmokeGenParams,
        task_stats: &TaskRuntimeHistory,
    ) -> Result<Vec<SubSuite>> {
        let test_list: Vec<String> = self
            .test_discovery
            .discover_tests(&params.suite_name)?
            .into_iter()
            .filter(|s| self.fs_service.file_exists(s))
            .collect();

        let total_runtime = task_stats
            .test_map
            .iter()
            .fold(0.0, |init, (_, item)| init + item.average_runtime);

        let max_tasks = min(self.n_suites, test_list.len());
        let runtime_per_subtask = total_runtime / max_tasks as f64;
        event!(
            Level::INFO,
            "Splitting task: {}, runtime: {}, tests: {}",
            &params.suite_name,
            runtime_per_subtask,
            test_list.len()
        );
        let mut sub_suites = vec![];
        let mut running_tests = vec![];
        let mut running_runtime = 0.0;
        let mut i = 0;
        for test in test_list {
            let test_name = get_test_name(&test);
            if let Some(test_stats) = task_stats.test_map.get(&test_name) {
                if (running_runtime + test_stats.average_runtime > runtime_per_subtask)
                    && !running_tests.is_empty()
                    && sub_suites.len() < max_tasks - 1
                {
                    sub_suites.push(SubSuite {
                        index: Some(i),
                        name: params.task_name.to_string(),
                        test_list: running_tests.clone(),
                    });
                    running_tests = vec![];
                    running_runtime = 0.0;
                    i += 1;
                }
                running_runtime += test_stats.average_runtime;
            }
            running_tests.push(test.clone());
        }
        if !running_tests.is_empty() {
            sub_suites.push(SubSuite {
                index: Some(i),
                name: params.task_name.to_string(),
                test_list: running_tests.clone(),
            });
        }

        Ok(sub_suites)
    }

    /// Split a task with no historic runtime information.
    ///
    /// Since we don't have any runtime information, we will just split the tests evenly among
    /// the number of suites we want to create.
    ///
    /// # Arguments
    ///
    /// * `params` - Parameters for how tasks should be generated.
    ///
    /// # Returns
    ///
    /// A list of sub-suites to run the tests is the given task.
    fn split_task_fallback(&self, params: &ResmokeGenParams) -> Result<Vec<SubSuite>> {
        let test_list: Vec<String> = self
            .test_discovery
            .discover_tests(&params.suite_name)?
            .into_iter()
            .filter(|s| self.fs_service.file_exists(s))
            .collect();

        let n_suites = min(test_list.len(), self.n_suites);
        let tasks_per_suite = test_list.len() / n_suites;

        let mut sub_suites = vec![];
        let mut current_tests = vec![];
        let mut i = 0;
        for test in test_list {
            current_tests.push(test);
            if current_tests.len() >= tasks_per_suite {
                sub_suites.push(SubSuite {
                    index: Some(i),
                    name: params.task_name.to_string(),
                    test_list: current_tests,
                });
                current_tests = vec![];
                i += 1;
            }
        }

        if !current_tests.is_empty() {
            sub_suites.push(SubSuite {
                index: Some(i),
                name: params.task_name.to_string(),
                test_list: current_tests,
            });
        }

        Ok(sub_suites)
    }

    /// Create version of the generated sub-tasks for all the multiversion combinations.
    ///
    /// # Arguments
    ///
    /// * `params` - Parameters for how task should be generated.
    /// * `sub_suites` - Sub-suites that were created for the task being generated.
    ///
    /// # Returns
    ///
    /// List of sub-suites that includes versions fall all multiversion combinations.
    fn create_multiversion_combinations(
        &self,
        params: &ResmokeGenParams,
        sub_suites: &[SubSuite],
    ) -> Result<Vec<SubSuite>> {
        let mut mv_sub_suites = vec![];
        for (old_version, version_combination) in self
            .multiversion_service
            .multiversion_iter(&params.suite_name)?
        {
            for sub_suite in sub_suites {
                let suite = self.multiversion_service.name_multiversion_suite(
                    &sub_suite.name,
                    &old_version,
                    &version_combination,
                );
                mv_sub_suites.push(SubSuite {
                    index: sub_suite.index,
                    name: suite,
                    test_list: sub_suite.test_list.clone(),
                });
            }
            // Add a `_misc` sub-task to the list of tasks.
            mv_sub_suites.push(SubSuite {
                index: None,
                name: self.multiversion_service.name_multiversion_suite(
                    &params.task_name,
                    &old_version,
                    &version_combination,
                ),
                test_list: vec![],
            });
        }

        Ok(mv_sub_suites)
    }
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
        build_variant: &str,
    ) -> Result<Box<dyn GeneratedSuite>> {
        let task_history = self
            .task_history_service
            .get_task_history(&params.task_name, build_variant)
            .await;

        let mut sub_suites = match task_history {
            Ok(task_history) => self.split_task(params, &task_history)?,
            Err(err) => {
                warn!(
                    task_name = params.task_name.as_str(),
                    error = err.to_string().as_str(),
                    "Could not get task history from evergreen",
                );
                // If we couldn't get the task history, then fallback to splitting the tests evenly
                // among the desired number of sub-suites.
                self.split_task_fallback(params)?
            }
        };

        let sub_task_total = sub_suites.len();
        let suite_info = ResmokeSuiteGenerationInfo {
            task_name: params.task_name.to_string(),
            origin_suite: params.suite_name.to_string(),
            sub_suites: sub_suites.clone(),
            generate_multiversion_combos: params.generate_multiversion_combos,
        };
        let mut resmoke_config_actor = self.resmoke_config_actor.lock().await;
        resmoke_config_actor.write_sub_suite(&suite_info).await;

        if params.generate_multiversion_combos {
            sub_suites = self.create_multiversion_combinations(params, &sub_suites)?;
        } else {
            // Add a `_misc` sub-task to the list of tasks.
            sub_suites.push(SubSuite {
                index: None,
                name: params.task_name.to_string(),
                test_list: vec![],
            });
        }

        Ok(Box::new(GeneratedResmokeSuite {
            task_name: params.task_name.clone(),
            sub_suites: sub_suites
                .into_iter()
                .map(|s| build_resmoke_sub_task(&s, sub_task_total, params))
                .collect(),
            use_large_distro: params.use_large_distro,
        }))
    }
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
    sub_suite: &SubSuite,
    total_sub_suites: usize,
    params: &ResmokeGenParams,
) -> EvgTask {
    let suite_file = &name_generated_task(&sub_suite.name, sub_suite.index, total_sub_suites);
    let run_test_vars = params.build_run_test_vars(suite_file);

    EvgTask {
        name: suite_file.to_string(),
        commands: resmoke_commands(
            RUN_GENERATED_TESTS,
            run_test_vars,
            params.require_multiversion_setup,
        ),
        ..Default::default()
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
    use crate::{
        evergreen::evg_task_history::TestRuntimeHistory,
        resmoke::{resmoke_proxy::MultiversionConfig, resmoke_suite::ResmokeSuiteConfig},
        task_types::multiversion::MultiversionIterator,
    };

    use super::*;

    // ResmokeGenParams tests.
    #[test]
    fn test_build_run_test_vars() {
        let params = ResmokeGenParams {
            suite_name: "my_suite".to_string(),
            resmoke_args: "resmoke args".to_string(),
            ..Default::default()
        };

        let test_vars = params.build_run_test_vars("my_suite_0");

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
            ..Default::default()
        };

        let test_vars = params.build_run_test_vars("my_suite_0");

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
    fn test_build_resmoke_args() {
        let params = ResmokeGenParams {
            suite_name: "my_suite".to_string(),
            resmoke_args: "--args to --pass to resmoke".to_string(),
            ..Default::default()
        };

        let resmoke_args = params.build_resmoke_args();

        assert!(resmoke_args.contains("--originSuite=my_suite"));
        assert!(resmoke_args.contains("--args to --pass to resmoke"));
    }

    // split_task tests
    struct MockTaskHistoryService {
        task_history: TaskRuntimeHistory,
    }

    #[async_trait]
    impl TaskHistoryService for MockTaskHistoryService {
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

        async fn flush(&mut self) {}
    }

    struct MockMultiversionService {
        old_version: Vec<String>,
        version_combos: Vec<String>,
    }
    impl MultiversionService for MockMultiversionService {
        fn get_version_combinations(&self, _suite_name: &str) -> Result<Vec<String>> {
            todo!()
        }

        fn multiversion_iter(
            &self,
            _version_combinations: &str,
        ) -> Result<crate::task_types::multiversion::MultiversionIterator> {
            Ok(MultiversionIterator::new(
                &self.old_version,
                &self.version_combos,
            ))
        }

        fn name_multiversion_suite(
            &self,
            base_name: &str,
            old_version: &str,
            version_combination: &str,
        ) -> String {
            format!("{}_{}_{}", base_name, old_version, version_combination)
        }
    }

    fn build_mocked_service(
        test_list: Vec<String>,
        task_history: TaskRuntimeHistory,
        n_suites: usize,
        old_version: Vec<String>,
        version_combos: Vec<String>,
    ) -> GenResmokeTaskServiceImpl {
        let test_discovery = MockTestDiscovery { test_list };
        let multiversion_service = MockMultiversionService {
            old_version,
            version_combos,
        };
        let task_history_service = MockTaskHistoryService {
            task_history: task_history.clone(),
        };
        let fs_service = MockFsService {};
        let resmoke_config_actor = MockResmokeConfigActor {};

        GenResmokeTaskServiceImpl::new(
            Arc::new(task_history_service),
            Arc::new(test_discovery),
            Arc::new(Mutex::new(resmoke_config_actor)),
            Arc::new(multiversion_service),
            Arc::new(fs_service),
            n_suites,
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
        let n_suites = 3;
        let test_list: Vec<String> = (0..6)
            .into_iter()
            .map(|i| format!("test_{}.js", i))
            .collect();
        let task_history = TaskRuntimeHistory {
            task_name: "my task".to_string(),
            test_map: hashmap! {
                "test_0".to_string() => build_mock_test_runtime("test_0.js", 100.0),
                "test_1".to_string() => build_mock_test_runtime("test_1.js", 50.0),
                "test_2".to_string() => build_mock_test_runtime("test_2.js", 50.0),
                "test_3".to_string() => build_mock_test_runtime("test_3.js", 34.0),
                "test_4".to_string() => build_mock_test_runtime("test_4.js", 34.0),
                "test_5".to_string() => build_mock_test_runtime("test_5.js", 34.0),
            },
        };
        let gen_resmoke_service =
            build_mocked_service(test_list, task_history.clone(), n_suites, vec![], vec![]);

        let params = ResmokeGenParams {
            ..Default::default()
        };

        let sub_suites = gen_resmoke_service
            .split_task(&params, &task_history)
            .unwrap();

        assert_eq!(sub_suites.len(), n_suites);
        let suite_0 = &sub_suites[0];
        assert!(suite_0.test_list.contains(&"test_0.js".to_string()));
        let suite_1 = &sub_suites[1];
        assert!(suite_1.test_list.contains(&"test_1.js".to_string()));
        assert!(suite_1.test_list.contains(&"test_2.js".to_string()));
        let suite_2 = &sub_suites[2];
        assert!(suite_2.test_list.contains(&"test_3.js".to_string()));
        assert!(suite_2.test_list.contains(&"test_4.js".to_string()));
        assert!(suite_2.test_list.contains(&"test_5.js".to_string()));
    }

    // split_task_fallback tests

    #[test]
    fn test_split_task_fallback_should_split_tasks_count() {
        let n_suites = 3;
        let test_list: Vec<String> = (0..6)
            .into_iter()
            .map(|i| format!("test_{}.js", i))
            .collect();
        let task_history = TaskRuntimeHistory {
            task_name: "my task".to_string(),
            test_map: hashmap! {},
        };
        let gen_resmoke_service =
            build_mocked_service(test_list, task_history.clone(), n_suites, vec![], vec![]);

        let params = ResmokeGenParams {
            ..Default::default()
        };

        let sub_suites = gen_resmoke_service.split_task_fallback(&params).unwrap();

        assert_eq!(sub_suites.len(), n_suites);
        let suite_0 = &sub_suites[0];
        assert!(suite_0.test_list.contains(&"test_0.js".to_string()));
        assert!(suite_0.test_list.contains(&"test_1.js".to_string()));
        let suite_1 = &sub_suites[1];
        assert!(suite_1.test_list.contains(&"test_2.js".to_string()));
        assert!(suite_1.test_list.contains(&"test_3.js".to_string()));
        let suite_2 = &sub_suites[2];
        assert!(suite_2.test_list.contains(&"test_4.js".to_string()));
        assert!(suite_2.test_list.contains(&"test_5.js".to_string()));
    }

    // create_multiversion_combinations tests.
    #[test]
    fn test_create_multiversion_combinations() {
        let old_version = vec!["last_lts".to_string(), "continuous".to_string()];
        let version_combos = vec!["new_new_new".to_string(), "old_new_old".to_string()];
        let sub_suites = vec![
            SubSuite {
                index: Some(0),
                name: "suite".to_string(),
                test_list: vec!["test_0.js".to_string(), "test_1.js".to_string()],
            },
            SubSuite {
                index: Some(1),
                name: "suite".to_string(),
                test_list: vec!["test_2.js".to_string(), "test_3.js".to_string()],
            },
        ];
        let params = ResmokeGenParams {
            ..Default::default()
        };
        let task_history = TaskRuntimeHistory {
            task_name: "my task".to_string(),
            test_map: hashmap! {},
        };
        let gen_resmoke_service = build_mocked_service(
            vec![],
            task_history,
            1,
            old_version.clone(),
            version_combos.clone(),
        );

        let suite_list = gen_resmoke_service
            .create_multiversion_combinations(&params, &sub_suites)
            .unwrap();

        for version in old_version {
            for combo in &version_combos {
                for sub_suite in &sub_suites {
                    let sub_task_name = format!("{}_{}_{}", &sub_suite.name, version, combo);
                    let suite = suite_list.iter().find(|s| s.name == sub_task_name);
                    assert!(suite.is_some());
                }
            }
        }
    }

    // generate_resmoke_task tests.
    #[tokio::test]
    async fn test_generate_resmoke_tasks() {
        // In this test we will create 3 subtasks with 6 tests. The first sub task should contain
        // a single test. The second 2 tests and the third 3 tests. We will set the test runtimes
        // to make this happen.
        let n_suites = 3;
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
        let gen_resmoke_service =
            build_mocked_service(test_list, task_history.clone(), n_suites, vec![], vec![]);

        let params = ResmokeGenParams {
            task_name: "my_task".to_string(),
            ..Default::default()
        };

        let suite = gen_resmoke_service
            .generate_resmoke_task(&params, "build-variant")
            .await
            .unwrap();

        assert_eq!(suite.display_name(), "my_task".to_string());
        assert_eq!(suite.sub_tasks().len(), n_suites + 1); // +1 for _misc suite.
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
}
