//! Entry point into the task generation logic.
//!
//! This code will go through the entire evergreen configuration and create task definitions
//! for any tasks that need to be generated. It will then add references to those generated
//! tasks to any build variants to expect to run them.
#![cfg_attr(feature = "strict", deny(missing_docs))]

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{Arc, Mutex},
};

use anyhow::{bail, Result};
use async_trait::async_trait;
use evergreen::{
    evg_config::{EvgConfigService, EvgProjectConfig},
    evg_config_utils::{EvgConfigUtils, EvgConfigUtilsImpl},
    evg_task_history::TaskHistoryServiceImpl,
};
use evergreen_names::{
    CONTINUE_ON_FAILURE, FUZZER_PARAMETERS, GENERATOR_TASKS, IDLE_TIMEOUT, LARGE_DISTRO_EXPANSION,
    MULTIVERSION, NO_MULTIVERSION_ITERATION, NPM_COMMAND, NUM_FUZZER_FILES, NUM_FUZZER_TASKS,
    REPEAT_SUITES, RESMOKE_ARGS, RESMOKE_JOBS_MAX, SHOULD_SHUFFLE_TESTS, USE_LARGE_DISTRO,
};
use evg_api_rs::EvgClient;
use resmoke::resmoke_proxy::ResmokeProxy;
use shrub_rs::models::{
    project::EvgProject,
    task::{EvgTask, TaskRef},
    variant::{BuildVariant, DisplayTask},
};
use task_types::{
    fuzzer_tasks::{FuzzerGenTaskParams, GenFuzzerService, GenFuzzerServiceImpl},
    generated_suite::GeneratedSuite,
    multiversion::MultiversionServiceImpl,
    resmoke_config_writer::{ResmokeConfigActor, ResmokeConfigActorService},
    resmoke_tasks::{GenResmokeTaskService, GenResmokeTaskServiceImpl, ResmokeGenParams},
};
use tracing::{event, Level};
use utils::{fs_service::FsServiceImpl, task_name::remove_gen_suffix};

mod evergreen;
mod evergreen_names;
mod resmoke;
mod task_types;
mod utils;

/// Directory to store the generated configuration in.
const HISTORY_LOOKBACK_DAYS: u64 = 14;
const MAX_SUB_TASKS_PER_TASK: usize = 5;

type GenTaskCollection = HashMap<String, Box<dyn GeneratedSuite>>;

/// Collection of services needed to execution.
#[derive(Clone)]
pub struct Dependencies {
    gen_task_service: Arc<dyn GenerateTasksService>,
    resmoke_config_actor: Arc<tokio::sync::Mutex<dyn ResmokeConfigActor>>,
}

impl Dependencies {
    /// Create a new set of dependency instances.
    ///
    /// # Arguments
    ///
    /// * `evg_project_location` - Path to the evergreen project configuration yaml.
    /// * `evg_project` - Evergreen project being run.
    /// * `evg_auth_file` - Path to evergreen API auth file.
    /// * `use_task_split_fallback` - Disable evergreen task-history queries and use task
    ///    splitting fallback.
    /// * `resmoke_command` - Command to execute resmoke.
    /// * `target_directory` - Directory to store generated configuration.
    ///
    /// # Returns
    ///
    /// A set of dependencies to run against.
    pub fn new(
        evg_project_location: &Path,
        evg_project: &str,
        evg_auth_file: &Path,
        use_task_split_fallback: bool,
        resmoke_command: &str,
        target_directory: &Path,
    ) -> Result<Self> {
        let fs_service = Arc::new(FsServiceImpl::new());
        let discovery_service = Arc::new(ResmokeProxy::new(resmoke_command));
        let multiversion_service =
            Arc::new(MultiversionServiceImpl::new(discovery_service.clone())?);
        let evg_config_service = Arc::new(
            EvgProjectConfig::new(evg_project_location).expect("Could not find evg project"),
        );
        let evg_config_utils = Arc::new(EvgConfigUtilsImpl::new());
        let gen_fuzzer_service = Arc::new(GenFuzzerServiceImpl::new(multiversion_service.clone()));
        let evg_client =
            Arc::new(EvgClient::from_file(evg_auth_file).expect("Cannot find evergreen auth file"));
        let task_history_service = Arc::new(TaskHistoryServiceImpl::new(
            evg_client,
            HISTORY_LOOKBACK_DAYS,
            evg_project.to_string(),
        ));
        let resmoke_config_actor =
            Arc::new(tokio::sync::Mutex::new(ResmokeConfigActorService::new(
                discovery_service.clone(),
                multiversion_service.clone(),
                fs_service.clone(),
                target_directory
                    .to_str()
                    .expect("Unexpected target directory"),
                32,
            )));
        let gen_resmoke_task_service = Arc::new(GenResmokeTaskServiceImpl::new(
            task_history_service,
            discovery_service,
            resmoke_config_actor.clone(),
            multiversion_service,
            fs_service,
            MAX_SUB_TASKS_PER_TASK,
            use_task_split_fallback,
        ));
        let gen_task_service = Arc::new(GenerateTasksServiceImpl::new(
            evg_config_service,
            evg_config_utils,
            gen_fuzzer_service,
            gen_resmoke_task_service,
        ));

        Ok(Self {
            gen_task_service,
            resmoke_config_actor,
        })
    }
}

/// A container for configuration generated for a build variant.
#[derive(Debug, Clone)]
struct GeneratedConfig {
    /// References to generated tasks that should be included.
    pub gen_task_specs: Vec<TaskRef>,
    /// Display tasks that should be created.
    pub display_tasks: Vec<DisplayTask>,
}

impl GeneratedConfig {
    /// Create an empty instance of generated configuration.
    pub fn new() -> Self {
        Self {
            gen_task_specs: vec![],
            display_tasks: vec![],
        }
    }
}

/// Create 'generate.tasks' configuration for all generated tasks in the provided evergreen
/// project configuration.
///
/// # Arguments
///
/// * `deps` - Dependencies needed to perform generation.
/// * `config_location` - Pointer to S3 location that configuration will be uploaded to.
/// * `target_directory` - Directory to store generated configuration.
pub async fn generate_configuration(
    deps: &Dependencies,
    config_location: &str,
    target_directory: &Path,
) -> Result<()> {
    let generate_tasks_service = deps.gen_task_service.clone();
    std::fs::create_dir_all(target_directory)?;

    // We are going to do 2 passes through the project build variants. In this first pass, we
    // are actually going to create all the generated tasks that we discover.
    let generated_tasks = generate_tasks_service
        .build_generated_tasks(deps, config_location)
        .await?;

    // Now that we have generated all the tasks we want to make another pass through all the
    // build variants and add references to the generated tasks that each build variant includes.
    let generated_build_variants =
        generate_tasks_service.generate_build_variants(generated_tasks.clone());

    let generated_tasks = generated_tasks.lock().unwrap();
    let task_defs: Vec<EvgTask> = generated_tasks
        .values()
        .flat_map(|g| g.sub_tasks())
        .collect();

    let gen_evg_project = EvgProject {
        buildvariants: generated_build_variants.to_vec(),
        tasks: task_defs,
        ..Default::default()
    };

    let mut config_file = target_directory.to_path_buf();
    config_file.push("evergreen_config.json");
    std::fs::write(config_file, serde_json::to_string_pretty(&gen_evg_project)?)?;
    let mut resmoke_config_actor = deps.resmoke_config_actor.lock().await;
    let failures = resmoke_config_actor.flush().await?;
    if !failures.is_empty() {
        bail!(format!(
            "Encountered errors writing resmoke configuration files: {:?}",
            failures
        ));
    }
    Ok(())
}

/// A service for generating tasks.
#[async_trait]
trait GenerateTasksService: Sync + Send {
    /// Build task definition for all tasks
    ///
    /// # Arguments
    ///
    /// * `config_location` - Location in S3 where generated configuration will be stored.
    ///
    /// Returns
    ///
    /// Map of task names to generated task definitions.
    async fn build_generated_tasks(
        &self,
        deps: &Dependencies,
        config_location: &str,
    ) -> Result<Arc<Mutex<GenTaskCollection>>>;

    /// Create build variants definitions containing all the generated tasks for each build variant.
    ///
    /// # Arguments
    ///
    /// * `generated_tasks` - Map of task names and their generated configuration.
    ///
    /// # Returns
    ///
    /// Vector of shrub build variants with generated task information.
    fn generate_build_variants(
        &self,
        generated_tasks: Arc<Mutex<GenTaskCollection>>,
    ) -> Vec<BuildVariant>;

    /// Build the configuration for generated a fuzzer based on the evergreen task definition.
    ///
    /// # Arguments
    ///
    /// * `task-def` - Task definition of fuzzer to generate.
    /// * `build_variant` -
    /// * `config_location` - Location where generated configuration will be stored in S3.
    ///
    /// # Returns
    ///
    /// Parameters to configure how fuzzer task should be generated.
    fn task_def_to_fuzzer_params(
        &self,
        task_def: &EvgTask,
        build_variant: &BuildVariant,
        config_location: &str,
    ) -> Result<FuzzerGenTaskParams>;

    /// Generate a task for the given task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition to base generated task on.
    /// * `build_variant` - Build Variant to base generated task on.
    /// * `config_location` - Location where generated task configuration will be stored.
    ///
    /// # Returns
    ///
    /// Configuration for a generated task.
    async fn generate_task(
        &self,
        task_def: &EvgTask,
        build_variant: &BuildVariant,
        config_location: &str,
    ) -> Result<Option<Box<dyn GeneratedSuite>>>;

    /// Build the configuration for generated a resmoke based on the evergreen task definition.
    ///
    /// # Arguments
    ///
    /// * `task-def` - Task definition of task to generate.
    /// * `config_location` - Location where generated configuration will be stored in S3.
    ///
    /// # Returns
    ///
    /// Parameters to configure how resmoke task should be generated.
    fn task_def_to_resmoke_params(
        &self,
        task_def: &EvgTask,
        config_location: &str,
    ) -> Result<ResmokeGenParams>;
}

struct GenerateTasksServiceImpl {
    evg_config_service: Arc<dyn EvgConfigService>,
    evg_config_utils: Arc<dyn EvgConfigUtils>,
    gen_fuzzer_service: Arc<dyn GenFuzzerService>,
    gen_resmoke_service: Arc<dyn GenResmokeTaskService>,
}

impl GenerateTasksServiceImpl {
    /// Create an instance of GenerateTasksServiceImpl.
    ///
    /// # Arguments
    ///
    /// * `evg_config_service` - Service to work with evergreen project configuration.
    /// * `evg_config_utils` - Utilities to work with evergreen project configuration.
    /// * `gen_fuzzer_service` - Service to generate fuzzer tasks.
    pub fn new(
        evg_config_service: Arc<dyn EvgConfigService>,
        evg_config_utils: Arc<dyn EvgConfigUtils>,
        gen_fuzzer_service: Arc<dyn GenFuzzerService>,
        gen_resmoke_service: Arc<dyn GenResmokeTaskService>,
    ) -> Self {
        Self {
            evg_config_service,
            evg_config_utils,
            gen_fuzzer_service,
            gen_resmoke_service,
        }
    }
}

/// An implementation of GeneratorTasksService.
#[async_trait]
impl GenerateTasksService for GenerateTasksServiceImpl {
    /// Build task definition for all tasks
    ///
    /// # Arguments
    ///
    /// * `config_location` - Location in S3 where generated configuration will be stored.
    ///
    /// Returns
    ///
    /// Map of task names to generated task definitions.
    async fn build_generated_tasks(
        &self,
        deps: &Dependencies,
        config_location: &str,
    ) -> Result<Arc<Mutex<GenTaskCollection>>> {
        let build_variant_list = self.evg_config_service.sort_build_variants_by_required();
        let build_variant_map = self.evg_config_service.get_build_variant_map();
        let task_map = self.evg_config_service.get_task_def_map();

        let mut thread_handles = vec![];

        let generated_tasks = Arc::new(Mutex::new(HashMap::new()));
        let mut seen_tasks = HashSet::new();
        for build_variant in &build_variant_list {
            let build_variant = build_variant_map.get(build_variant).unwrap();
            for task in &build_variant.tasks {
                // Skip tasks that have already been seen.
                if seen_tasks.contains(&task.name) {
                    continue;
                }

                seen_tasks.insert(task.name.to_string());
                if let Some(task_def) = task_map.get(&task.name) {
                    if self.evg_config_utils.is_task_generated(task_def) {
                        // Spawn off a tokio task to do the actual generation work.
                        thread_handles.push(create_task_worker(
                            deps,
                            task_def,
                            build_variant,
                            config_location,
                            generated_tasks.clone(),
                        ));
                    }
                }
            }
        }

        for handle in thread_handles {
            handle.await.unwrap();
        }

        Ok(generated_tasks)
    }

    /// Generate a task for the given task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition to base generated task on.
    /// * `build_variant` - Build Variant to base generated task on.
    /// * `config_location` - Location where generated task configuration will be stored.
    ///
    /// # Returns
    ///
    /// Configuration for a generated task.
    async fn generate_task(
        &self,
        task_def: &EvgTask,
        build_variant: &BuildVariant,
        config_location: &str,
    ) -> Result<Option<Box<dyn GeneratedSuite>>> {
        let generated_task = if self.evg_config_utils.is_task_fuzzer(task_def) {
            event!(Level::INFO, "Generating fuzzer: {}", task_def.name);

            let params =
                self.task_def_to_fuzzer_params(task_def, build_variant, config_location)?;

            Some(self.gen_fuzzer_service.generate_fuzzer_task(&params)?)
        } else {
            event!(Level::INFO, "Generating resmoke task: {}", task_def.name);
            let params = self.task_def_to_resmoke_params(task_def, config_location)?;
            Some(
                self.gen_resmoke_service
                    .generate_resmoke_task(&params, &build_variant.name)
                    .await?,
            )
        };

        Ok(generated_task)
    }

    /// Create build variants definitions containing all the generated tasks for each build variant.
    ///
    /// # Arguments
    ///
    /// * `generated_tasks` - Map of task names and their generated configuration.
    ///
    /// # Returns
    ///
    /// Vector of shrub build variants with generated task information.
    fn generate_build_variants(
        &self,
        generated_tasks: Arc<Mutex<GenTaskCollection>>,
    ) -> Vec<BuildVariant> {
        let mut generated_build_variants = vec![];

        let build_variant_map = self.evg_config_service.get_build_variant_map();
        for (_bv_name, build_variant) in build_variant_map {
            let mut gen_config = GeneratedConfig::new();
            let mut generating_tasks = vec![];
            let large_distro_name = self
                .evg_config_utils
                .lookup_build_variant_expansion(LARGE_DISTRO_EXPANSION, build_variant);
            for task in &build_variant.tasks {
                let generated_tasks = generated_tasks.lock().unwrap();
                if let Some(generated_task) = generated_tasks.get(&task.name) {
                    let distro = if generated_task.use_large_distro() {
                        large_distro_name.clone()
                    } else {
                        None
                    };

                    generating_tasks.push(&task.name);
                    gen_config
                        .display_tasks
                        .push(generated_task.build_display_task());
                    gen_config
                        .gen_task_specs
                        .extend(generated_task.build_task_ref(distro));
                }
            }

            if !generating_tasks.is_empty() {
                // Put all the "_gen" tasks into a display task to hide them from view.
                gen_config.display_tasks.push(DisplayTask {
                    name: GENERATOR_TASKS.to_string(),
                    execution_tasks: generating_tasks
                        .into_iter()
                        .map(|s| s.to_string())
                        .collect(),
                });

                let gen_build_variant = BuildVariant {
                    name: build_variant.name.clone(),
                    tasks: gen_config.gen_task_specs.clone(),
                    display_tasks: Some(gen_config.display_tasks.clone()),
                    activate: Some(false),
                    ..Default::default()
                };
                generated_build_variants.push(gen_build_variant);
            }
        }

        generated_build_variants
    }

    /// Build the configuration for generated a fuzzer based on the evergreen task definition.
    ///
    /// # Arguments
    ///
    /// * `evg_config_utils` -
    /// * `task-def` - Task definition of fuzzer to generate.
    /// * `build_variant` -
    /// * `config_location` - Location where generated configuration will be stored in S3.
    ///
    /// # Returns
    ///
    /// Parameters to configure how fuzzer task should be generated.
    fn task_def_to_fuzzer_params(
        &self,
        task_def: &EvgTask,
        build_variant: &BuildVariant,
        config_location: &str,
    ) -> Result<FuzzerGenTaskParams> {
        let evg_config_utils = self.evg_config_utils.clone();
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
            config_location: config_location.to_string(),
        })
    }

    /// Build the configuration for generated a resmoke based on the evergreen task definition.
    ///
    /// # Arguments
    ///
    /// * `task-def` - Task definition of task to generate.
    /// * `config_location` - Location where generated configuration will be stored in S3.
    ///
    /// # Returns
    ///
    /// Parameters to configure how resmoke task should be generated.
    fn task_def_to_resmoke_params(
        &self,
        task_def: &EvgTask,
        config_location: &str,
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
            config_location: config_location.to_string(),
        })
    }
}

/// Spawn a tokio task to perform the task generation work.
///
/// # Arguments
///
/// * `deps` - Service dependencies.
/// * `task_def` - Evergreen task definition to base generated task off.
/// * `build_variant` - Build variant to query timing information from.
/// * `config_location` - S3 location where generated config will be stored.
/// * `generated_tasks` - Map to stored generated to in.
///
/// # Returns
///
/// Handle to created tokio worker.
fn create_task_worker(
    deps: &Dependencies,
    task_def: &EvgTask,
    build_variant: &BuildVariant,
    config_location: &str,
    generated_tasks: Arc<Mutex<GenTaskCollection>>,
) -> tokio::task::JoinHandle<()> {
    let generate_task_service = deps.gen_task_service.clone();
    let task_def = task_def.clone();
    let build_variant = build_variant.clone();
    let config_location = config_location.to_string();
    let generated_tasks = generated_tasks.clone();

    tokio::spawn(async move {
        let generated_task = generate_task_service
            .generate_task(&task_def, &build_variant, &config_location)
            .await
            .unwrap();

        if let Some(generated_task) = generated_task {
            let mut generated_tasks = generated_tasks.lock().unwrap();
            generated_tasks.insert(task_def.name.clone(), generated_task);
        }
    })
}
