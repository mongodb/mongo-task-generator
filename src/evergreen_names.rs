//! Names referencing items in the mongodb/mongo etc/evergreen.yml.

// Functions to setup tasks
/// Function setup authentication to evergreen API.
pub const CONFIGURE_EVG_API_CREDS: &str = "configure evergreen api credentials";
/// Function to setup a resmoke task.
pub const DO_SETUP: &str = "do setup";

// Functions for running generated tasks.
/// Function to setup fuzzer.
pub const SETUP_JSTESTFUZZ: &str = "setup jstestfuzz";
/// Function to generated fuzzer tests.
pub const RUN_FUZZER: &str = "run jstestfuzz";
/// Function to run generated tasks.
pub const RUN_GENERATED_TESTS: &str = "run generated tests";

// Function for multi-version tests.
/// Function to do setup for multi-version testing.
pub const DO_MULTIVERSION_SETUP: &str = "do multiversion setup";
/// Function to get the project with no modules.
pub const GET_PROJECT_WITH_NO_MODULES: &str = "git get project no modules";
/// Function to add a git tag.
pub const ADD_GIT_TAG: &str = "add git tag";

// Functions for generating tasks.
pub const GENERATE_RESMOKE_TASKS: &str = "generate resmoke tasks";

// Tasks
/// Task which creates artifacts needed to execute tests.
pub const ARTIFACT_CREATION_TASK: &str = "archive_dist_test_debug";
/// Name of display task to hide all "_gen" tasks behind.
pub const GENERATOR_TASKS: &str = "generator_tasks";

// Vars
/// Variable that indicates a task is a fuzzer.
pub const IS_FUZZER: &str = "is_jstestfuzz";
/// If true, generate sub-tasks to run on large distros.
pub const USE_LARGE_DISTRO: &str = "use_large_distro";

// Parameters
// Shared parameters between fuzzers and resmoke.
/// Is multiversion setup required to execute this task.
pub const REQUIRE_MULTIVERSION_SETUP: &str = "require_multiversion_setup";
/// Arguments to pass to resmoke command.
pub const RESMOKE_ARGS: &str = "resmoke_args";
/// Name of suite being executed.
pub const SUITE_NAME: &str = "suite";
/// Location where generation task configuration is stored in S3.
pub const GEN_TASK_CONFIG_LOCATION: &str = "gen_task_config_location";
/// Maximum amount of resmoke jobs to execute in parallel.
pub const RESMOKE_JOBS_MAX: &str = "resmoke_jobs_max";

// Fuzzer parameters.
/// Name of npm command to run.
pub const NPM_COMMAND: &str = "npm_command";
/// Parameters to pass to fuzzer command.
pub const FUZZER_PARAMETERS: &str = "jstestfuzz_vars";
/// Should test execution continue after a failure.
pub const CONTINUE_ON_FAILURE: &str = "continue_on_failure";
/// Should test order to shuffled for execution.
pub const SHOULD_SHUFFLE_TESTS: &str = "should_shuffle";
/// Name of task being executed.
pub const TASK_NAME: &str = "task";
/// Idle timeout to set for execution.
pub const IDLE_TIMEOUT: &str = "timeout_secs";
/// Multiversion version combination being run against.
pub const MULTIVERSION_EXCLUDE_TAGS: &str = "multiversion_exclude_tags_version";

// Build Variant expansions.
/// Name of large distro for build variant.
pub const LARGE_DISTRO_EXPANSION: &str = "large_distro_name";
