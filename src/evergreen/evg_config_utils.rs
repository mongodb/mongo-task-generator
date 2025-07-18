use std::collections::{HashMap, HashSet};
use std::vec;

use anyhow::{bail, Result};
use lazy_static::lazy_static;
use regex::Regex;
use shrub_rs::models::commands::EvgCommand::Function;
use shrub_rs::models::params::ParamValue;
use shrub_rs::models::task::TaskDependency;
use shrub_rs::models::{commands::FunctionCall, task::EvgTask, variant::BuildVariant};

use crate::evergreen_names::{
    BURN_IN_TAG_EXCLUDE_BUILD_VARIANTS, BURN_IN_TAG_INCLUDE_ALL_REQUIRED_AND_SUGGESTED,
    BURN_IN_TAG_INCLUDE_BUILD_VARIANTS, GENERATE_RESMOKE_TASKS, INITIALIZE_MULTIVERSION_TASKS,
    IS_FUZZER, LINUX, MACOS, RUN_RESMOKE_TESTS, WINDOWS,
};
use crate::utils::task_name::remove_gen_suffix;

lazy_static! {
    /// Regular expression for finding expansions.
    ///   `${expansion}` or `${expansion|default_value}`
    static ref EXPANSION_RE: Regex =
        Regex::new(r"\$\{(?P<id>[a-zA-Z0-9_]+)(\|(?P<default>.*))?}").unwrap();
}

/// Multiversion task that will be generated.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct MultiversionGenerateTaskConfig {
    /// Name of suite to use for the generated task.
    pub suite_name: String,
    /// Old version to run testing against.
    pub old_version: String,
    /// The bazel test target, if it is a bazel-based resmoke task.
    pub bazel_target: Option<String>,
}

pub trait EvgConfigUtils: Sync + Send {
    /// Determine if the given evergreen task is a generated task.
    ///
    /// # Arguments
    ///
    /// * `task` - Evergreen task to check.
    ///
    /// # Returns
    ///
    /// `true` if the given task is generated.
    fn is_task_generated(&self, task: &EvgTask) -> bool;

    /// Determine if the given evergreen task a fuzzer task.
    ///
    /// # Arguments
    ///
    /// * `task` - Evergreen task to check.
    ///
    /// # Returns
    ///
    /// `true` if the given task is a fuzzer task.
    fn is_task_fuzzer(&self, task: &EvgTask) -> bool;

    /// Find the name of the resmoke suite the given task executes.
    ///
    /// # Arguments
    ///
    /// * `task` - Evergreen task to check.
    ///
    /// # Returns
    ///
    /// Name of task the given resmoke suite executes.
    fn find_suite_name<'a>(&self, task: &'a EvgTask) -> &'a str;

    /// Get a set of the task tags defined in the given task definition.
    ///
    /// # Arguments
    ///
    /// * `task` - Evergreen task to query.
    ///
    /// # Returns
    ///
    /// Set of tags assigned to the task.
    fn get_task_tags(&self, task: &EvgTask) -> HashSet<String>;

    /// Get the multiversion generate tasks in the task definition.
    ///
    /// # Arguments
    ///
    /// * `task` - Evergreen task to query.
    ///
    /// # Returns
    ///
    /// List of multiversion generate tasks.
    fn get_multiversion_generate_tasks(
        &self,
        task: &EvgTask,
    ) -> Option<Vec<MultiversionGenerateTaskConfig>>;

    /// Get a list of tasks the given task depends on.
    ///
    /// # Arguments
    ///
    /// * `task` - Evergreen task to query.
    ///
    /// # Returns
    ///
    /// List of task names the task depends on.
    fn get_task_dependencies(&self, task: &EvgTask) -> Vec<String>;

    fn get_task_ref_dependencies(
        &self,
        task_name: &str,
        build_variant: &BuildVariant,
    ) -> Option<Vec<TaskDependency>>;

    /// Lookup the given variable in the vars section of the 'generate resmoke task' func.
    ///
    /// # Arguments
    ///
    /// * `task` - Shrub task to query.
    /// * `var` - Name of variable to lookup.
    ///
    /// # Returns
    ///
    /// Value of given variable in the 'generate resmoke task' vars.
    fn get_gen_task_var<'a>(&self, task: &'a EvgTask, var: &str) -> Option<&'a str>;

    /// Get vars HashMap of the 'generate resmoke task' func.
    ///
    /// # Arguments
    ///
    /// * `task` - Shrub task to query.
    ///
    /// # Returns
    ///
    /// HashMap of vars in the 'generate resmoke task'.
    fn get_gen_task_vars(&self, task: &EvgTask) -> Option<HashMap<String, ParamValue>>;

    /// Lookup the given 'run_var' in the build variant 'vars' and provide the value.
    ///
    /// # Arguments
    ///
    /// * `run_var` - Name of var to lookup.
    /// * `build_variant` - Build Variant to search for var in.
    ///
    /// # Returns
    ///
    /// Value to use for the given `run_var` if found.
    fn translate_run_var(&self, run_var: &str, build_variant: &BuildVariant) -> Option<String>;

    /// Lookup the specified expansion in the given build variant.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of expansion to query.
    /// * `build_variant` - Build Variant to query.
    ///
    /// # Returns
    ///
    /// Value of expansion if it exists.
    fn lookup_build_variant_expansion(
        &self,
        name: &str,
        build_variant: &BuildVariant,
    ) -> Option<String>;

    /// Lookup and split by whitespace the specified expansion in the given build variant.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of expansion to query.
    /// * `build_variant` - Build Variant to query.
    ///
    /// # Returns
    ///
    /// List of values of expansion splitted by whitespace if it exists.
    fn lookup_and_split_by_whitespace_build_variant_expansion(
        &self,
        name: &str,
        build_variant: &BuildVariant,
    ) -> Vec<String>;

    /// Determine corresponding burn in tag build variants for the given build variant.
    ///
    /// # Arguments
    ///
    /// * `build_variant` - Build Variant to query.
    /// * `build_variant_map` - A map of build variant names to their definitions.
    ///
    /// # Returns
    ///
    /// List of build variant names to use for burn in tags
    fn resolve_burn_in_tag_build_variants(
        &self,
        build_variant: &BuildVariant,
        build_variant_map: &HashMap<String, &BuildVariant>,
    ) -> Vec<String>;

    /// Lookup the given variable in the task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition to query.
    /// * `run_var` - Variable to query.
    ///
    /// # Returns
    ///
    /// Value of run_var for the given task, an `Error` will be returned if not defined.
    fn lookup_required_param_str(&self, task_def: &EvgTask, run_var: &str) -> Result<String>;

    /// Lookup the given variable in the task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition to query.
    /// * `run_var` - Variable to query.
    ///
    /// # Returns
    ///
    /// Value of run_var for the given task, an `Error` will be returned if not defined.
    fn lookup_required_param_u64(&self, task_def: &EvgTask, run_var: &str) -> Result<u64>;

    /// Lookup the given variable in the task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition to query.
    /// * `run_var` - Variable to query.
    ///
    /// # Returns
    ///
    /// Value of run_var for the given task, an `Error` will be returned if not defined.
    fn lookup_required_param_bool(&self, task_def: &EvgTask, run_var: &str) -> Result<bool>;

    /// Lookup the given variable in the task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition to query.
    /// * `run_var` - Variable to query.
    /// * `default` - Default value to use if `run_var` is undefined.
    ///
    /// # Returns
    ///
    /// Value of run_var for the given task, the default will be returned if not defined.
    fn lookup_default_param_bool(
        &self,
        task_def: &EvgTask,
        run_var: &str,
        default: bool,
    ) -> Result<bool>;

    /// Lookup the given variable in the task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition to query.
    /// * `run_var` - Variable to query.
    /// * `default` - Default value to use if `run_var` is undefined.
    ///
    /// # Returns
    ///
    /// Value of run_var for the given task, the default will be returned if not defined.
    fn lookup_default_param_str(&self, task_def: &EvgTask, run_var: &str, default: &str) -> String;

    /// Lookup the given variable in the task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition to query.
    /// * `run_var` - Variable to query.
    ///
    /// # Returns
    ///
    /// Value of run_var for the given task if it exists.
    fn lookup_optional_param_u64(&self, task_def: &EvgTask, run_var: &str) -> Result<Option<u64>>;

    /// Check if the given build variant includes the enterprise module.
    ///
    /// # Arguments
    ///
    /// * `build_variant` - Build variant to check.
    ///
    /// # Returns
    ///
    /// true if given build variant includes the enterprise module.
    fn is_enterprise_build_variant(&self, build_variant: &BuildVariant) -> bool;

    /// Infer platform that build variant will be running on.
    ///
    /// # Arguments
    ///
    /// * `build_variant` - Build variant to query.
    ///
    /// # Returns
    ///
    /// Linux, or Mac, or Windows platform that build variant will be running on.
    fn infer_build_variant_platform(&self, build_variant: &BuildVariant) -> String;
}

/// Service for utilities to help interpret evergreen configuration.
pub struct EvgConfigUtilsImpl {}

impl EvgConfigUtilsImpl {
    /// Create a new instance of the EvgConfigUtilsImpl.
    pub fn new() -> Self {
        Self {}
    }
}

impl EvgConfigUtils for EvgConfigUtilsImpl {
    /// Determine if the given evergreen task a generated task.
    ///
    /// # Arguments
    ///
    /// * `task` - Evergreen task to check.
    ///
    /// # Returns
    ///
    /// `true` if the given task is generated.
    fn is_task_generated(&self, task: &EvgTask) -> bool {
        if let Some(commands) = &task.commands {
            commands.iter().any(|c| {
                if let Function(func) = c {
                    if func.func == GENERATE_RESMOKE_TASKS {
                        return true;
                    }
                }
                false
            })
        } else {
            false
        }
    }

    /// Determine if the given evergreen task a fuzzer task.
    ///
    /// # Arguments
    ///
    /// * `task` - Evergreen task to check.
    ///
    /// # Returns
    ///
    /// `true` if the given task is a fuzzer task.
    fn is_task_fuzzer(&self, task: &EvgTask) -> bool {
        let is_jstestfuzz = self.get_gen_task_var(task, IS_FUZZER);
        if let Some(is_jstestfuzz) = is_jstestfuzz {
            is_jstestfuzz == "true"
        } else {
            false
        }
    }

    /// Find the name of the resmoke suite the given task executes.
    ///
    /// # Arguments
    ///
    /// * `task` - Evergreen task to check.
    ///
    /// # Returns
    ///
    /// Name of task the given resmoke suite executes.
    fn find_suite_name<'a>(&self, task: &'a EvgTask) -> &'a str {
        let optional_vars = get_resmoke_vars(task);

        let generated_task_name = remove_gen_suffix(&task.name);

        if let Some(vars) = optional_vars {
            if let Some(ParamValue::String(suite)) = vars.get("suite") {
                if is_bazel_suite(suite) {
                    get_bazel_suite_name(suite)
                } else {
                    suite
                }
            } else {
                generated_task_name
            }
        } else {
            generated_task_name
        }
    }

    /// Get a set of the task tags defined in the given task definition.
    ///
    /// # Arguments
    ///
    /// * `task` - Evergreen task to query.
    ///
    /// # Returns
    ///
    /// Set of tags assigned to the task.
    fn get_task_tags(&self, task: &EvgTask) -> HashSet<String> {
        task.tags
            .clone()
            .unwrap_or_default()
            .iter()
            .map(|t| t.to_string())
            .collect()
    }

    /// Get the multiversion generate tasks in the task definition.
    ///
    /// # Arguments
    ///
    /// * `task` - Evergreen task to query.
    ///
    /// # Returns
    ///
    /// List of multiversion generate tasks.
    fn get_multiversion_generate_tasks(
        &self,
        task: &EvgTask,
    ) -> Option<Vec<MultiversionGenerateTaskConfig>> {
        if let Some(multiversion_task_map) =
            get_func_vars_by_name(task, INITIALIZE_MULTIVERSION_TASKS)
        {
            let mut multiversion_generate_tasks = vec![];
            for (suite, old_version) in multiversion_task_map {
                let (suite_name, bazel_target) = if is_bazel_suite(suite) {
                    (get_bazel_suite_name(suite).to_string(), Some(suite.clone()))
                } else {
                    (suite.clone(), None)
                };

                if let ParamValue::String(value) = old_version {
                    multiversion_generate_tasks.push(MultiversionGenerateTaskConfig {
                        suite_name,
                        old_version: value.clone(),
                        bazel_target,
                    });
                }
            }
            return Some(multiversion_generate_tasks);
        }
        None
    }

    /// Get a list of tasks the given task depends on.
    ///
    /// # Arguments
    ///
    /// * `task` - Evergreen task to query.
    ///
    /// # Returns
    ///
    /// List of task names the task depends on.
    fn get_task_dependencies(&self, task: &EvgTask) -> Vec<String> {
        let dependencies = task
            .clone()
            .depends_on
            .map(|dep_list| dep_list.iter().map(|d| d.name.to_string()).collect());

        dependencies.unwrap_or_default()
    }

    fn get_task_ref_dependencies(
        &self,
        task_name: &str,
        build_variant: &BuildVariant,
    ) -> Option<Vec<TaskDependency>> {
        for task_ref in &build_variant.tasks {
            if task_ref.name == task_name {
                let dependencies = task_ref.depends_on.clone();
                return dependencies;
            }
        }
        None
    }

    /// Lookup the given variable in the vars section of the 'generate resmoke task' func.
    ///
    /// # Arguments
    ///
    /// * `task` - Shrub task to query.
    /// * `var` - Name of variable to lookup.
    ///
    /// # Returns
    ///
    /// Value of given variable in the 'generate resmoke task' vars.
    fn get_gen_task_var<'a>(&self, task: &'a EvgTask, var: &str) -> Option<&'a str> {
        if let Some(vars) = get_func_vars_by_name(task, GENERATE_RESMOKE_TASKS) {
            if let Some(ParamValue::String(value)) = vars.get(var) {
                return Some(value);
            }
        }
        None
    }

    /// Get vars HashMap of the 'generate resmoke task' func.
    ///
    /// # Arguments
    ///
    /// * `task` - Shrub task to query.
    ///
    /// # Returns
    ///
    /// HashMap of vars in the 'generate resmoke task'.
    fn get_gen_task_vars(&self, task: &EvgTask) -> Option<HashMap<String, ParamValue>> {
        if let Some(vars) = get_func_vars_by_name(task, GENERATE_RESMOKE_TASKS) {
            return Some(vars.clone());
        }
        None
    }

    /// Lookup the given 'run_var' in the build variant 'vars' and provide the value.
    ///
    /// # Arguments
    ///
    /// * `run_var` - Name of var to lookup.
    /// * `build_variant` - Build Variant to search for var in.
    ///
    /// # Returns
    ///
    /// Value to use for the given `run_var` if found.
    fn translate_run_var(&self, run_var: &str, build_variant: &BuildVariant) -> Option<String> {
        let expansion = EXPANSION_RE.captures(run_var);
        if let Some(captures) = expansion {
            let id = captures.name("id").unwrap();
            if let Some(value) = self.lookup_build_variant_expansion(id.as_str(), build_variant) {
                Some(value)
            } else {
                captures.name("default").map(|d| d.as_str().to_string())
            }
        } else {
            Some(run_var.to_string())
        }
    }

    /// Lookup the specified expansion in the given build variant.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of expansion to query.
    /// * `build_variant` - Build Variant to query.
    ///
    /// # Returns
    ///
    /// Value of expansion if it exists.
    fn lookup_build_variant_expansion(
        &self,
        name: &str,
        build_variant: &BuildVariant,
    ) -> Option<String> {
        build_variant
            .expansions
            .clone()
            .unwrap_or_default()
            .get(name)
            .map(|v| v.to_string())
    }

    /// Lookup and split by whitespace the specified expansion in the given build variant.
    ///
    /// # Arguments
    ///
    /// * `name` - Name of expansion to query.
    /// * `build_variant` - Build Variant to query.
    ///
    /// # Returns
    ///
    /// List of values of expansion splitted by whitespace if it exists.
    fn lookup_and_split_by_whitespace_build_variant_expansion(
        &self,
        name: &str,
        build_variant: &BuildVariant,
    ) -> Vec<String> {
        self.lookup_build_variant_expansion(name, build_variant)
            .unwrap_or_default()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect()
    }

    /// Determine burn in tag build variants for the given build variant.
    ///
    /// # Arguments
    ///
    /// * `build_variant` - Build Variant to query.
    /// * `build_variant_map` - A map of build variant names to their definitions.
    ///
    /// # Returns
    ///
    /// List of build variant names to use for burn in tags.
    fn resolve_burn_in_tag_build_variants(
        &self,
        build_variant: &BuildVariant,
        build_variant_map: &HashMap<String, &BuildVariant>,
    ) -> Vec<String> {
        let mut burn_in_build_variants = self
            .lookup_and_split_by_whitespace_build_variant_expansion(
                BURN_IN_TAG_INCLUDE_BUILD_VARIANTS,
                build_variant,
            );
        if self
            .lookup_build_variant_expansion(
                BURN_IN_TAG_INCLUDE_ALL_REQUIRED_AND_SUGGESTED,
                build_variant,
            )
            .unwrap_or_else(|| "false".to_string())
            .parse::<bool>()
            .unwrap()
        {
            burn_in_build_variants.extend(
                build_variant_map
                    .iter()
                    .filter_map(|(name, build_variant)| {
                        let display_name = build_variant.display_name.as_ref().unwrap();
                        if display_name.starts_with('!') || display_name.starts_with('*') {
                            Some(name.to_string())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<String>>(),
            );
        }
        let exclude_burn_in_build_variants = self
            .lookup_and_split_by_whitespace_build_variant_expansion(
                BURN_IN_TAG_EXCLUDE_BUILD_VARIANTS,
                build_variant,
            );
        burn_in_build_variants
            .into_iter()
            .collect::<HashSet<String>>()
            .into_iter()
            .filter(|name| !exclude_burn_in_build_variants.contains(name))
            .collect::<Vec<String>>()
    }

    /// Lookup the given variable in the task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition to query.
    /// * `run_var` - Variable to query.
    ///
    /// # Returns
    ///
    /// Value of run_var for the given task, an `Error` will be returned if not defined.
    fn lookup_required_param_str(&self, task_def: &EvgTask, run_var: &str) -> Result<String> {
        Ok(match self.get_gen_task_var(task_def, run_var) {
            Some(v) => v.to_string(),
            _ => bail!(format!(
                "Missing var '{}' for task '{}'",
                run_var, task_def.name
            )),
        })
    }

    /// Lookup the given variable in the task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition to query.
    /// * `run_var` - Variable to query.
    ///
    /// # Returns
    ///
    /// Value of run_var for the given task, an `Error` will be returned if not defined.
    fn lookup_required_param_u64(&self, task_def: &EvgTask, run_var: &str) -> Result<u64> {
        Ok(match self.get_gen_task_var(task_def, run_var) {
            Some(v) => v.parse()?,
            _ => bail!(format!(
                "Missing var '{}' for task '{}'",
                run_var, task_def.name
            )),
        })
    }

    /// Lookup the given variable in the task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition to query.
    /// * `run_var` - Variable to query.
    ///
    /// # Returns
    ///
    /// Value of run_var for the given task, an `Error` will be returned if not defined.
    fn lookup_required_param_bool(&self, task_def: &EvgTask, run_var: &str) -> Result<bool> {
        Ok(match self.get_gen_task_var(task_def, run_var) {
            Some(v) => v.parse()?,
            _ => bail!(format!(
                "Missing var '{}' for task '{}'",
                run_var, task_def.name
            )),
        })
    }

    /// Lookup the given variable in the task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition to query.
    /// * `run_var` - Variable to query.
    /// * `default` - Default value to use if `run_var` is undefined.
    ///
    /// # Returns
    ///
    /// Value of run_var for the given task, the default will be returned if not defined.
    fn lookup_default_param_bool(
        &self,
        task_def: &EvgTask,
        run_var: &str,
        default: bool,
    ) -> Result<bool> {
        Ok(match self.get_gen_task_var(task_def, run_var) {
            Some(v) => v.parse()?,
            _ => default,
        })
    }

    /// Lookup the given variable in the task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition to query.
    /// * `run_var` - Variable to query.
    /// * `default` - Default value to use if `run_var` is undefined.
    ///
    /// # Returns
    ///
    /// Value of run_var for the given task, the default will be returned if not defined.
    fn lookup_default_param_str(&self, task_def: &EvgTask, run_var: &str, default: &str) -> String {
        self.get_gen_task_var(task_def, run_var)
            .unwrap_or(default)
            .to_string()
    }

    /// Lookup the given variable in the task definition.
    ///
    /// # Arguments
    ///
    /// * `task_def` - Task definition to query.
    /// * `run_var` - Variable to query.
    ///
    /// # Returns
    ///
    /// Value of run_var for the given task if it exists.
    fn lookup_optional_param_u64(&self, task_def: &EvgTask, run_var: &str) -> Result<Option<u64>> {
        Ok(match self.get_gen_task_var(task_def, run_var) {
            Some(v) => Some(v.parse()?),
            _ => None,
        })
    }

    /// Check if the given build variant includes the enterprise module.
    ///
    /// # Arguments
    ///
    /// * `build_variant` - Build variant to check.
    ///
    /// # Returns
    ///
    /// true if given build variant includes the enterprise module.
    fn is_enterprise_build_variant(&self, build_variant: &BuildVariant) -> bool {
        let pattern = Regex::new(r"--enableEnterpriseTests\s*=?\s*off").unwrap();
        if let Some(expansions_map) = &build_variant.expansions {
            for (_key, value) in expansions_map.iter() {
                if pattern.is_match(value) {
                    return false;
                }
            }
        }
        true
    }

    /// Infer platform that build variant will run on.
    ///
    /// # Arguments
    ///
    /// * `build_variant` - Build variant to query.
    ///
    /// # Returns
    ///
    /// linux, or mac, or windows platform that build variant will run on.
    fn infer_build_variant_platform(&self, build_variant: &BuildVariant) -> String {
        let distro = build_variant
            .run_on
            .as_ref()
            .unwrap_or(&vec!["".to_string()])
            .first()
            .unwrap_or(&"".to_string())
            .to_lowercase();

        if distro.contains(MACOS) {
            MACOS.to_string()
        } else if distro.contains(WINDOWS) {
            WINDOWS.to_string()
        } else {
            LINUX.to_string()
        }
    }
}

/// Get the shrub function make the 'generate resmoke task' call in the given task.
///
/// # Arguments
///
/// * `task` - Shrub task to query.
/// * `func_name` - Function to lookup.
///
/// # Returns
///
/// Function call to 'generate resmoke task'.
fn get_func_by_name<'a>(task: &'a EvgTask, func_name: &str) -> Option<&'a FunctionCall> {
    let command = if let Some(commands) = &task.commands {
        commands.iter().find(|c| {
            if let Function(func) = c {
                if func.func == func_name {
                    return true;
                }
            }
            false
        })
    } else {
        None
    };

    if let Some(Function(func)) = command {
        Some(func)
    } else {
        None
    }
}

/// Get vars HashMap of the given func.
///
/// # Arguments
///
/// * `task` - Shrub task to query.
/// * `func_name` - Function to lookup.
///
/// # Returns
///
/// HashMap of vars in the given function.
fn get_func_vars_by_name<'a>(
    task: &'a EvgTask,
    func_name: &str,
) -> Option<&'a HashMap<String, ParamValue>> {
    if let Some(func) = get_func_by_name(task, func_name) {
        return func.vars.as_ref();
    }
    None
}

/// Get the vars passed to "generate resmoke task" or "run tests".
///
/// # Arguments
///
/// * `task` - Shrub task to query.
///
/// # Returns
///
/// vars forwarded to resmoke.py.
fn get_resmoke_vars(task: &EvgTask) -> Option<&HashMap<String, ParamValue>> {
    if let Some(generate_resmoke_tasks_vars) = get_func_vars_by_name(task, GENERATE_RESMOKE_TASKS) {
        return Some(generate_resmoke_tasks_vars);
    }
    return get_func_vars_by_name(task, RUN_RESMOKE_TESTS);
}

/// Checks if a Resmoke suite is a bazel target.
///
/// # Arguments
///
/// * `suite` - A suite name from Evergreen YAML.
///
/// # Returns
///
/// True if the suite looks like a bazel target (e.g. starts with `//`).
pub fn is_bazel_suite(suite: &str) -> bool {
    suite.starts_with("//")
}

/// Get a suite name from a bazel target.
///
/// # Arguments
///
/// * `target` - A bazel target.
///
/// # Returns
///
/// A useful suite name, just the name of the target without the bazel package prefix.
pub fn get_bazel_suite_name(target: &str) -> &str {
    let (_, name) = target.rsplit_once(':').unwrap();
    name
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use maplit::btreemap;
    use maplit::hashmap;
    use rstest::rstest;
    use shrub_rs::models::commands::{fn_call, fn_call_with_params};
    use shrub_rs::models::params::ParamValue;
    use shrub_rs::models::project::EvgProject;
    use shrub_rs::models::task::TaskDependency;

    use super::*;

    // test burn in variant resolver

    fn get_evg_project() -> EvgProject {
        EvgProject {
            buildvariants: vec![
                BuildVariant {
                    name: "bv1".to_string(),
                    display_name: Some("! required".to_string()),
                    ..Default::default()
                },
                BuildVariant {
                    name: "bv2".to_string(),
                    display_name: Some("* suggested".to_string()),
                    ..Default::default()
                },
                BuildVariant {
                    name: "bv3".to_string(),
                    display_name: Some("other bv asdf".to_string()),
                    ..Default::default()
                },
                BuildVariant {
                    name: "bv4".to_string(),
                    display_name: Some("other bv xyz".to_string()),
                    ..Default::default()
                },
                BuildVariant {
                    name: "bv5".to_string(),
                    display_name: Some("other bv 123".to_string()),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }
    #[test]
    fn test_resolve_burn_in_tag_bv_none() {
        let build_variant = BuildVariant {
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let lookup = evg_config_utils.resolve_burn_in_tag_build_variants(
            &build_variant,
            &get_evg_project().build_variant_map(),
        );

        assert_eq!(lookup.len(), 0);
    }
    #[test]
    fn test_resolve_burn_in_tag_bv_includes() {
        let build_variant = BuildVariant {
            expansions: Some(BTreeMap::from([
                (
                    BURN_IN_TAG_INCLUDE_BUILD_VARIANTS.to_string(),
                    "bv1 bv3 bv5".to_string(),
                ),
                (
                    BURN_IN_TAG_EXCLUDE_BUILD_VARIANTS.to_string(),
                    "bv5".to_string(),
                ),
            ])),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let lookup = evg_config_utils.resolve_burn_in_tag_build_variants(
            &build_variant,
            &get_evg_project().build_variant_map(),
        );

        assert_eq!(lookup.len(), 2);
        assert_eq!(lookup.contains(&"bv1".to_string()), true);
        assert_eq!(lookup.contains(&"bv3".to_string()), true);
    }
    #[test]
    fn test_resolve_burn_in_tag_bv_suggested_and_required() {
        let build_variant = BuildVariant {
            expansions: Some(BTreeMap::from([
                (
                    BURN_IN_TAG_INCLUDE_BUILD_VARIANTS.to_string(),
                    "bv3 bv5".to_string(),
                ),
                (
                    BURN_IN_TAG_EXCLUDE_BUILD_VARIANTS.to_string(),
                    "bv2".to_string(),
                ),
                (
                    BURN_IN_TAG_INCLUDE_ALL_REQUIRED_AND_SUGGESTED.to_string(),
                    "true".to_string(),
                ),
            ])),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let lookup = evg_config_utils.resolve_burn_in_tag_build_variants(
            &build_variant,
            &get_evg_project().build_variant_map(),
        );

        assert_eq!(lookup.len(), 3);
        assert_eq!(lookup.contains(&"bv1".to_string()), true);
        assert_eq!(lookup.contains(&"bv3".to_string()), true);
        assert_eq!(lookup.contains(&"bv5".to_string()), true);
    }

    #[test]
    fn test_resolve_burn_in_tag_bv_suggested_and_required_duplicates() {
        let build_variant = BuildVariant {
            expansions: Some(BTreeMap::from([
                (
                    BURN_IN_TAG_INCLUDE_BUILD_VARIANTS.to_string(),
                    "bv1 bv2 bv3 bv5".to_string(),
                ),
                (
                    BURN_IN_TAG_EXCLUDE_BUILD_VARIANTS.to_string(),
                    "bv2".to_string(),
                ),
                (
                    BURN_IN_TAG_INCLUDE_ALL_REQUIRED_AND_SUGGESTED.to_string(),
                    "true".to_string(),
                ),
            ])),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let lookup = evg_config_utils.resolve_burn_in_tag_build_variants(
            &build_variant,
            &get_evg_project().build_variant_map(),
        );

        assert_eq!(lookup.len(), 3);
        assert_eq!(lookup.contains(&"bv1".to_string()), true);
        assert_eq!(lookup.contains(&"bv3".to_string()), true);
        assert_eq!(lookup.contains(&"bv5".to_string()), true);
    }

    // is_task_generated tests.

    #[test]
    fn test_is_task_generated_should_return_false_if_not_generated() {
        let evg_task = EvgTask {
            commands: Some(vec![fn_call("hello world"), fn_call("run tests")]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(evg_config_utils.is_task_generated(&evg_task), false);
    }

    #[test]
    fn test_is_task_generated_should_return_true_if_generated() {
        let evg_task = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call("generate resmoke tasks"),
                fn_call("run tests"),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(evg_config_utils.is_task_generated(&evg_task), true);
    }

    // is_task_fuzzer tests.
    #[test]
    fn test_is_task_fuzzer_should_return_false_if_var_is_missing() {
        let evg_task = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call_with_params(
                    "generate resmoke tasks",
                    hashmap! {
                        "var1".to_string() => ParamValue::from("value1"),
                        "var2".to_string() => ParamValue::from("value2"),
                    },
                ),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(evg_config_utils.is_task_fuzzer(&evg_task), false);
    }

    #[test]
    fn test_is_task_fuzzer_should_return_true_is_var_is_true() {
        let evg_task = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call_with_params(
                    "generate resmoke tasks",
                    hashmap! {
                        "var1".to_string() => ParamValue::from("value1"),
                        "is_jstestfuzz".to_string() => ParamValue::from("true"),
                        "var2".to_string() => ParamValue::from("value2"),
                    },
                ),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(evg_config_utils.is_task_fuzzer(&evg_task), true);
    }

    // find_suite_name tests.
    #[test]
    fn test_find_suite_name_should_use_suite_var_for_generated_task_if_it_exists() {
        let evg_task = EvgTask {
            name: "my_task_gen".to_string(),
            commands: Some(vec![
                fn_call("hello world"),
                fn_call_with_params(
                    "generate resmoke tasks",
                    hashmap! {
                        "var1".to_string() => ParamValue::from("value1"),
                        "suite".to_string() => ParamValue::from("my suite name"),
                        "var2".to_string() => ParamValue::from("value2"),
                    },
                ),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(evg_config_utils.find_suite_name(&evg_task), "my suite name");
    }

    // find_suite_name tests.
    #[test]
    fn test_find_suite_name_should_use_suite_var_for_non_generated_task_if_it_exists() {
        let evg_task = EvgTask {
            name: "my_task".to_string(),
            commands: Some(vec![
                fn_call("hello world"),
                fn_call_with_params(
                    "run tests",
                    hashmap! {
                        "var1".to_string() => ParamValue::from("value1"),
                        "suite".to_string() => ParamValue::from("my suite name"),
                        "var2".to_string() => ParamValue::from("value2"),
                    },
                ),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(evg_config_utils.find_suite_name(&evg_task), "my suite name");
    }

    #[test]
    fn test_find_suite_name_should_use_task_name_for_generated_task_if_no_var() {
        let evg_task = EvgTask {
            name: "my_task_gen".to_string(),
            commands: Some(vec![
                fn_call("hello world"),
                fn_call_with_params(
                    "generate resmoke tasks",
                    hashmap! {
                        "var1".to_string() => ParamValue::from("value1"),
                        "var2".to_string() => ParamValue::from("value2"),
                    },
                ),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(evg_config_utils.find_suite_name(&evg_task), "my_task");
    }

    #[test]
    fn test_find_suite_name_should_use_task_name_for_non_generated_task_if_no_var() {
        let evg_task = EvgTask {
            name: "my_task".to_string(),
            commands: Some(vec![
                fn_call("hello world"),
                fn_call_with_params(
                    "run_tests",
                    hashmap! {
                        "var1".to_string() => ParamValue::from("value1"),
                        "var2".to_string() => ParamValue::from("value2"),
                    },
                ),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(evg_config_utils.find_suite_name(&evg_task), "my_task");
    }

    // get_task_tags tests.
    #[test]
    fn test_get_task_tags_with_no_tags_should_return_empty_set() {
        let evg_task = EvgTask {
            tags: None,
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert!(evg_config_utils.get_task_tags(&evg_task).is_empty());
    }

    #[test]
    fn test_get_task_tags_with_tags_should_return_tags_in_set() {
        let evg_task = EvgTask {
            tags: Some(vec![
                "tag_0".to_string(),
                "tag_1".to_string(),
                "tag_2".to_string(),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let tags = evg_config_utils.get_task_tags(&evg_task);
        assert_eq!(tags.len(), 3);
        assert!(tags.contains("tag_0"));
        assert!(tags.contains("tag_1"));
        assert!(tags.contains("tag_2"));
    }

    // get_task_dependencies tests.
    #[test]
    fn test_get_task_dependencies_with_no_dependencies_should_return_empty_list() {
        let evg_task = EvgTask {
            depends_on: None,
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert!(evg_config_utils.get_task_dependencies(&evg_task).is_empty());
    }

    #[test]
    fn test_get_task_dependencies_with_dependencies_should_return_list_of_dependencies() {
        let evg_task = EvgTask {
            depends_on: Some(vec![
                TaskDependency {
                    name: "dep0".to_string(),
                    variant: None,
                },
                TaskDependency {
                    name: "dep1".to_string(),
                    variant: None,
                },
                TaskDependency {
                    name: "dep2".to_string(),
                    variant: None,
                },
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let dependencies = evg_config_utils.get_task_dependencies(&evg_task);
        assert_eq!(dependencies.len(), 3);
        assert!(dependencies.contains(&"dep0".to_string()));
        assert!(dependencies.contains(&"dep1".to_string()));
        assert!(dependencies.contains(&"dep2".to_string()));
    }

    // get_gen_task_var tests.

    #[test]
    fn test_get_gen_task_var_should_return_none_if_no_func() {
        let evg_task = EvgTask {
            commands: Some(vec![fn_call("hello world"), fn_call("run tests")]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(
            evg_config_utils
                .get_gen_task_var(&evg_task, "my var")
                .is_none(),
            true
        );
    }

    #[test]
    fn test_get_gen_task_var_should_return_none_if_no_func_vars() {
        let evg_task = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call("generate resmoke tasks"),
                fn_call("run tests"),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(
            evg_config_utils
                .get_gen_task_var(&evg_task, "my var")
                .is_none(),
            true
        );
    }

    #[test]
    fn test_get_gen_task_var_should_return_none_if_missing_var() {
        let evg_task = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call_with_params(
                    "generate resmoke tasks",
                    hashmap! {
                        "var1".to_string() => ParamValue::from("value1"),
                        "var2".to_string() => ParamValue::from("value2"),
                    },
                ),
                fn_call("run tests"),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(
            evg_config_utils
                .get_gen_task_var(&evg_task, "my var")
                .is_none(),
            true
        );
    }

    #[test]
    fn test_get_gen_task_var_should_return_var_value_if_it_exists() {
        let evg_task = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call_with_params(
                    "generate resmoke tasks",
                    hashmap! {
                        "var1".to_string() => ParamValue::from("value1"),
                        "my var".to_string() => ParamValue::from("my value"),
                        "var2".to_string() => ParamValue::from("value2"),
                    },
                ),
                fn_call("run tests"),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(
            evg_config_utils.get_gen_task_var(&evg_task, "my var"),
            Some("my value")
        );
    }

    // get_gen_task_vars tests.
    #[test]
    fn test_get_gen_task_vars_should_return_none_if_no_func() {
        let evg_task = EvgTask {
            commands: Some(vec![fn_call("hello world"), fn_call("run tests")]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(
            evg_config_utils.get_gen_task_vars(&evg_task).is_none(),
            true
        );
    }

    #[test]
    fn test_get_gen_task_vars_should_return_none_if_no_func_vars() {
        let evg_task = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call("generate resmoke tasks"),
                fn_call("run tests"),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(
            evg_config_utils.get_gen_task_vars(&evg_task).is_none(),
            true
        );
    }

    #[test]
    fn test_get_gen_task_vars_should_return_vars_hashmap_if_it_exists() {
        let evg_task = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call_with_params(
                    "generate resmoke tasks",
                    hashmap! {
                        "var1".to_string() => ParamValue::from("value1"),
                        "var2".to_string() => ParamValue::from("value2"),
                    },
                ),
                fn_call("run tests"),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(
            evg_config_utils.get_gen_task_vars(&evg_task),
            Some(hashmap! {
                "var1".to_string() => ParamValue::from("value1"),
                "var2".to_string() => ParamValue::from("value2"),
            })
        );
    }

    // get_multiversion_generate_tasks tests.
    #[test]
    fn test_get_multiversion_generate_tasks_returns_empty_if_init_func_dne() {
        let evg_task = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call(GENERATE_RESMOKE_TASKS),
                fn_call("run tests"),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();
        let multiversion_generate_tasks =
            evg_config_utils.get_multiversion_generate_tasks(&evg_task);
        assert_eq!(multiversion_generate_tasks, None)
    }

    #[test]
    fn test_get_multiversion_generate_tasks_if_init_func_exists() {
        let vars = hashmap! {
                        "mv_suite1_last_continuous".to_string() => ParamValue::from("last-continuous"),
                        "mv_suite1_last_lts".to_string() => ParamValue::from("last-lts"),
        };
        let evg_task = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call_with_params(INITIALIZE_MULTIVERSION_TASKS, vars.clone()),
                fn_call(GENERATE_RESMOKE_TASKS),
                fn_call("run tests"),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();
        let multiversion_generate_tasks =
            evg_config_utils.get_multiversion_generate_tasks(&evg_task);
        let expected_generate_tasks = vec![
            MultiversionGenerateTaskConfig {
                suite_name: "mv_suite1_last_continuous".to_string(),
                old_version: "last-continuous".to_string(),
                bazel_target: None,
            },
            MultiversionGenerateTaskConfig {
                suite_name: "mv_suite1_last_lts".to_string(),
                old_version: "last-lts".to_string(),
                bazel_target: None,
            },
        ];
        assert!(multiversion_generate_tasks
            .unwrap()
            .iter()
            .all(|task| expected_generate_tasks.contains(task)));
    }

    // get_func_vars_by_name tests.
    #[test]
    fn test_get_func_vars_by_name_return_none_if_no_func_exists() {
        let evg_task = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call(GENERATE_RESMOKE_TASKS),
                fn_call("run tests"),
            ]),
            ..Default::default()
        };
        let vars = get_func_vars_by_name(&evg_task, GENERATE_RESMOKE_TASKS);
        assert_eq!(vars, None)
    }

    #[test]
    fn test_get_func_vars_by_name_return_vars_if_func_exists() {
        let vars = hashmap! {
                        "var1".to_string() => ParamValue::from("value1"),
                        "var2".to_string() => ParamValue::from("value2"),
        };
        let evg_task = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call_with_params(GENERATE_RESMOKE_TASKS, vars.clone()),
                fn_call("run tests"),
            ]),
            ..Default::default()
        };
        let extracted_vars = get_func_vars_by_name(&evg_task, GENERATE_RESMOKE_TASKS);
        assert_eq!(extracted_vars.unwrap(), &vars)
    }

    // get_func_by_name tests.
    #[test]
    fn test_get_func_by_name_should_return_function_if_func_exists() {
        let evg_task = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call(GENERATE_RESMOKE_TASKS),
                fn_call("run tests"),
            ]),
            ..Default::default()
        };

        let func = get_func_by_name(&evg_task, GENERATE_RESMOKE_TASKS);

        assert_eq!(
            func.map(|f| &f.func),
            Some(&"generate resmoke tasks".to_string())
        );
    }

    #[test]
    fn test_get_func_by_name_should_return_none_if_no_func_exists() {
        let evg_task = EvgTask {
            commands: Some(vec![fn_call("hello world"), fn_call("run tests")]),
            ..Default::default()
        };

        let func = get_func_by_name(&evg_task, GENERATE_RESMOKE_TASKS);

        assert_eq!(func.is_none(), true);
    }

    // translate_run_var tests

    #[test]
    fn test_value_should_be_returned_if_no_matching_expansion() {
        let build_variant = BuildVariant {
            ..Default::default()
        };
        let run_var = "var";
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let lookup = evg_config_utils.translate_run_var(run_var, &build_variant);

        assert_eq!(lookup, Some(run_var.to_string()));
    }

    #[test]
    fn test_none_should_be_returned_if_expansion_but_not_in_bv_and_no_default() {
        let build_variant = BuildVariant {
            ..Default::default()
        };
        let run_var = r"${expansion}";
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let lookup = evg_config_utils.translate_run_var(run_var, &build_variant);

        assert_eq!(lookup, None);
    }

    #[test]
    fn test_default_should_be_returned_if_expansion_and_default_but_not_in_bv() {
        let build_variant = BuildVariant {
            ..Default::default()
        };
        let run_var = r"${expansion|default}";
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let lookup = evg_config_utils.translate_run_var(run_var, &build_variant);

        assert_eq!(lookup, Some("default".to_string()));
    }

    #[test]
    fn test_bv_value_should_be_returned_if_expansion_and_in_bv() {
        let build_variant = BuildVariant {
            expansions: Some(btreemap! {
                "expansion".to_string() => "build variant value".to_string(),
            }),
            ..Default::default()
        };
        let run_var = r"${expansion|default}";
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let lookup = evg_config_utils.translate_run_var(run_var, &build_variant);

        assert_eq!(lookup, Some("build variant value".to_string()));
    }

    // lookup_build_variant_expansion tests
    #[test]
    fn test_lookup_in_a_build_variant_with_no_expansions_should_return_none() {
        let build_variant = BuildVariant {
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let lookup =
            evg_config_utils.lookup_build_variant_expansion("my expansion", &build_variant);

        assert!(lookup.is_none());
    }

    #[test]
    fn test_lookup_in_a_build_variant_with_missing_expansion_should_return_none() {
        let build_variant = BuildVariant {
            expansions: Some(btreemap! {
                "expansion".to_string() => "build variant value".to_string(),
            }),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let lookup =
            evg_config_utils.lookup_build_variant_expansion("my expansion", &build_variant);

        assert!(lookup.is_none());
    }

    #[test]
    fn test_lookup_in_a_build_variant_with_expected_expansion_should_return_value() {
        let build_variant = BuildVariant {
            expansions: Some(btreemap! {
                "expansion".to_string() => "build variant value".to_string(),
                "my expansion".to_string() => "expansion value".to_string(),
            }),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let lookup =
            evg_config_utils.lookup_build_variant_expansion("my expansion", &build_variant);

        assert_eq!(lookup, Some("expansion value".to_string()));
    }

    // lookup_and_split_by_whitespace_build_variant_expansion tests
    #[test]
    fn test_lookup_and_split_by_whitespace_in_a_build_variant_with_no_expansions_should_return_none(
    ) {
        let build_variant = BuildVariant {
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let lookup = evg_config_utils
            .lookup_and_split_by_whitespace_build_variant_expansion("my expansion", &build_variant);

        assert!(lookup.is_empty());
    }

    #[test]
    fn test_lookup_and_split_by_whitespace_in_a_build_variant_with_missing_expansion_should_return_none(
    ) {
        let build_variant = BuildVariant {
            expansions: Some(btreemap! {
                "expansion".to_string() => "build variant value".to_string(),
            }),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let lookup = evg_config_utils
            .lookup_and_split_by_whitespace_build_variant_expansion("my expansion", &build_variant);

        assert!(lookup.is_empty());
    }

    #[test]
    fn test_lookup_and_split_by_whitespace_in_a_build_variant_with_expected_expansion_should_return_value(
    ) {
        let build_variant = BuildVariant {
            expansions: Some(btreemap! {
                "expansion".to_string() => "build variant value".to_string(),
                "my expansion".to_string() => "expansion value".to_string(),
            }),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let lookup = evg_config_utils
            .lookup_and_split_by_whitespace_build_variant_expansion("my expansion", &build_variant);

        assert_eq!(lookup, vec!["expansion".to_string(), "value".to_string()]);
    }

    // lookup_* tests.
    #[test]
    fn test_lookup_required_should_return_error_if_no_var() {
        let task_def = EvgTask {
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let result = evg_config_utils.lookup_required_param_str(&task_def, "my var");
        assert_eq!(result.is_err(), true);

        let result = evg_config_utils.lookup_required_param_bool(&task_def, "my var");
        assert_eq!(result.is_err(), true);

        let result = evg_config_utils.lookup_required_param_u64(&task_def, "my var");
        assert_eq!(result.is_err(), true);
    }

    #[test]
    fn test_lookup_required_should_return_value_if_it_exists() {
        let task_def = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call_with_params(
                    "generate resmoke tasks",
                    hashmap! {
                        "var_str".to_string() => ParamValue::from("value1"),
                        "var_u64".to_string() => ParamValue::from("12345"),
                        "var_bool".to_string() => ParamValue::from("true"),
                    },
                ),
                fn_call("run tests"),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let result = evg_config_utils.lookup_required_param_str(&task_def, "var_str");
        assert_eq!(result.unwrap(), "value1".to_string());

        let result = evg_config_utils.lookup_required_param_bool(&task_def, "var_bool");
        assert_eq!(result.unwrap(), true);

        let result = evg_config_utils.lookup_required_param_u64(&task_def, "var_u64");
        assert_eq!(result.unwrap(), 12345);
    }

    #[test]
    fn test_lookup_default_should_return_default_if_no_var() {
        let task_def = EvgTask {
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let result = evg_config_utils.lookup_default_param_bool(&task_def, "my var", false);
        assert_eq!(result.unwrap(), false);

        let result =
            evg_config_utils.lookup_default_param_str(&task_def, "my var", "default value");
        assert_eq!(result, "default value");
    }

    #[test]
    fn test_lookup_optional_should_return_none_if_no_var() {
        let task_def = EvgTask {
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let result = evg_config_utils.lookup_optional_param_u64(&task_def, "my var");
        assert_eq!(result.unwrap(), None);
    }

    #[test]
    fn test_lookup_optional_should_return_value_if_it_exists() {
        let task_def = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call_with_params(
                    "generate resmoke tasks",
                    hashmap! {
                        "var_str".to_string() => ParamValue::from("value1"),
                        "var_u64".to_string() => ParamValue::from("12345"),
                        "var_bool".to_string() => ParamValue::from("true"),
                    },
                ),
                fn_call("run tests"),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        let result = evg_config_utils.lookup_optional_param_u64(&task_def, "var_u64");
        assert_eq!(result.unwrap(), Some(12345));
    }

    // tests for is_enterprise_build_variant.
    #[test]
    fn test_build_variant_with_enterprise_module_should_return_true() {
        let build_variant = BuildVariant {
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert!(evg_config_utils.is_enterprise_build_variant(&build_variant));
    }

    #[rstest]
    #[case(Some(vec![]))]
    #[case(Some(vec!["Another Module".to_string(), "Not Enterprise".to_string()]))]
    #[case(None)]
    fn test_build_variant_with_out_enterprise_module_should_return_false(
        #[case] _modules: Option<Vec<String>>,
    ) {
        let build_variant = BuildVariant {
            expansions: Some(BTreeMap::from([(
                "enterprise_test_flag".to_string(),
                "--enableEnterpriseTests=off".to_string(),
            )])),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert!(!evg_config_utils.is_enterprise_build_variant(&build_variant));
    }

    // tests for infer_build_variant_platform
    #[rstest]
    #[case(Some(vec!["rhel80-small".to_string()]), "linux".to_string())]
    #[case(Some(vec!["windows-vsCurrent-small".to_string()]), "windows".to_string())]
    #[case(Some(vec!["macos-1100".to_string()]), "macos".to_string())]
    #[case(Some(vec!["rhel80-small".to_string(), "macos-1100".to_string()]), "linux".to_string())]
    #[case(Some(vec![]), "linux".to_string())]
    fn test_infer_build_variant_platform(
        #[case] distros: Option<Vec<String>>,
        #[case] platform: String,
    ) {
        let build_variant = BuildVariant {
            run_on: distros,
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(
            evg_config_utils.infer_build_variant_platform(&build_variant),
            platform
        );
    }
}
