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

use anyhow::Result;
use evg_config::{EvgConfigService, EvgConfigUtils, EvgConfigUtilsImpl, EvgProjectConfig};
use lazy_static::lazy_static;
use regex::Regex;
use resmoke_proxy::ResmokeProxy;
use shrub_rs::models::{
    project::EvgProject,
    task::{EvgTask, TaskRef},
    variant::{BuildVariant, DisplayTask},
};
use task_name::remove_gen_suffix;
use task_types::{
    fuzzer_tasks::{FuzzerGenTaskParams, GenFuzzerService, GenFuzzerServiceImpl},
    generated_suite::GeneratedSuite,
    multiversion::MultiversionServiceImpl,
};
use tracing::{event, Level};

mod evergreen_names;
mod evg_config;
mod resmoke_proxy;
mod task_name;
mod task_types;

/// Directory to store the generated configuration in.
const CONFIG_DIR: &str = "generated_resmoke_config";

lazy_static! {
    static ref EXPANSION_RE: Regex =
        Regex::new(r"\$\{(?P<id>[a-zA-Z0-9_]+)(\|(?P<default>.*))?}").unwrap();
}

type GenTaskCollection = HashMap<String, Box<dyn GeneratedSuite>>;

/// Collection of services needed to execution.
#[derive(Clone)]
pub struct Dependencies {
    gen_task_service: Arc<dyn GenerateTasksService>,
}

impl Dependencies {
    /// Create a new set of dependency instances.
    ///
    /// # Arguments
    ///
    /// * `evg_project_location` - Path to the evergreen project configuration yaml.
    ///
    /// # Returns
    ///
    /// A set of dependencies to run against.
    pub fn new(evg_project_location: &Path) -> Result<Self> {
        let discovery_service = Arc::new(ResmokeProxy::new());
        let multiversion_service = Arc::new(MultiversionServiceImpl::new(discovery_service)?);
        let evg_config_service = Arc::new(EvgProjectConfig::new(evg_project_location)?);
        let evg_config_utils = Arc::new(EvgConfigUtilsImpl::new());
        let gen_fuzzer_service = Arc::new(GenFuzzerServiceImpl::new(multiversion_service));
        let gen_task_service = Arc::new(GenerateTasksServiceImpl::new(
            evg_config_service,
            evg_config_utils,
            gen_fuzzer_service,
        ));

        Ok(Self { gen_task_service })
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
pub fn generate_configuration(deps: Dependencies, config_location: &str) -> Result<()> {
    let generate_tasks_service = deps.gen_task_service;

    // We are going to do 2 passes through the project build variants. In this first pass, we
    // are actually going to create all the generated tasks that we discover.
    let generated_tasks = generate_tasks_service.build_generated_tasks(config_location)?;

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

    std::fs::create_dir_all(CONFIG_DIR).unwrap();
    let mut config_file = Path::new(CONFIG_DIR).to_path_buf();
    config_file.push("evergreen_config.json");
    std::fs::write(config_file, serde_json::to_string_pretty(&gen_evg_project)?)?;
    println!("{}", serde_yaml::to_string(&gen_evg_project)?);
    Ok(())
}

/// A service for generating tasks.
trait GenerateTasksService {
    /// Build task definition for all tasks
    ///
    /// # Arguments
    ///
    /// * `config_location` - Location in S3 where generated configuration will be stored.
    ///
    /// Returns
    ///
    /// Map of task names to generated task definitions.
    fn build_generated_tasks(&self, config_location: &str)
        -> Result<Arc<Mutex<GenTaskCollection>>>;

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
}

struct GenerateTasksServiceImpl {
    evg_config_service: Arc<dyn EvgConfigService>,
    evg_config_utils: Arc<dyn EvgConfigUtils>,
    gen_fuzzer_service: Arc<dyn GenFuzzerService>,
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
    ) -> Self {
        Self {
            evg_config_service,
            evg_config_utils,
            gen_fuzzer_service,
        }
    }
}

/// An implementation of GeneratorTasksService.
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
    fn build_generated_tasks(
        &self,
        config_location: &str,
    ) -> Result<Arc<Mutex<GenTaskCollection>>> {
        let build_variant_list = self.evg_config_service.sort_build_variants_by_required();
        let build_variant_map = self.evg_config_service.get_build_variant_map();
        let task_map = self.evg_config_service.get_task_def_map();

        let generated_tasks = Arc::new(Mutex::new(HashMap::new()));
        let mut seen_tasks = HashSet::new();
        for build_variant in &build_variant_list {
            let build_variant = build_variant_map.get(build_variant).unwrap();
            for task in &build_variant.tasks {
                if !seen_tasks.contains(&task.name) {
                    seen_tasks.insert(task.name.to_string());
                    if let Some(task_def) = task_map.get(&task.name) {
                        if self.evg_config_utils.is_task_generated(task_def) {
                            if self.evg_config_utils.is_task_fuzzer(task_def) {
                                event!(Level::INFO, "Generating fuzzer: {}", &task.name,);

                                let params = self.task_def_to_fuzzer_params(
                                    task_def,
                                    build_variant,
                                    config_location,
                                )?;

                                let gen_fuzzer_service = self.gen_fuzzer_service.clone();
                                let generated_task =
                                    gen_fuzzer_service.generate_fuzzer_task(&params).unwrap();
                                let mut generated_tasks = generated_tasks.lock().unwrap();
                                generated_tasks.insert(task.name.clone(), generated_task);
                            } else {
                                event!(Level::INFO, "Generating resmoke task: {}", &task.name,);
                            }
                        }
                    }
                }
            }
        }

        Ok(generated_tasks)
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
            for task in &build_variant.tasks {
                let generated_tasks = generated_tasks.lock().unwrap();
                if let Some(generated_task) = generated_tasks.get(&task.name) {
                    gen_config
                        .display_tasks
                        .push(generated_task.build_display_task());
                    gen_config
                        .gen_task_specs
                        .extend(generated_task.build_task_ref());
                }
            }

            let gen_build_variant = BuildVariant {
                name: build_variant.name.clone(),
                tasks: gen_config.gen_task_specs.clone(),
                display_tasks: Some(gen_config.display_tasks.clone()),
                activate: Some(false),
                ..Default::default()
            };
            generated_build_variants.push(gen_build_variant);
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
                    .get_gen_task_var(task_def, "num_files")
                    .unwrap_or_else(|| panic!("`num_files` missing for task: '{}'", task_def.name)),
                build_variant,
            )
            .unwrap();

        let suite = evg_config_utils.find_suite_name(task_def).to_string();
        Ok(FuzzerGenTaskParams {
            task_name,
            variant: build_variant.name.to_string(),
            suite,
            num_files,
            num_tasks: evg_config_utils.lookup_required_param_u64(task_def, "num_tasks")?,
            resmoke_args: evg_config_utils.lookup_required_param_str(task_def, "resmoke_args")?,
            npm_command: evg_config_utils.lookup_default_param_str(
                task_def,
                "npm_command",
                "jstestfuzz",
            ),
            jstestfuzz_vars: evg_config_utils
                .get_gen_task_var(task_def, "jstestfuzz_vars")
                .map(|j| j.to_string()),
            continue_on_failure: evg_config_utils
                .lookup_required_param_bool(task_def, "continue_on_failure")?,
            resmoke_jobs_max: evg_config_utils
                .lookup_required_param_u64(task_def, "resmoke_jobs_max")?,
            should_shuffle: evg_config_utils
                .lookup_required_param_bool(task_def, "should_shuffle")?,
            timeout_secs: evg_config_utils.lookup_required_param_u64(task_def, "timeout_secs")?,
            require_multiversion_setup: Some(
                task_def
                    .tags
                    .clone()
                    .unwrap_or_default()
                    .contains(&"multiversion".to_string()),
            ),
            config_location: config_location.to_string(),
        })
    }
}
