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
    sub_task_index: Option<u64>,
    total_tasks: Option<u64>,
) -> String {
    if let Some(index) = sub_task_index {
        let total_tasks = total_tasks.unwrap();
        let alignment = (total_tasks as f64).log10().ceil() as usize;
        format!("{}_{:0fill$}", display_name, index, fill = alignment)
    } else {
        format!("{}_misc", display_name)
    }
}

/// Remove the '_gen' from end of the given task name if it exists.
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
    #[case("task", Some(0), Some(10), "task_0")]
    #[case("task", Some(42), Some(1001), "task_0042")]
    #[case("task", None, Some(1001), "task_misc")]
    #[case("task", None, None, "task_misc")]
    fn test_name_generated_task_should_not_include_suffix(
        #[case] name: &str,
        #[case] index: Option<u64>,
        #[case] total: Option<u64>,
        #[case] expected: &str,
    ) {
        let task_name = name_generated_task(name, index, total);

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
