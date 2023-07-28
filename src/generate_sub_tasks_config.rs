use std::{collections::HashSet, path::Path};
use tracing::error;

use anyhow::Result;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct GenerateSubTasksConfig {
    pub build_variant_large_distro_exceptions: HashSet<String>,
}

impl GenerateSubTasksConfig {
    pub fn from_yaml_file<P: AsRef<Path>>(location: P) -> Result<Self> {
        let contents = std::fs::read_to_string(&location)?;

        let subtasks: Result<Self, serde_yaml::Error> = serde_yaml::from_str(&contents);
        if subtasks.is_err() {
            error!(
                file = location.as_ref().display().to_string(),
                contents = &contents,
                "Failed to parse yaml for GenerateSubTasksConfig from file",
            );
        }

        Ok(subtasks?)
    }

    pub fn ignore_missing_large_distro(&self, build_variant_name: &str) -> bool {
        self.build_variant_large_distro_exceptions
            .contains(build_variant_name)
    }
}
