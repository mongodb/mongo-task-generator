// Configuration for generating evergreen tasks.

#[derive(Debug, Clone)]
pub struct GenerateConfig {
    /// List of tasks generated tasks should be dependent on.
    pub dependencies: Vec<String>,

    /// Max number of sub-tasks to split a suite into.
    pub max_sub_tasks_per_task: usize,

    /// Disable evergreen task-history queries and use task splitting fallback.
    pub use_task_split_fallback: bool,
}
