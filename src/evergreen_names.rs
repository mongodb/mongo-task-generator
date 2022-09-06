//! Names referencing items in the mongodb/mongo etc/evergreen.yml.

// Module Names
/// Name of enterprise module.
pub const ENTERPRISE_MODULE: &str = "enterprise";

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

// Functions for invoking resmoke.py in a generated or non-generated task.
pub const RUN_RESMOKE_TESTS: &str = "run tests";

// Tasks
/// Name of display task to hide all "_gen" tasks behind.
pub const GENERATOR_TASKS: &str = "generator_tasks";
/// Name of burn_in_tests task.
pub const BURN_IN_TESTS: &str = "burn_in_tests_gen";
/// Name of burn_in_tags task.
pub const BURN_IN_TAGS: &str = "burn_in_tags_gen";

// Vars
/// Variable that indicates a task is a fuzzer.
pub const IS_FUZZER: &str = "is_jstestfuzz";
/// If true, generate sub-tasks to run on large distros.
pub const USE_LARGE_DISTRO: &str = "use_large_distro";
/// Number of files that each fuzzer sub-task should generate.
pub const NUM_FUZZER_FILES: &str = "num_files";
/// Number of sub-tasks that should be generated for a fuzzer.
pub const NUM_FUZZER_TASKS: &str = "num_tasks";
/// Tag to exclude multiversion version.
pub const MULTIVERSION_EXCLUDE_TAG: &str = "multiversion_exclude_tags_version";

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
/// Number of times to repeat a given resmoke suite.
pub const REPEAT_SUITES: &str = "resmoke_repeat_suites";

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
/// List of build variant names delimited by spaces to generate burn_in_tags for.
pub const BURN_IN_TAG_BUILD_VARIANTS: &str = "burn_in_tag_buildvariants";
/// The distro to use when compiling burn_in_tags.
pub const BURN_IN_TAG_COMPILE_DISTRO: &str = "burn_in_tag_compile_distro";
/// The name to give the task group for compiling.
pub const BURN_IN_TAG_COMPILE_TASK_GROUP_NAME: &str = "burn_in_tag_compile_task_group_name";
/// Name of build variant to determine the timeouts for.
pub const BURN_IN_BYPASS: &str = "burn_in_bypass";

// Task Tags
/// Tag to include multiversion setup is required.
pub const MULTIVERSION: &str = "multiversion";
/// Tag to indicate multiversion combination should not be created.
pub const NO_MULTIVERSION_ITERATION: &str = "no_version_combination";

// Multiversion values
/// Tag to include required backport.
pub const BACKPORT_REQUIRED_TAG: &str = "backport_required_multiversion";
/// Tag to mark task multiversion incompatible.
pub const MULTIVERSION_INCOMPATIBLE: &str = "multiversion_incompatible";
/// Filename of multiversion exclude tags file.
pub const MULTIVERSION_EXCLUDE_TAGS_FILE: &str = "multiversion_exclude_tags.yml";
/// Name of last lts configuration.
pub const MULTIVERSION_LAST_LTS: &str = "last_lts";
/// Name of last continuous configuration.
pub const MULTIVERSION_LAST_CONTINUOUS: &str = "last_continuous";

// Distro group names
/// Windows distro group name.
pub const WINDOWS: &str = "windows";
/// MacOS distro group name.
pub const MACOS: &str = "macos";
/// Linux distro group name.
pub const LINUX: &str = "linux";
