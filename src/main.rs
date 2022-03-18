use std::{
    path::{Path, PathBuf},
    process::exit,
    time::Instant,
};

use anyhow::Result;
use clap::Parser;
use mongo_task_generator::{generate_configuration, Dependencies};
use serde::Deserialize;
use tracing::{event, Level};
use tracing_subscriber::fmt::format;

/// Expansions from evergreen to determine settings for how task should be generated.
#[derive(Debug, Deserialize)]
struct EvgExpansions {
    /// Evergreen project being run.
    pub project: String,
    /// Git revision being run against.
    pub revision: String,
    /// ID of Evergreen version running.
    pub version_id: String,
}

impl EvgExpansions {
    /// Read evergreen expansions from the given yaml file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to YAML file to read.
    pub fn from_yaml_file(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&contents)?)
    }

    /// File to store generated configuration under.
    pub fn config_location(&self) -> String {
        format!(
            "{}/{}/generate_tasks/generated-config-{}.tgz",
            self.project, self.revision, self.version_id
        )
    }
}

#[derive(Parser, Debug)]
struct Args {
    /// File containing evergreen project configuration.
    #[clap(long, parse(from_os_str))]
    evg_project_file: PathBuf,

    /// File containing expansions that impact task generation.
    #[clap(long, parse(from_os_str))]
    expansion_file: PathBuf,

    /// File with information on how to authenticate against the evergreen API.
    #[clap(long, parse(from_os_str))]
    evg_auth_file: PathBuf,
}

/// Configure logging for the command execution.
fn configure_logging() {
    let format = format::json();
    let subscriber = tracing_subscriber::fmt().event_format(format).finish();

    tracing::subscriber::set_global_default(subscriber).unwrap();
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    configure_logging();

    let evg_expansions = EvgExpansions::from_yaml_file(&args.expansion_file)
        .expect("Error reading expansions file.");
    let deps = Dependencies::new(
        &args.evg_project_file,
        &evg_expansions.project,
        &args.evg_auth_file,
    )
    .unwrap();

    let start = Instant::now();
    let result = generate_configuration(&deps, &evg_expansions.config_location()).await;
    event!(
        Level::INFO,
        "generation completed: {} seconds",
        duration_secs = start.elapsed().as_secs()
    );
    if let Err(err) = result {
        eprintln!("Error encountered during execution: {:?}", err);
        exit(1);
    }
}
