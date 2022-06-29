use anyhow::Result;
use maplit::hashmap;
use std::{collections::HashMap, path::Path, process::Command};

use shrub_rs::models::{project::EvgProject, task::EvgTask, variant::BuildVariant};

const REQUIRED_PREFIX: &str = "-required";

pub trait EvgConfigService: Sync + Send {
    /// Get a map of build variant names to build variant definitions.
    fn get_build_variant_map(&self) -> HashMap<String, &BuildVariant>;

    /// Get a map of task name to task definitions.
    fn get_task_def_map(&self) -> HashMap<String, EvgTask>;

    /// Get a list of build variants with the required build variants at the start.
    fn sort_build_variants_by_required(&self) -> Vec<String>;

    /// Get the directory of the given module.
    fn get_module_dir(&self, module_name: &str) -> Option<String>;
}

/// Items needed to implement an evergreen configuration service.
pub struct EvgProjectConfig {
    /// Shrub representation of the evg project.
    evg_project: EvgProject,
}

impl EvgProjectConfig {
    /// Create a new instance of an EvgConfigService.
    ///
    /// # Parameters
    ///
    /// * `evg_project_location` - Path to evergreen project configuration to load.
    pub fn new(evg_project_location: &Path) -> Result<Self> {
        let evg_project = get_project_config(evg_project_location)?;
        Ok(Self { evg_project })
    }
}

impl EvgConfigService for EvgProjectConfig {
    /// Get a map of build variant names to build variant definitions.
    fn get_build_variant_map(&self) -> HashMap<String, &BuildVariant> {
        self.evg_project.build_variant_map()
    }

    /// Get a map of task name to task definitions.
    fn get_task_def_map(&self) -> HashMap<String, EvgTask> {
        let mut task_map = hashmap! {};
        for (k, v) in self.evg_project.task_def_map() {
            task_map.insert(k, v.clone());
        }
        task_map
    }

    /// Get a list of build variants with the required build variants at the start.
    fn sort_build_variants_by_required(&self) -> Vec<String> {
        let build_variant_map = self.get_build_variant_map();
        let mut build_variants: Vec<String> = build_variant_map
            .keys()
            .into_iter()
            .filter_map(|bv| {
                if bv.ends_with(REQUIRED_PREFIX) {
                    Some(bv.to_string())
                } else {
                    None
                }
            })
            .collect();

        build_variants.extend::<Vec<String>>(
            build_variant_map
                .keys()
                .into_iter()
                .filter_map(|bv| {
                    if !bv.ends_with(REQUIRED_PREFIX) {
                        Some(bv.to_string())
                    } else {
                        None
                    }
                })
                .collect(),
        );

        build_variants
    }

    /// Get the directory of the given module.
    fn get_module_dir(&self, module_name: &str) -> Option<String> {
        if let Some(modules) = &self.evg_project.modules {
            for module in modules {
                if module.name == module_name {
                    return Some(format!("{}/{}", &module.prefix, module_name));
                }
            }
        }
        None
    }
}

/// Evaluate the evergreen configuration and load it into a shrub project.
///
/// # Arguments
///
/// * `location` - Path to file containing evergreen configuration to load.
///
/// # Returns
///
/// Shrub representation of evergreen configuration.
fn get_project_config(location: &Path) -> Result<EvgProject> {
    let evg_config_yaml = Command::new("evergreen")
        .args(&["evaluate", location.to_str().unwrap()])
        .output()?;
    Ok(EvgProject::from_yaml_str(std::str::from_utf8(&evg_config_yaml.stdout)?).unwrap())
}
