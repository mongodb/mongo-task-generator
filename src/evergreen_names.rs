//! Names referencing items in the 10gen/mongo etc/evergreen.yml.

// Module Names
/// Name of enterprise module.
pub const ENTERPRISE_MODULE: &str = "enterprise";

// Functions to setup tasks
/// Function setup authentication to evergreen API.
pub const CONFIGURE_EVG_API_CREDS: &str = "configure evergreen api credentials";
/// Function to setup a resmoke task.
pub const DO_SETUP: &str = "do setup";
pub const RETRIEVE_GENERATED_TEST_CONFIG: &str = "retrieve generated test configuration";
pub const EXTRACT_GENERATED_TEST_CONFIG: &str = "extract generated test configuration";
pub const GET_ENGFLOW_CREDS: &str = "get engflow creds";

// Functions for running generated tasks.
/// Function to setup fuzzer.
pub const SETUP_JSTESTFUZZ: &str = "setup jstestfuzz";
/// Function to generated fuzzer tests.
pub const RUN_FUZZER: &str = "run jstestfuzz";
/// Function to run generated tasks.
pub const RUN_GENERATED_TESTS: &str = "run generated tests";
pub const BAZEL_TEST: &str = "bazel test";

// Function for multi-version tests.
/// Function to do setup for multi-version testing.
pub const DO_MULTIVERSION_SETUP: &str = "do multiversion setup";
/// Function to get the project with no modules.
pub const GET_PROJECT_WITH_NO_MODULES: &str = "git get project no modules";
/// Function to add a git tag.
pub const ADD_GIT_TAG: &str = "add git tag";
pub const GET_PROJECT_AND_ADD_TAG: &str = "git get project and add git tag";

// Noop function which stores multiversion task data.
pub const INITIALIZE_MULTIVERSION_TASKS: &str = "initialize multiversion tasks";
//
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
/// Name of burn_in_tasks task.
pub const BURN_IN_TASKS: &str = "burn_in_tasks_gen";
/// Name of multiversion binary selection task.
pub const MULTIVERSION_BINARY_SELECTION: &str = "select_multiversion_binaries";

// Vars
/// Variable that indicates a task is a fuzzer.
pub const IS_FUZZER: &str = "is_jstestfuzz";
/// If true, generate sub-tasks to run on large distros.
pub const USE_LARGE_DISTRO: &str = "use_large_distro";
/// If true, generate sub-tasks to run on large distros.
pub const USE_XLARGE_DISTRO: &str = "use_xlarge_distro";
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
/// Variant used for compile.
pub const COMPILE_VARIANT: &str = "compile_variant";

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
/// Name of xlarge distro for build variant.
pub const XLARGE_DISTRO_EXPANSION: &str = "xlarge_distro_name";
/// List of build variant names delimited by spaces to generate burn_in_tests for.
pub const BURN_IN_TAG_INCLUDE_BUILD_VARIANTS: &str = "burn_in_tag_include_build_variants";
/// Generate burn_in_tests for all required and suggested build variants.
pub const BURN_IN_TAG_INCLUDE_ALL_REQUIRED_AND_SUGGESTED: &str =
    "burn_in_tag_include_all_required_and_suggested";
/// Build variants to exclude when burn_in_required_and_suggested_build_variants is set.
pub const BURN_IN_TAG_EXCLUDE_BUILD_VARIANTS: &str = "burn_in_tag_exclude_build_variants";
/// Compile task name generated build variant should depend on.
pub const BURN_IN_TAG_COMPILE_TASK_DEPENDENCY: &str = "burn_in_tag_compile_task_dependency";
/// Name of build variant to determine the timeouts for.
pub const BURN_IN_BYPASS: &str = "burn_in_bypass";
/// List of tasks to burn in.
pub const BURN_IN_TASK_NAME: &str = "burn_in_task_name";
/// Variant specific override of last_versions in the multiversion-config
pub const LAST_VERSIONS_EXPANSION: &str = "last_versions";
/// Unique identifier for generated tasks to use that override last_versions
pub const UNIQUE_GEN_SUFFIX_EXPANSION: &str = "unique_gen_suffix";

// Task Tags
/// Tag to include multiversion setup is required.
pub const MULTIVERSION: &str = "multiversion";
/// Tag to indicate multiversion combination should not be created.
pub const NO_MULTIVERSION_GENERATE_TASKS: &str = "no_multiversion_generate_tasks";

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

// Constants in evergreen.yml.
/// Name of the variant that calls generate.task on the version.
pub const VERSION_GEN_VARIANT: &str = "generate-tasks-for-version";
/// Name of the task that calls generate.task on the version for burn-in.
pub const VERSION_BURN_IN_GEN_TASK: &str = "version_burn_in_gen";
