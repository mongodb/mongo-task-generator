use shrub_rs::models::{
    task::{EvgTask, TaskRef},
    variant::DisplayTask,
};

/// Definition of a generated sub task.
#[derive(Clone, Debug, Default)]
pub struct GeneratedSubTask {
    /// Definition of an Evergreen task.
    pub evg_task: EvgTask,
    /// Distro this task should run on.
    pub distro: Option<String>,
}

/// Interface for representing a generated task.
pub trait GeneratedSuite: Sync + Send {
    /// Get the display name to use for the generated task.
    fn display_name(&self) -> String;

    /// Get the list of sub-tasks that comprise the generated task.
    fn sub_tasks(&self) -> Vec<GeneratedSubTask>;

    /// Build a shrub display task for this generated task.
    fn build_display_task(&self) -> DisplayTask {
        DisplayTask {
            name: self.display_name(),
            execution_tasks: self
                .sub_tasks()
                .iter()
                .map(|s| s.evg_task.name.to_string())
                .collect(),
        }
    }

    /// Build a shrub task reference for this generated task.
    fn build_task_ref(&self) -> Vec<TaskRef> {
        self.sub_tasks()
            .iter()
            .map(|s| {
                s.evg_task
                    .get_reference(s.distro.clone().map(|d| vec![d]), Some(false))
            })
            .collect()
    }
}
