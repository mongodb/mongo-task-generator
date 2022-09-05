use std::{
    path::{Path, PathBuf},
    process::exit,
    time::Instant,
};

use anyhow::Result;
use clap::Parser;
use mongo_task_generator::{
    generate_configuration, Dependencies, ExecutionConfiguration, ProjectInfo,
};
use serde::Deserialize;
use tracing::{event, Level};
use tracing_subscriber::fmt::format;

const DEFAULT_EVG_AUTH_FILE: &str = "~/.evergreen.yml";
const DEFAULT_EVG_PROJECT_FILE: &str = "etc/evergreen.yml";
const DEFAULT_RESMOKE_COMMAND: &str = "python buildscripts/resmoke.py";
const DEFAULT_BURN_IN_TESTS_COMMAND: &str = "python buildscripts/burn_in_tests.py";
const DEFAULT_TARGET_DIRECTORY: &str = "generated_resmoke_config";

/// Expansions from evergreen to determine settings for how task should be generated.
#[derive(Debug, Deserialize)]
struct EvgExpansions {
    /// Evergreen project being run.
    pub project: String,
    /// Git revision being run against.
    pub revision: String,
    /// Name of task running generator.
    pub task_name: String,
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
    #[clap(long, parse(from_os_str), default_value = DEFAULT_EVG_PROJECT_FILE)]
    evg_project_file: PathBuf,

    /// File containing expansions that impact task generation.
    #[clap(long, parse(from_os_str))]
    expansion_file: PathBuf,

    /// File with information on how to authenticate against the evergreen API.
    #[clap(long, parse(from_os_str), default_value = DEFAULT_EVG_AUTH_FILE)]
    evg_auth_file: PathBuf,

    /// Directory to write generated configuration files.
    #[clap(long, parse(from_os_str), default_value = DEFAULT_TARGET_DIRECTORY)]
    target_directory: PathBuf,

    /// Disable evergreen task-history queries and use task splitting fallback.
    #[clap(long)]
    use_task_split_fallback: bool,

    /// Command to invoke resmoke.
    #[clap(long, default_value = DEFAULT_RESMOKE_COMMAND)]
    resmoke_command: String,

    /// File containing configuration for generating sub-tasks.
    #[clap(long, parse(from_os_str))]
    generate_sub_tasks_config: Option<PathBuf>,

    /// Generate burn_in related tasks.
    #[clap(long)]
    burn_in: bool,

    /// Command to invoke burn_in_tests.
    #[clap(long, default_value = DEFAULT_BURN_IN_TESTS_COMMAND)]
    burn_in_tests_command: String,
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

    let gen_sub_tasks_config_file = &args.generate_sub_tasks_config.map(|p| expand_path(&p));
    let evg_expansions = EvgExpansions::from_yaml_file(&args.expansion_file)
        .expect("Error reading expansions file.");
    let project_info = ProjectInfo::new(
        &args.evg_project_file,
        &evg_expansions.project,
        gen_sub_tasks_config_file.as_ref(),
    );
    let execution_config = ExecutionConfiguration {
        project_info: &project_info,
        evg_auth_file: &expand_path(&args.evg_auth_file),
        use_task_split_fallback: args.use_task_split_fallback,
        resmoke_command: &args.resmoke_command,
        target_directory: &expand_path(&args.target_directory),
        generating_task: &evg_expansions.task_name,
        config_location: &evg_expansions.config_location(),
        gen_burn_in: args.burn_in,
        burn_in_tests_command: &args.burn_in_tests_command,
    };
    let deps = Dependencies::new(execution_config).unwrap();

    let start = Instant::now();
    let result = generate_configuration(&deps, &args.target_directory).await;
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

/// Expand ~ and any environment variables in the given path.
///
/// # Arguments
///
/// * `path` - Path to expand.
///
/// # Returns
///
/// Path with ~ and environment variables expanded.
fn expand_path(path: &Path) -> PathBuf {
    let path_str = path.to_str().unwrap();
    let expanded = shellexpand::full(path_str).unwrap();
    PathBuf::from(expanded.to_string())
}
