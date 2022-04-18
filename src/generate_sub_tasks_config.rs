use std::{collections::HashSet, path::Path};

use anyhow::Result;
use serde::Deserialize;

#[derive(Deserialize, Debug, Clone)]
pub struct GenerateSubTasksConfig {
    pub build_variant_large_distro_exceptions: HashSet<String>,
}

impl GenerateSubTasksConfig {
    pub fn from_yaml_file<P: AsRef<Path>>(location: P) -> Result<Self> {
        let contents = std::fs::read_to_string(location)?;

        Ok(serde_yaml::from_str(&contents)?)
    }

    pub fn ignore_missing_large_distro(&self, build_variant_name: &str) -> bool {
        self.build_variant_large_distro_exceptions
            .contains(build_variant_name)
    }
}
