use shrub_rs::models::{
    task::{EvgTask, TaskRef},
    variant::DisplayTask,
};

/// Interface for representing a generated task.
pub trait GeneratedSuite {
    /// Get the display name to use for the generated task.
    fn display_name(&self) -> String;
    /// Get the list of sub-tasks that comprise the generated task.
    fn sub_tasks(&self) -> Vec<EvgTask>;

    /// Build a shrub display task for this generated task.
    fn build_display_task(&self) -> DisplayTask {
        DisplayTask {
            name: self.display_name(),
            execution_tasks: self
                .sub_tasks()
                .iter()
                .map(|s| s.name.to_string())
                .collect(),
        }
    }

    /// Build a shrub task reference for this generated task.
    fn build_task_ref(&self) -> Vec<TaskRef> {
        self.sub_tasks()
            .iter()
            .map(|s| s.get_reference(None, Some(false)))
            .collect()
    }
}
