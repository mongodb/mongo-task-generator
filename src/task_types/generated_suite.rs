use shrub_rs::models::{
    task::{EvgTask, TaskRef},
    variant::DisplayTask,
};

/// Definition of a generated sub task.
#[derive(Clone, Debug, Default)]
pub struct GeneratedSubTask {
    /// Definition of an Evergreen task.
    pub evg_task: EvgTask,
    /// Whether to run generated task on a large distro.
    pub use_large_distro: bool,
    /// Whether to run generated task on a xlarge distro.
    pub use_xlarge_distro: bool,
}

/// Interface for representing a generated task.
pub trait GeneratedSuite: Sync + Send {
    /// Get the display name to use for the generated task.
    fn display_name(&self) -> String;

    /// Get the list of sub-tasks that comprise the generated task.
    fn sub_tasks(&self) -> Vec<GeneratedSubTask>;

    /// Check whether any sub task requires large distro.
    fn use_large_distro(&self) -> bool {
        self.sub_tasks()
            .iter()
            .any(|sub_task| sub_task.use_large_distro)
    }

    /// Check whether any sub task requires xlarge distro.
    fn use_xlarge_distro(&self) -> bool {
        self.sub_tasks()
            .iter()
            .any(|sub_task| sub_task.use_xlarge_distro)
    }

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
    fn build_task_ref(&self, distro: Option<String>) -> Vec<TaskRef> {
        self.sub_tasks()
            .iter()
            .map(|sub_task| {
                let mut large_distro = None;
                if sub_task.use_large_distro || sub_task.use_xlarge_distro {
                    large_distro = distro.clone();
                }
                let mut task_ref = sub_task
                    .evg_task
                    .get_reference(large_distro.map(|d| vec![d]), Some(false));

                match &sub_task.evg_task.depends_on {
                    Some(deps)
                        if deps
                            .iter()
                            .any(|dep| dep.name == "multiversion_binary_search") =>
                    {
                        task_ref.depends_on = sub_task.evg_task.depends_on.clone()
                    }
                    _ => (),
                }
                return task_ref;
            })
            .collect()
    }
}
