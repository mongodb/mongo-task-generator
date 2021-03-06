//! Utilities for working with task names.

use crate::evergreen_names::ENTERPRISE_MODULE;
const GEN_SUFFIX: &str = "_gen";

/// Generate a name for a generated task.
///
/// # Arguments
///
/// * `parent_name` - Name of task parent task being generated.
/// * `task_index` - Index of sub-task being named.
/// * `total_tasks` - Total number of sub-tasks generated for this parent task.
/// * `variant` - Build Variant being generated.
pub fn name_generated_task(
    display_name: &str,
    sub_task_index: Option<usize>,
    total_tasks: usize,
    is_enterprise: bool,
) -> String {
    let suffix = if is_enterprise {
        format!("-{}", ENTERPRISE_MODULE)
    } else {
        "".to_string()
    };

    if let Some(index) = sub_task_index {
        let alignment = (total_tasks as f64).log10().ceil() as usize;
        format!(
            "{}_{:0fill$}{}",
            display_name,
            index,
            suffix,
            fill = alignment
        )
    } else {
        format!("{}_misc{}", display_name, suffix)
    }
}

/// Remove the '_gen' from end of the given task name if it exists.
///
/// # Arguments
///
/// * `task_name` - Name of task.
///
/// # Returns
///
/// Name of task with `_gen` stripped off.
pub fn remove_gen_suffix(task_name: &str) -> &str {
    if task_name.ends_with(GEN_SUFFIX) {
        let end = task_name.len() - GEN_SUFFIX.len();
        &task_name[..end]
    } else {
        task_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;

    #[rstest]
    #[case("task", Some(0), 10, false, "task_0")]
    #[case("task", Some(42), 1001, false, "task_0042")]
    #[case("task", None, 1001, false, "task_misc")]
    #[case("task", None, 0, false, "task_misc")]
    #[case("task", Some(0), 10, true, "task_0-enterprise")]
    #[case("task", Some(42), 1001, true, "task_0042-enterprise")]
    #[case("task", None, 1001, true, "task_misc-enterprise")]
    #[case("task", None, 0, true, "task_misc-enterprise")]
    fn test_name_generated_task_should_not_include_suffix(
        #[case] name: &str,
        #[case] index: Option<usize>,
        #[case] total: usize,
        #[case] is_enterprise: bool,
        #[case] expected: &str,
    ) {
        let task_name = name_generated_task(name, index, total, is_enterprise);

        assert_eq!(task_name, expected);
    }

    #[rstest]
    #[case("task_name", "task_name")]
    #[case("task_name_gen", "task_name")]
    #[case("task_name_", "task_name_")]
    fn test_remove_gen_suffix(#[case] original_task: &str, #[case] expected_task: &str) {
        assert_eq!(remove_gen_suffix(original_task), expected_task);
    }
}
