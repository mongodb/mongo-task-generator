use std::collections::HashSet;

use anyhow::{bail, Result};
use lazy_static::lazy_static;
use regex::Regex;
use shrub_rs::models::commands::EvgCommand::Function;
use shrub_rs::models::params::ParamValue;
use shrub_rs::models::{commands::FunctionCall, task::EvgTask, variant::BuildVariant};

use crate::evergreen_names::{GENERATE_RESMOKE_TASKS, IS_FUZZER};
use crate::utils::task_name::remove_gen_suffix;

lazy_static! {
    /// Regular expression for finding expansions.
    ///   `${expansion}` or `${expansion|default_value}`
    static ref EXPANSION_RE: Regex =
        Regex::new(r"\$\{(?P<id>[a-zA-Z0-9_]+)(\|(?P<default>.*))?}").unwrap();
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
        let suite = self.get_gen_task_var(task, "suite");
        if let Some(suite) = suite {
            suite
        } else {
            remove_gen_suffix(&task.name)
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
        let generate_func = get_generate_resmoke_func(task);
        if let Some(func) = generate_func {
            if let Some(vars) = &func.vars {
                if let Some(ParamValue::String(value)) = vars.get(var) {
                    return Some(value);
                }
            }
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
}

/// Get the shrub function make the 'generate resmoke task' call in the given task.
///
/// # Arguments
///
/// * `task` - Shrub task to query.
///
/// # Returns
///
/// Function call to 'generate resmoke task'.
fn get_generate_resmoke_func(task: &EvgTask) -> Option<&FunctionCall> {
    let command = if let Some(commands) = &task.commands {
        commands.iter().find(|c| {
            if let Function(func) = c {
                if func.func == GENERATE_RESMOKE_TASKS {
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

#[cfg(test)]
mod tests {
    use maplit::btreemap;
    use maplit::hashmap;
    use shrub_rs::models::commands::{fn_call, fn_call_with_params};
    use shrub_rs::models::params::ParamValue;

    use super::*;

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
                fn_call("run tests"),
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
                fn_call("run tests"),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(evg_config_utils.is_task_fuzzer(&evg_task), true);
    }

    // find_suite_name tests.
    #[test]
    fn test_find_suite_name_should_use_suite_var_if_it_exists() {
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
                fn_call("run tests"),
            ]),
            ..Default::default()
        };
        let evg_config_utils = EvgConfigUtilsImpl::new();

        assert_eq!(evg_config_utils.find_suite_name(&evg_task), "my suite name");
    }

    #[test]
    fn test_find_suite_name_should_use_task_name_if_no_var() {
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
                fn_call("run tests"),
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

    // get_gen_task_vars tests.

    #[test]
    fn test_get_gen_task_vars_should_return_none_if_no_func() {
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
            evg_config_utils
                .get_gen_task_var(&evg_task, "my var")
                .is_none(),
            true
        );
    }

    #[test]
    fn test_get_gen_task_vars_should_return_none_if_missing_var() {
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
    fn test_get_gen_task_vars_should_return_var_value_if_it_exists() {
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

    // get_generated_resmoke_func tests.
    #[test]
    fn test_get_generated_resmoke_func_should_return_resmoke_function() {
        let evg_task = EvgTask {
            commands: Some(vec![
                fn_call("hello world"),
                fn_call("generate resmoke tasks"),
                fn_call("run tests"),
            ]),
            ..Default::default()
        };

        let func = get_generate_resmoke_func(&evg_task);

        assert_eq!(
            func.map(|f| &f.func),
            Some(&"generate resmoke tasks".to_string())
        );
    }

    #[test]
    fn test_get_generated_resmoke_func_should_return_none_if_no_func_exists() {
        let evg_task = EvgTask {
            commands: Some(vec![fn_call("hello world"), fn_call("run tests")]),
            ..Default::default()
        };

        let func = get_generate_resmoke_func(&evg_task);

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
}
