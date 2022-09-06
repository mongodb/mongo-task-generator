//! Entry point into the task generation logic.
//!
//! This code will go through the entire evergreen configuration and create task definitions
//! for any tasks that need to be generated. It will then add references to those generated
//! tasks to any build variants to expect to run them.
#![cfg_attr(feature = "strict", deny(missing_docs))]

use core::panic;
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    vec,
};

use anyhow::{bail, Result};
use async_trait::async_trait;
use evergreen::{
    evg_config::{EvgConfigService, EvgProjectConfig},
    evg_config_utils::{EvgConfigUtils, EvgConfigUtilsImpl},
    evg_task_history::TaskHistoryServiceImpl,
};
use evergreen_names::{
    BURN_IN_TAGS, BURN_IN_TAG_BUILD_VARIANTS, BURN_IN_TAG_COMPILE_DISTRO,
    BURN_IN_TAG_COMPILE_TASK_GROUP_NAME, BURN_IN_TESTS, ENTERPRISE_MODULE, GENERATOR_TASKS,
    LARGE_DISTRO_EXPANSION,
};
use evg_api_rs::EvgClient;
use generate_sub_tasks_config::GenerateSubTasksConfig;
use resmoke::{burn_in_proxy::BurnInProxy, resmoke_proxy::ResmokeProxy};
use services::config_extraction::{ConfigExtractionService, ConfigExtractionServiceImpl};
use shrub_rs::models::{
    project::EvgProject,
    task::{EvgTask, TaskRef},
    variant::{BuildVariant, DisplayTask},
};
use task_types::{
    burn_in_tests::{BurnInService, BurnInServiceImpl},
    fuzzer_tasks::{GenFuzzerService, GenFuzzerServiceImpl},
    generated_suite::GeneratedSuite,
    multiversion::MultiversionServiceImpl,
    resmoke_config_writer::{ResmokeConfigActor, ResmokeConfigActorService},
    resmoke_tasks::{GenResmokeConfig, GenResmokeTaskService, GenResmokeTaskServiceImpl},
};
use tracing::{event, Level};
use utils::fs_service::FsServiceImpl;

mod evergreen;
mod evergreen_names;
mod generate_sub_tasks_config;
mod resmoke;
mod services;
mod task_types;
mod utils;

const BURN_IN_PREFIX: &str = "burn_in_tests";
const HISTORY_LOOKBACK_DAYS: u64 = 14;
const MAX_SUB_TASKS_PER_TASK: usize = 5;

type GenTaskCollection = HashMap<String, Box<dyn GeneratedSuite>>;

pub struct BurnInTagBuildVariantInfo {
    pub compile_task_group_name: String,
    pub compile_distro: String,
}

/// Information about the Evergreen project being run against.
pub struct ProjectInfo {
    /// Path to the evergreen project configuration yaml.
    pub evg_project_location: PathBuf,

    /// Evergreen project being run.
    pub evg_project: String,

    /// Path to the sub-tasks configuration file.
    pub gen_sub_tasks_config_file: Option<PathBuf>,
}

impl ProjectInfo {
    /// Create a new ProjectInfo struct.
    ///
    /// # Arguments
    ///
    /// * `evg_project_location` - Path to the evergreen project configuration yaml.
    /// * `evg_project` - Evergreen project being run.
    /// * `gen_sub_tasks_config_file` - Path to the sub-tasks configuration file.
    ///
    /// # Returns
    ///
    /// Instance of ProjectInfo with provided info.
    pub fn new<P: AsRef<Path>>(
        evg_project_location: P,
        evg_project: &str,
        gen_sub_tasks_config_file: Option<P>,
    ) -> Self {
        Self {
            evg_project_location: evg_project_location.as_ref().to_path_buf(),
            evg_project: evg_project.to_string(),
            gen_sub_tasks_config_file: gen_sub_tasks_config_file.map(|p| p.as_ref().to_path_buf()),
        }
    }

    /// Get the project configuration for this project.
    pub fn get_project_config(&self) -> Result<EvgProjectConfig> {
        Ok(EvgProjectConfig::new(&self.evg_project_location).expect("Could not find evg project"))
    }

    /// Get the generate sub-task configuration for this project.
    pub fn get_generate_sub_tasks_config(&self) -> Result<Option<GenerateSubTasksConfig>> {
        if let Some(gen_sub_tasks_config_file) = &self.gen_sub_tasks_config_file {
            Ok(Some(GenerateSubTasksConfig::from_yaml_file(
                gen_sub_tasks_config_file,
            )?))
        } else {
            Ok(None)
        }
    }
}

/// Configuration required to execute generating tasks.
pub struct ExecutionConfiguration<'a> {
    /// Information about the project being generated under.
    pub project_info: &'a ProjectInfo,
    /// Path to the evergreen API authentication file.
    pub evg_auth_file: &'a Path,
    /// Should task splitting use the fallback method by default.
    pub use_task_split_fallback: bool,
    /// Command to execute resmoke.
    pub resmoke_command: &'a str,
    /// Directory to place generated configuration files.
    pub target_directory: &'a Path,
    /// Task generating the configuration.
    pub generating_task: &'a str,
    /// Location in S3 where generated configuration will be uploaded.
    pub config_location: &'a str,
    /// Should burn_in tasks be generated.
    pub gen_burn_in: bool,
    /// Command to execute burn_in_tests.
    pub burn_in_tests_command: &'a str,
}

/// Collection of services needed to execution.
#[derive(Clone)]
pub struct Dependencies {
    evg_config_utils: Arc<dyn EvgConfigUtils>,
    gen_task_service: Arc<dyn GenerateTasksService>,
    resmoke_config_actor: Arc<tokio::sync::Mutex<dyn ResmokeConfigActor>>,
    burn_in_service: Arc<dyn BurnInService>,
}

impl Dependencies {
    /// Create a new set of dependency instances.
    ///
    /// # Arguments
    ///
    /// * `execution_config` - Information about how generation to take place.
    ///
    /// # Returns
    ///
    /// A set of dependencies to run against.
    pub fn new(execution_config: ExecutionConfiguration) -> Result<Self> {
        let fs_service = Arc::new(FsServiceImpl::new());
        let discovery_service = Arc::new(ResmokeProxy::new(execution_config.resmoke_command));
        let multiversion_service =
            Arc::new(MultiversionServiceImpl::new(discovery_service.clone())?);
        let evg_config_service = Arc::new(execution_config.project_info.get_project_config()?);
        let evg_config_utils = Arc::new(EvgConfigUtilsImpl::new());
        let gen_fuzzer_service = Arc::new(GenFuzzerServiceImpl::new(multiversion_service.clone()));
        let config_extraction_service = Arc::new(ConfigExtractionServiceImpl::new(
            evg_config_utils.clone(),
            execution_config.generating_task.to_string(),
            execution_config.config_location.to_string(),
        ));
        let evg_client = Arc::new(
            EvgClient::from_file(execution_config.evg_auth_file)
                .expect("Cannot find evergreen auth file"),
        );
        let task_history_service = Arc::new(TaskHistoryServiceImpl::new(
            evg_client,
            HISTORY_LOOKBACK_DAYS,
            execution_config.project_info.evg_project.clone(),
        ));
        let resmoke_config_actor =
            Arc::new(tokio::sync::Mutex::new(ResmokeConfigActorService::new(
                discovery_service.clone(),
                fs_service.clone(),
                execution_config
                    .target_directory
                    .to_str()
                    .expect("Unexpected target directory"),
                32,
            )));
        let enterprise_dir = evg_config_service.get_module_dir(ENTERPRISE_MODULE);
        let gen_resmoke_config = GenResmokeConfig::new(
            MAX_SUB_TASKS_PER_TASK,
            execution_config.use_task_split_fallback,
            enterprise_dir,
        );
        let gen_resmoke_task_service = Arc::new(GenResmokeTaskServiceImpl::new(
            task_history_service,
            discovery_service,
            resmoke_config_actor.clone(),
            multiversion_service.clone(),
            fs_service,
            gen_resmoke_config,
        ));
        let gen_sub_tasks_config = execution_config
            .project_info
            .get_generate_sub_tasks_config()?;
        let gen_task_service = Arc::new(GenerateTasksServiceImpl::new(
            evg_config_service,
            evg_config_utils.clone(),
            gen_fuzzer_service,
            gen_resmoke_task_service.clone(),
            config_extraction_service.clone(),
            gen_sub_tasks_config,
            execution_config.gen_burn_in,
        ));

        let burn_in_discovery = Arc::new(BurnInProxy::new(execution_config.burn_in_tests_command));
        let burn_in_service = Arc::new(BurnInServiceImpl::new(
            burn_in_discovery,
            gen_resmoke_task_service,
            config_extraction_service,
            multiversion_service,
        ));

        Ok(Self {
            evg_config_utils,
            gen_task_service,
            resmoke_config_actor,
            burn_in_service,
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
/// * `target_directory` - Directory to store generated configuration.
pub async fn generate_configuration(deps: &Dependencies, target_directory: &Path) -> Result<()> {
    let generate_tasks_service = deps.gen_task_service.clone();
    std::fs::create_dir_all(target_directory)?;

    // We are going to do 2 passes through the project build variants. In this first pass, we
    // are actually going to create all the generated tasks that we discover.
    let generated_tasks = generate_tasks_service.build_generated_tasks(deps).await?;

    // Now that we have generated all the tasks we want to make another pass through all the
    // build variants and add references to the generated tasks that each build variant includes.
    let generated_build_variants =
        generate_tasks_service.generate_build_variants(deps, generated_tasks.clone())?;

    let task_defs: Vec<EvgTask> = {
        let generated_tasks = generated_tasks.lock().unwrap();
        generated_tasks
            .values()
            .flat_map(|g| g.sub_tasks())
            .collect()
    };

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
    /// * `deps` - Service dependencies.
    ///
    /// Returns
    ///
    /// Map of task names to generated task definitions.
    async fn build_generated_tasks(
        &self,
        deps: &Dependencies,
    ) -> Result<Arc<Mutex<GenTaskCollection>>>;

    /// Create build variants definitions containing all the generated tasks for each build variant.
    ///
    /// # Arguments
    ///
    /// * `deps` - Service dependencies.
    /// * `generated_tasks` - Map of task names and their generated configuration.
    ///
    /// # Returns
    ///
    /// Vector of shrub build variants with generated task information.
    fn generate_build_variants(
        &self,
        deps: &Dependencies,
        generated_tasks: Arc<Mutex<GenTaskCollection>>,
    ) -> Result<Vec<BuildVariant>>;

    /// Generate a task for the given task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition to base generated task on.
    /// * `build_variant` - Build Variant to base generated task on.
    ///
    /// # Returns
    ///
    /// Configuration for a generated task.
    async fn generate_task(
        &self,
        task_def: &EvgTask,
        build_variant: &BuildVariant,
    ) -> Result<Option<Box<dyn GeneratedSuite>>>;
}

struct GenerateTasksServiceImpl {
    evg_config_service: Arc<dyn EvgConfigService>,
    evg_config_utils: Arc<dyn EvgConfigUtils>,
    gen_fuzzer_service: Arc<dyn GenFuzzerService>,
    gen_resmoke_service: Arc<dyn GenResmokeTaskService>,
    config_extraction_service: Arc<dyn ConfigExtractionService>,
    gen_sub_tasks_config: Option<GenerateSubTasksConfig>,
    gen_burn_in: bool,
}

impl GenerateTasksServiceImpl {
    /// Create an instance of GenerateTasksServiceImpl.
    ///
    /// # Arguments
    ///
    /// * `evg_config_service` - Service to work with evergreen project configuration.
    /// * `evg_config_utils` - Utilities to work with evergreen project configuration.
    /// * `gen_fuzzer_service` - Service to generate fuzzer tasks.
    /// * `gen_resmoke_service` - Service for generating resmoke tasks.
    /// * `config_extraction_service` - Service to extraction configuration from evergreen config.
    /// * `gen_sub_tasks_config` - Configuration for generating sub-tasks.
    pub fn new(
        evg_config_service: Arc<dyn EvgConfigService>,
        evg_config_utils: Arc<dyn EvgConfigUtils>,
        gen_fuzzer_service: Arc<dyn GenFuzzerService>,
        gen_resmoke_service: Arc<dyn GenResmokeTaskService>,
        config_extraction_service: Arc<dyn ConfigExtractionService>,
        gen_sub_tasks_config: Option<GenerateSubTasksConfig>,
        gen_burn_in: bool,
    ) -> Self {
        Self {
            evg_config_service,
            evg_config_utils,
            gen_fuzzer_service,
            gen_resmoke_service,
            config_extraction_service,
            gen_sub_tasks_config,
            gen_burn_in,
        }
    }

    /// Determine which distro the given sub-tasks should run on.
    ///
    /// By default, we won't specify a distro and they will just use the default for the build
    /// variant. If they specify `use_large_distro` then we should instead use the large distro
    /// configured for the build variant. If that is not defined, then throw an error unless
    /// the build variant is configured to be ignored.
    ///
    /// # Arguments
    ///
    /// * `large_distro_name` - Name
    fn determine_distro(
        &self,
        large_distro_name: &Option<String>,
        generated_task: &dyn GeneratedSuite,
        build_variant_name: &str,
    ) -> Result<Option<String>> {
        if generated_task.use_large_distro() {
            if large_distro_name.is_some() {
                return Ok(large_distro_name.clone());
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

/// An implementation of GeneratorTasksService.
#[async_trait]
impl GenerateTasksService for GenerateTasksServiceImpl {
    /// Build task definition for all tasks
    ///
    /// # Arguments
    ///
    /// * `deps` - Service dependencies.
    ///
    /// Returns
    ///
    /// Map of task names to generated task definitions.
    async fn build_generated_tasks(
        &self,
        deps: &Dependencies,
    ) -> Result<Arc<Mutex<GenTaskCollection>>> {
        let build_variant_list = self.evg_config_service.sort_build_variants_by_required();
        let build_variant_map = self.evg_config_service.get_build_variant_map();
        let task_map = Arc::new(self.evg_config_service.get_task_def_map());

        let mut thread_handles = vec![];

        let generated_tasks = Arc::new(Mutex::new(HashMap::new()));
        let mut seen_tasks = HashSet::new();
        for build_variant in &build_variant_list {
            let build_variant = build_variant_map.get(build_variant).unwrap();
            let is_enterprise = self
                .evg_config_utils
                .is_enterprise_build_variant(build_variant);
            let platform = self
                .evg_config_utils
                .infer_build_variant_platform(build_variant);
            for task in &build_variant.tasks {
                // Burn in tasks could be different for each build variant, so we will always
                // handle them.
                if self.gen_burn_in {
                    if task.name == BURN_IN_TESTS {
                        thread_handles.push(create_burn_in_worker(
                            deps,
                            task_map.clone(),
                            build_variant,
                            build_variant.name.clone(),
                            generated_tasks.clone(),
                        ));
                    }

                    if task.name == BURN_IN_TAGS {
                        for base_bv_name in self
                            .evg_config_utils
                            .lookup_and_split_by_whitespace_build_variant_expansion(
                                BURN_IN_TAG_BUILD_VARIANTS,
                                build_variant,
                            )
                        {
                            let base_build_variant = build_variant_map.get(&base_bv_name).unwrap();
                            let run_build_variant_name =
                                format!("{}-required", base_build_variant.name);
                            thread_handles.push(create_burn_in_worker(
                                deps,
                                task_map.clone(),
                                base_build_variant,
                                run_build_variant_name,
                                generated_tasks.clone(),
                            ));
                        }
                    }

                    continue;
                }

                if task.name == BURN_IN_TESTS || task.name == BURN_IN_TAGS {
                    continue;
                }

                // Skip tasks that have already been seen.
                let task_name = lookup_task_name(is_enterprise, &task.name, &platform);
                if seen_tasks.contains(&task_name) {
                    continue;
                }

                seen_tasks.insert(task_name);
                if let Some(task_def) = task_map.get(&task.name) {
                    if self.evg_config_utils.is_task_generated(task_def) {
                        // Spawn off a tokio task to do the actual generation work.
                        thread_handles.push(create_task_worker(
                            deps,
                            task_def,
                            build_variant,
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
    ///
    /// # Returns
    ///
    /// Configuration for a generated task.
    async fn generate_task(
        &self,
        task_def: &EvgTask,
        build_variant: &BuildVariant,
    ) -> Result<Option<Box<dyn GeneratedSuite>>> {
        let generated_task = if self.evg_config_utils.is_task_fuzzer(task_def) {
            event!(Level::INFO, "Generating fuzzer: {}", task_def.name);

            let params = self
                .config_extraction_service
                .task_def_to_fuzzer_params(task_def, build_variant)?;

            Some(self.gen_fuzzer_service.generate_fuzzer_task(&params)?)
        } else {
            let is_enterprise = self
                .evg_config_utils
                .is_enterprise_build_variant(build_variant);
            let platform = self
                .evg_config_utils
                .infer_build_variant_platform(build_variant);
            event!(
                Level::INFO,
                "Generating resmoke task: {}, is_enterprise: {}, platform: {}",
                task_def.name,
                is_enterprise,
                platform
            );
            let params = self.config_extraction_service.task_def_to_resmoke_params(
                task_def,
                is_enterprise,
                Some(platform),
            )?;
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
    /// * `deps` - Service dependencies.
    /// * `generated_tasks` - Map of task names and their generated configuration.
    ///
    /// # Returns
    ///
    /// Vector of shrub build variants with generated task information.
    fn generate_build_variants(
        &self,
        deps: &Dependencies,
        generated_tasks: Arc<Mutex<GenTaskCollection>>,
    ) -> Result<Vec<BuildVariant>> {
        let mut generated_build_variants = vec![];
        let mut burn_in_tag_build_variant_info: HashMap<String, BurnInTagBuildVariantInfo> =
            HashMap::new();

        let build_variant_map = self.evg_config_service.get_build_variant_map();
        for (bv_name, build_variant) in &build_variant_map {
            let is_enterprise = self
                .evg_config_utils
                .is_enterprise_build_variant(build_variant);
            let platform = self
                .evg_config_utils
                .infer_build_variant_platform(build_variant);
            let mut gen_config = GeneratedConfig::new();
            let mut generating_tasks = vec![];
            let large_distro_name = self
                .evg_config_utils
                .lookup_build_variant_expansion(LARGE_DISTRO_EXPANSION, build_variant);
            for task in &build_variant.tasks {
                if task.name == BURN_IN_TAGS {
                    if self.gen_burn_in {
                        let burn_in_tag_build_variants = self
                            .evg_config_utils
                            .lookup_and_split_by_whitespace_build_variant_expansion(
                                BURN_IN_TAG_BUILD_VARIANTS,
                                build_variant,
                            );
                        let compile_distro = self
                            .evg_config_utils
                            .lookup_build_variant_expansion(
                                BURN_IN_TAG_COMPILE_DISTRO,
                                build_variant,
                            ).unwrap_or_else(|| {
                                panic!(
                                    "`{}` is missing the burn_in_tag compile distro expansion. Set the '{}' expansion in your project's config to continue.",
                                    build_variant.name, BURN_IN_TAG_COMPILE_DISTRO
                                )
                            });
                        let compile_task_group_name = self
                            .evg_config_utils
                            .lookup_build_variant_expansion(
                                BURN_IN_TAG_COMPILE_TASK_GROUP_NAME,
                                build_variant,
                            ).unwrap_or_else(|| {
                                panic!(
                                    "`{}` is missing the burn_in_tag compile task group name expansion. Set the '{}' expansion in your project's config to continue.",
                                    build_variant.name, BURN_IN_TAG_COMPILE_TASK_GROUP_NAME
                                )
                            });
                        for variant in burn_in_tag_build_variants {
                            let bv_info = burn_in_tag_build_variant_info
                                .entry(variant.clone())
                                .or_insert(BurnInTagBuildVariantInfo {
                                    compile_distro: compile_distro.clone(),
                                    compile_task_group_name: compile_task_group_name.clone(),
                                });
                            if bv_info.compile_distro != compile_distro.clone() {
                                panic!(
                                    "`{}` is trying to set a different compile distro than already exists for {}. Check the {} expansions in your config.",
                                    build_variant.name, variant, BURN_IN_TAG_COMPILE_DISTRO
                                )
                            }
                            if bv_info.compile_task_group_name != compile_task_group_name {
                                panic!(
                                    "`{}` is trying to set a different compile task group name than already exists for {}. Check the {} expansions in your config.",
                                    build_variant.name, variant, BURN_IN_TAG_COMPILE_TASK_GROUP_NAME
                                )
                            }
                        }
                    }
                    continue;
                }

                let generated_tasks = generated_tasks.lock().unwrap();

                let task_name = if task.name == BURN_IN_TESTS {
                    format!("{}-{}", BURN_IN_PREFIX, build_variant.name)
                } else {
                    lookup_task_name(is_enterprise, &task.name, &platform)
                };

                if let Some(generated_task) = generated_tasks.get(&task_name) {
                    let distro = self.determine_distro(
                        &large_distro_name,
                        generated_task.as_ref(),
                        bv_name,
                    )?;

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

        for (base_bv_name, bv_info) in burn_in_tag_build_variant_info {
            let generated_tasks = generated_tasks.lock().unwrap();
            let base_build_variant = build_variant_map.get(&base_bv_name).unwrap();
            let run_build_variant_name = format!("{}-required", base_build_variant.name);
            let task_name = format!("{}-{}", BURN_IN_PREFIX, run_build_variant_name);

            if let Some(generated_task) = generated_tasks.get(&task_name) {
                generated_build_variants.push(
                    deps.burn_in_service.generate_burn_in_tags_build_variant(
                        base_build_variant,
                        run_build_variant_name,
                        generated_task.as_ref(),
                        bv_info.compile_distro,
                        bv_info.compile_task_group_name,
                    ),
                );
            }
        }

        Ok(generated_build_variants)
    }
}

/// Determine the task name to use.
///
/// We append "enterprise" to tasks run on enterprise module build variants, so they don't
/// conflict with the normal tasks.
///
/// # Arguments
///
/// * `is_enterprise` - Whether the task is for an enterprise build variant.
/// * `task` - Name of task.
/// * `platform` - Platform that task will run on.
///
/// # Returns
///
/// Name to use for task.
fn lookup_task_name(is_enterprise: bool, task_name: &str, platform: &str) -> String {
    if is_enterprise {
        format!("{}-{}-{}", task_name, platform, ENTERPRISE_MODULE)
    } else {
        format!("{}-{}", task_name, platform)
    }
}

/// Spawn a tokio task to perform the task generation work.
///
/// # Arguments
///
/// * `deps` - Service dependencies.
/// * `task_def` - Evergreen task definition to base generated task off.
/// * `build_variant` - Build variant to query timing information from.
/// * `generated_tasks` - Map to stored generated to in.
///
/// # Returns
///
/// Handle to created tokio worker.
fn create_task_worker(
    deps: &Dependencies,
    task_def: &EvgTask,
    build_variant: &BuildVariant,
    generated_tasks: Arc<Mutex<GenTaskCollection>>,
) -> tokio::task::JoinHandle<()> {
    let generate_task_service = deps.gen_task_service.clone();
    let evg_config_utils = deps.evg_config_utils.clone();
    let task_def = task_def.clone();
    let build_variant = build_variant.clone();
    let generated_tasks = generated_tasks.clone();

    tokio::spawn(async move {
        let generated_task = generate_task_service
            .generate_task(&task_def, &build_variant)
            .await
            .unwrap();

        let is_enterprise = evg_config_utils.is_enterprise_build_variant(&build_variant);
        let platform = evg_config_utils.infer_build_variant_platform(&build_variant);
        let task_name = lookup_task_name(is_enterprise, &task_def.name, &platform);

        if let Some(generated_task) = generated_task {
            let mut generated_tasks = generated_tasks.lock().unwrap();
            generated_tasks.insert(task_name, generated_task);
        }
    })
}

/// Spawn a tokio task to perform the burn_in_test generation work.
///
/// # Arguments
///
/// * `deps` - Service dependencies.
/// * `task_map` - Map of task definitions in evergreen project configuration.
/// * `build_variant` - Build variant to query timing information from.
/// * `run_build_variant_name` - Build variant name to run burn_in_tests task on.
/// * `generated_tasks` - Map to stored generated tasks in.
///
/// # Returns
///
/// Handle to created tokio worker.
fn create_burn_in_worker(
    deps: &Dependencies,
    task_map: Arc<HashMap<String, EvgTask>>,
    build_variant: &BuildVariant,
    run_build_variant_name: String,
    generated_tasks: Arc<Mutex<GenTaskCollection>>,
) -> tokio::task::JoinHandle<()> {
    let burn_in_service = deps.burn_in_service.clone();
    let build_variant = build_variant.clone();
    let generated_tasks = generated_tasks.clone();

    tokio::spawn(async move {
        let generated_task = burn_in_service
            .generate_burn_in_suite(&build_variant.name, &run_build_variant_name, task_map)
            .unwrap();

        let task_name = format!("{}-{}", BURN_IN_PREFIX, run_build_variant_name);

        if !generated_task.sub_tasks().is_empty() {
            let mut generated_tasks = generated_tasks.lock().unwrap();
            generated_tasks.insert(task_name, generated_task);
        }
    })
}

#[cfg(test)]
mod tests {
    use maplit::hashset;
    use rstest::rstest;

    use crate::{
        resmoke::burn_in_proxy::{BurnInDiscovery, DiscoveredTask},
        task_types::{
            fuzzer_tasks::FuzzerGenTaskParams,
            multiversion::{MultiversionIterator, MultiversionService},
            resmoke_tasks::{
                GeneratedResmokeSuite, ResmokeGenParams, ResmokeSuiteGenerationInfo, SubSuite,
            },
        },
    };

    use super::*;

    struct MockConfigService {}
    impl EvgConfigService for MockConfigService {
        fn get_build_variant_map(&self) -> HashMap<String, &BuildVariant> {
            todo!()
        }

        fn get_task_def_map(&self) -> HashMap<String, EvgTask> {
            todo!()
        }

        fn sort_build_variants_by_required(&self) -> Vec<String> {
            todo!()
        }

        fn get_module_dir(&self, _module_name: &str) -> Option<String> {
            todo!()
        }
    }

    struct MockGenFuzzerService {}
    impl GenFuzzerService for MockGenFuzzerService {
        fn generate_fuzzer_task(
            &self,
            _params: &FuzzerGenTaskParams,
        ) -> Result<Box<dyn GeneratedSuite>> {
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
            todo!()
        }
    }

    fn build_mock_generate_tasks_service() -> GenerateTasksServiceImpl {
        let evg_config_utils = Arc::new(EvgConfigUtilsImpl::new());
        GenerateTasksServiceImpl::new(
            Arc::new(MockConfigService {}),
            evg_config_utils.clone(),
            Arc::new(MockGenFuzzerService {}),
            Arc::new(MockGenResmokeTasksService {}),
            Arc::new(ConfigExtractionServiceImpl::new(
                evg_config_utils,
                "generating_task".to_string(),
                "config_location".to_string(),
            )),
            None,
            false,
        )
    }

    // Tests for determine_distro.
    #[rstest]
    #[case(false, None, None)]
    #[case(false, Some("large_distro".to_string()), None)]
    fn test_valid_determine_distros_should_work(
        #[case] use_large_distro: bool,
        #[case] large_distro_name: Option<String>,
        #[case] expected_distro: Option<String>,
    ) {
        let gen_task_service = build_mock_generate_tasks_service();
        let generated_task = Box::new(task_types::resmoke_tasks::GeneratedResmokeSuite {
            task_name: "my task".to_string(),
            sub_suites: vec![],
            use_large_distro,
        });

        let distro = gen_task_service
            .determine_distro(
                &large_distro_name,
                generated_task.as_ref(),
                "my_build_variant",
            )
            .unwrap();

        assert_eq!(distro, expected_distro);
    }

    #[test]
    fn test_determine_distros_should_fail_if_no_large_distro() {
        let gen_task_service = build_mock_generate_tasks_service();
        let generated_task = Box::new(task_types::resmoke_tasks::GeneratedResmokeSuite {
            task_name: "my task".to_string(),
            sub_suites: vec![],
            use_large_distro: true,
        });

        let distro =
            gen_task_service.determine_distro(&None, generated_task.as_ref(), "my_build_variant");

        assert!(distro.is_err());
    }

    #[test]
    fn test_determine_distros_should_no_large_distro_can_be_ignored() {
        let mut gen_task_service = build_mock_generate_tasks_service();
        gen_task_service.gen_sub_tasks_config = Some(GenerateSubTasksConfig {
            build_variant_large_distro_exceptions: hashset! {
                "build_variant_0".to_string(),
                "my_build_variant".to_string(),
                "build_variant_1".to_string(),
            },
        });
        let generated_task = Box::new(task_types::resmoke_tasks::GeneratedResmokeSuite {
            task_name: "my task".to_string(),
            sub_suites: vec![],
            use_large_distro: true,
        });

        let distro =
            gen_task_service.determine_distro(&None, generated_task.as_ref(), "my_build_variant");

        assert!(distro.is_ok());
    }

    // tests for lookup_task_name.
    #[rstest]
    #[case(false, "my_task", "my_platform", "my_task-my_platform")]
    #[case(true, "my_task", "my_platform", "my_task-my_platform-enterprise")]
    fn test_lookup_task_name_should_use_enterprise_when_specified(
        #[case] is_enterprise: bool,
        #[case] task_name: &str,
        #[case] platform: &str,
        #[case] expected_task_name: &str,
    ) {
        assert_eq!(
            lookup_task_name(is_enterprise, task_name, platform),
            expected_task_name.to_string()
        );
    }

    struct MockEvgConfigUtils {}
    impl EvgConfigUtils for MockEvgConfigUtils {
        fn is_task_generated(&self, _task: &EvgTask) -> bool {
            todo!()
        }

        fn is_task_fuzzer(&self, _task: &EvgTask) -> bool {
            todo!()
        }

        fn find_suite_name<'a>(&self, _task: &'a EvgTask) -> &'a str {
            todo!()
        }

        fn get_task_tags(&self, _task: &EvgTask) -> HashSet<String> {
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
            _build_variantt: &BuildVariant,
        ) -> Option<String> {
            todo!()
        }

        fn lookup_build_variant_expansion(
            &self,
            _name: &str,
            _build_variant: &BuildVariant,
        ) -> Option<String> {
            todo!()
        }

        fn lookup_and_split_by_whitespace_build_variant_expansion(
            &self,
            _name: &str,
            _build_variant: &BuildVariant,
        ) -> Vec<String> {
            todo!()
        }

        fn lookup_required_param_str(
            &self,
            _task_def: &EvgTask,
            _run_varr: &str,
        ) -> Result<String> {
            todo!()
        }

        fn lookup_required_param_u64(&self, _task_def: &EvgTask, _run_varr: &str) -> Result<u64> {
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

    struct MockResmokeConfigActorService {}
    #[async_trait]
    impl ResmokeConfigActor for MockResmokeConfigActorService {
        async fn write_sub_suite(&mut self, _gen_suite: &ResmokeSuiteGenerationInfo) {
            todo!()
        }

        async fn flush(&mut self) -> Result<Vec<String>> {
            todo!()
        }
    }

    struct MockBurnInDiscovery {}
    impl BurnInDiscovery for MockBurnInDiscovery {
        fn discover_tasks(&self, _build_variant: &str) -> Result<Vec<DiscoveredTask>> {
            todo!()
        }
    }

    struct MockConfigExtractionService {}
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
            _platform: Option<String>,
        ) -> Result<ResmokeGenParams> {
            todo!()
        }
    }

    struct MockMultiversionService {}
    impl MultiversionService for MockMultiversionService {
        fn get_version_combinations(&self, _suite_name: &str) -> Result<Vec<String>> {
            todo!()
        }

        fn multiversion_iter(&self, _suite_name: &str) -> Result<MultiversionIterator> {
            todo!()
        }

        fn name_multiversion_suite(
            &self,
            _base_name: &str,
            _old_version: &str,
            _version_combination: &str,
        ) -> String {
            todo!()
        }

        fn exclude_tags_for_task(&self, _task_name: &str, _mv_mode: Option<String>) -> String {
            todo!()
        }
    }

    struct MockBurnInService {
        sub_suites: Vec<EvgTask>,
    }
    impl BurnInService for MockBurnInService {
        fn generate_burn_in_suite(
            &self,
            _build_variant: &str,
            _run_build_variant_name: &str,
            _task_map: Arc<HashMap<String, EvgTask>>,
        ) -> Result<Box<dyn GeneratedSuite>> {
            Ok(Box::new(GeneratedResmokeSuite {
                task_name: "burn_in_tests".to_string(),
                sub_suites: self.sub_suites.clone(),
                use_large_distro: false,
            }))
        }

        fn generate_burn_in_tags_build_variant(
            &self,
            _base_build_variant: &BuildVariant,
            _run_build_variant_name: String,
            _generated_task: &dyn GeneratedSuite,
            _compile_distro: String,
            _compile_task_group_name: String,
        ) -> BuildVariant {
            todo!()
        }
    }

    fn build_mocked_burn_in_service(sub_suites: Vec<EvgTask>) -> MockBurnInService {
        MockBurnInService {
            sub_suites: sub_suites.clone(),
        }
    }

    fn build_mocked_dependencies(burn_in_service: MockBurnInService) -> Dependencies {
        Dependencies {
            evg_config_utils: Arc::new(MockEvgConfigUtils {}),
            gen_task_service: Arc::new(build_mock_generate_tasks_service()),
            resmoke_config_actor: Arc::new(tokio::sync::Mutex::new(
                MockResmokeConfigActorService {},
            )),
            burn_in_service: Arc::new(burn_in_service),
        }
    }

    // tests for create_burn_in_worker.
    #[tokio::test]
    async fn test_create_burn_in_worker_should_add_task_when_burn_in_suites_are_present() {
        let mock_burn_in_service = build_mocked_burn_in_service(vec![EvgTask {
            ..Default::default()
        }]);
        let mock_deps = build_mocked_dependencies(mock_burn_in_service);
        let task_map = Arc::new(HashMap::new());
        let generated_tasks = Arc::new(Mutex::new(HashMap::new()));

        let thread_handle = create_burn_in_worker(
            &mock_deps,
            task_map.clone(),
            &BuildVariant {
                ..Default::default()
            },
            "run_bv_name".to_string(),
            generated_tasks.clone(),
        );
        thread_handle.await.unwrap();

        assert_eq!(
            generated_tasks
                .lock()
                .unwrap()
                .contains_key(&format!("{}-{}", BURN_IN_PREFIX, "run_bv_name")),
            true
        );
    }

    #[tokio::test]
    async fn test_create_burn_in_worker_should_not_add_task_when_burn_in_suites_are_absent() {
        let mock_burn_in_service = build_mocked_burn_in_service(vec![]);
        let mock_deps = build_mocked_dependencies(mock_burn_in_service);
        let task_map = Arc::new(HashMap::new());
        let generated_tasks = Arc::new(Mutex::new(HashMap::new()));

        let thread_handle = create_burn_in_worker(
            &mock_deps,
            task_map.clone(),
            &BuildVariant {
                ..Default::default()
            },
            "run_bv_name".to_string(),
            generated_tasks.clone(),
        );
        thread_handle.await.unwrap();

        assert_eq!(
            generated_tasks
                .lock()
                .unwrap()
                .contains_key(&format!("{}-{}", BURN_IN_PREFIX, "run_bv_name")),
            false
        );
    }
}
