use std::{
    path::{Path, PathBuf},
    process::exit,
    time::Instant,
};

use anyhow::Result;
use clap::Parser;
use mongo_task_generator::{
    build_s3_client, generate_configuration, Dependencies, ExecutionConfiguration, ProjectInfo,
    SubtaskLimits,
};
use serde::Deserialize;
use tracing::{error, event, Level};
use tracing_subscriber::fmt::format;

const DEFAULT_EVG_AUTH_FILE: &str = "~/.evergreen.yml";
const DEFAULT_EVG_PROJECT_FILE: &str = "etc/evergreen.yml";
const DEFAULT_RESMOKE_COMMAND: &str = "python buildscripts/resmoke.py";
const DEFAULT_BURN_IN_TESTS_COMMAND: &str = "python buildscripts/burn_in_tests.py run";
const DEFAULT_TARGET_DIRECTORY: &str = "generated_resmoke_config";
const DEFAULT_S3_TEST_STATS_BUCKET: &str = "mongo-test-stats";
const DEFAULT_MAX_SUBTASKS_PER_TASK: &str = "10";
const DEFAULT_DEFAULT_SUBTASKS_PER_TASKS: &str = "5";
const DEFAULT_TEST_RUNTIME_PER_REQUIRED_SUBTASK: &str = "3600";
const DEFAULT_LARGE_REQUIRED_TASK_RUNTIME_THRESHOLD: &str = "7200";

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
    /// True if the patch is a patch build.
    #[serde(default, deserialize_with = "deserialize_bool_string")]
    pub is_patch: bool,
    /// True if we should NOT skip tests covered by more complex suites.
    #[serde(default, deserialize_with = "deserialize_bool_string")]
    pub run_covered_tests: bool,
}

// The boolean YAML fields `is_patch` and `run_covered_tests` are set to the
// string "true" rather than a boolean `true`. Therefore we need a custom
// deserializer to convert from the string "true" to the boolean `true`, and
// in all other cases return `false`.
fn deserialize_bool_string<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: &str = serde::Deserialize::deserialize(deserializer)?;
    match s {
        "true" => Ok(true),
        _ => Ok(false),
    }
}

impl EvgExpansions {
    /// Read evergreen expansions from the given yaml file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to YAML file to read.
    pub fn from_yaml_file(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;

        let evg_expansions: Result<Self, serde_yaml::Error> = serde_yaml::from_str(&contents);
        if evg_expansions.is_err() {
            error!(
                file = path.display().to_string(),
                contents = &contents,
                "Failed to parse yaml for EvgExpansions from file",
            );
        }

        Ok(evg_expansions?)
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
    #[clap(long, value_parser, default_value = DEFAULT_EVG_PROJECT_FILE)]
    evg_project_file: PathBuf,

    /// File containing expansions that impact task generation.
    #[clap(long, value_parser)]
    expansion_file: PathBuf,

    /// File with information on how to authenticate against the evergreen API.
    #[clap(long, value_parser, default_value = DEFAULT_EVG_AUTH_FILE)]
    evg_auth_file: PathBuf,

    /// Directory to write generated configuration files.
    #[clap(long, value_parser, default_value = DEFAULT_TARGET_DIRECTORY)]
    target_directory: PathBuf,

    /// Disable evergreen task-history queries and use task splitting fallback.
    #[clap(long)]
    use_task_split_fallback: bool,

    /// Command to invoke resmoke.
    #[clap(long, default_value = DEFAULT_RESMOKE_COMMAND)]
    resmoke_command: String,

    /// If the generator should include tests that are tagged with fully disabled features.
    #[clap(long)]
    include_fully_disabled_feature_tests: bool,

    /// File containing configuration for generating sub-tasks.
    #[clap(long, value_parser)]
    generate_sub_tasks_config: Option<PathBuf>,

    /// Generate burn_in related tasks.
    #[clap(long)]
    burn_in: bool,

    /// Command to invoke burn_in_tests.
    #[clap(long, default_value = DEFAULT_BURN_IN_TESTS_COMMAND)]
    burn_in_tests_command: String,

    /// S3 bucket to get test stats from.
    #[clap(long, default_value = DEFAULT_S3_TEST_STATS_BUCKET)]
    s3_test_stats_bucket: String,

    // Ideal total test runtime (in seconds) for individual subtasks on required
    // variants, used to determine the number of subtasks for tasks on required variants.
    #[clap(long, default_value = DEFAULT_TEST_RUNTIME_PER_REQUIRED_SUBTASK)]
    test_runtime_per_required_subtask: f64,

    // Threshold of total test runtime (in seconds) for a required task to be considered
    // large enough to warrant splitting into more that the default number of tasks.
    #[clap(long, default_value = DEFAULT_LARGE_REQUIRED_TASK_RUNTIME_THRESHOLD)]
    large_required_task_runtime_threshold: f64,

    // Default number of subtasks that should be generated for tasks
    #[clap(long, default_value = DEFAULT_DEFAULT_SUBTASKS_PER_TASKS)]
    default_subtasks_per_task: usize,

    // Maximum number of subtasks that can be generated for tasks
    #[clap(long, default_value = DEFAULT_MAX_SUBTASKS_PER_TASK)]
    max_subtasks_per_task: usize,
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
        skip_covered_tests: evg_expansions.is_patch && !evg_expansions.run_covered_tests,
        include_fully_disabled_feature_tests: args.include_fully_disabled_feature_tests,
        burn_in_tests_command: &args.burn_in_tests_command,
        s3_test_stats_bucket: &args.s3_test_stats_bucket,
        subtask_limits: SubtaskLimits {
            test_runtime_per_required_subtask: args.test_runtime_per_required_subtask,
            max_subtasks_per_task: args.max_subtasks_per_task,
            default_subtasks_per_task: args.default_subtasks_per_task,
            large_required_task_runtime_threshold: args.large_required_task_runtime_threshold,
        },
    };
    let s3_client = build_s3_client().await;
    let deps = Dependencies::new(execution_config, s3_client).unwrap();

    let start = Instant::now();
    let result = generate_configuration(&deps, &args.target_directory).await;
    event!(
        Level::INFO,
        "generation completed: {duration_secs} seconds",
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
