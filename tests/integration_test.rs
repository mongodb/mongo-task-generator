use assert_cmd::Command;
use rstest::rstest;
use std::fs::File;
use std::io::{BufRead, BufReader};
use tempdir::TempDir;

#[test]
fn test_end2end_execution() {
    let mut cmd = Command::cargo_bin("mongo-task-generator").unwrap();
    let tmp_dir = TempDir::new("generated_resmoke_config").unwrap();

    cmd.args(&[
        "--target-directory",
        tmp_dir.path().to_str().unwrap(),
        "--expansion-file",
        "tests/data/sample_expansions.yml",
        "--evg-project-file",
        "tests/data/evergreen.yml",
        "--evg-auth-file",
        "tests/data/sample_evergreen_auth.yml",
        "--resmoke-command",
        "python3 tests/mocks/resmoke.py",
        "--use-task-split-fallback",
        "--generate-sub-tasks-config",
        "tests/data/sample_generate_subtasks_config.yml",
        "--bazel-suite-configs",
        "tests/data/sample_bazel_suite_configs.yml",
    ])
    .assert()
    .success();

    let tmp_dir_path = tmp_dir.path();
    assert!(tmp_dir_path.exists());

    let files = std::fs::read_dir(tmp_dir_path).unwrap();
    assert_eq!(688, files.into_iter().collect::<Vec<_>>().len());
}

#[test]
fn test_end2end_target_variant_and_task() {
    let mut cmd = Command::cargo_bin("mongo-task-generator").unwrap();
    let tmp_dir = TempDir::new("generated_resmoke_config").unwrap();

    cmd.args(&[
        "--target-directory",
        tmp_dir.path().to_str().unwrap(),
        "--expansion-file",
        "tests/data/sample_expansions.yml",
        "--evg-project-file",
        "tests/data/evergreen.yml",
        "--evg-auth-file",
        "tests/data/sample_evergreen_auth.yml",
        "--resmoke-command",
        "python3 tests/mocks/resmoke.py",
        "--use-task-split-fallback",
        "--generate-sub-tasks-config",
        "tests/data/sample_generate_subtasks_config.yml",
        "--bazel-suite-configs",
        "tests/data/sample_bazel_suite_configs.yml",
        "--target-variant",
        "enterprise-rhel-80-64-bit-dynamic-required",
        "--target-task",
        "unittest_shell_hang_analyzer_gen",
    ])
    .assert()
    .success();

    let tmp_dir_path = tmp_dir.path();
    assert!(tmp_dir_path.exists());

    let files = std::fs::read_dir(tmp_dir_path).unwrap();
    let num_files = files.into_iter().collect::<Vec<_>>().len();
    // Only the targeted task should be generated, not all 688 files of the full run.
    assert!(num_files > 0);
    assert!(
        num_files < 688,
        "expected filtered generation but found {} files",
        num_files
    );

    let config_file = tmp_dir_path.join("evergreen_config.json");
    assert!(config_file.exists());
    let config = std::fs::read_to_string(config_file).unwrap();
    assert!(
        config.contains("unittest_shell_hang_analyzer"),
        "expected targeted task in generated config"
    );
}

#[test]
fn test_end2end_max_tasks() {
    let mut cmd = Command::cargo_bin("mongo-task-generator").unwrap();
    let tmp_dir = TempDir::new("generated_resmoke_config").unwrap();

    cmd.args(&[
        "--target-directory",
        tmp_dir.path().to_str().unwrap(),
        "--expansion-file",
        "tests/data/sample_expansions.yml",
        "--evg-project-file",
        "tests/data/evergreen.yml",
        "--evg-auth-file",
        "tests/data/sample_evergreen_auth.yml",
        "--resmoke-command",
        "python3 tests/mocks/resmoke.py",
        "--use-task-split-fallback",
        "--generate-sub-tasks-config",
        "tests/data/sample_generate_subtasks_config.yml",
        "--bazel-suite-configs",
        "tests/data/sample_bazel_suite_configs.yml",
        "--max-tasks",
        "1",
    ])
    .assert()
    .success();

    let tmp_dir_path = tmp_dir.path();
    assert!(tmp_dir_path.exists());
    let config_file = tmp_dir_path.join("evergreen_config.json");
    assert!(config_file.exists());

    // max_tasks caps the total number of generated tasks (including sub-tasks), so a run
    // with max_tasks=1 must emit exactly one task definition even though the first suite
    // would otherwise split into multiple sub-tasks. All tests land in the single suite file
    // rather than being scattered across orphaned slice files.
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(config_file).unwrap()).unwrap();
    assert_eq!(config["tasks"].as_array().unwrap().len(), 1);

    let suite_files: Vec<_> = std::fs::read_dir(tmp_dir_path)
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            (path.extension().and_then(|e| e.to_str()) == Some("yml")).then_some(path)
        })
        .collect();
    assert_eq!(
        suite_files.len(),
        1,
        "expected one suite file, found {:#?}",
        suite_files
    );
}

#[test]
fn test_end2end_target_task_matches_generated_name() {
    let mut cmd = Command::cargo_bin("mongo-task-generator").unwrap();
    let tmp_dir = TempDir::new("generated_resmoke_config").unwrap();

    // The target task name matches the generated task's name (without the `_gen` suffix),
    // not the `unittest_shell_hang_analyzer_gen` task definition name.
    cmd.args(&[
        "--target-directory",
        tmp_dir.path().to_str().unwrap(),
        "--expansion-file",
        "tests/data/sample_expansions.yml",
        "--evg-project-file",
        "tests/data/evergreen.yml",
        "--evg-auth-file",
        "tests/data/sample_evergreen_auth.yml",
        "--resmoke-command",
        "python3 tests/mocks/resmoke.py",
        "--use-task-split-fallback",
        "--generate-sub-tasks-config",
        "tests/data/sample_generate_subtasks_config.yml",
        "--bazel-suite-configs",
        "tests/data/sample_bazel_suite_configs.yml",
        "--target-task",
        "unittest_shell_hang_analyzer",
    ])
    .assert()
    .success();

    let config_file = tmp_dir.path().join("evergreen_config.json");
    assert!(config_file.exists());
    let config = std::fs::read_to_string(config_file).unwrap();
    assert!(
        config.contains("unittest_shell_hang_analyzer"),
        "expected targeted generated task in config"
    );
}

#[test]
fn test_end2end_burn_in_execution() {
    let mut cmd = Command::cargo_bin("mongo-task-generator").unwrap();
    let tmp_dir = TempDir::new("generated_resmoke_config").unwrap();

    cmd.args(&[
        "--target-directory",
        tmp_dir.path().to_str().unwrap(),
        "--expansion-file",
        "tests/data/sample_expansions.yml",
        "--evg-project-file",
        "tests/data/evergreen.yml",
        "--evg-auth-file",
        "tests/data/sample_evergreen_auth.yml",
        "--resmoke-command",
        "python3 tests/mocks/resmoke.py",
        "--use-task-split-fallback",
        "--generate-sub-tasks-config",
        "tests/data/sample_generate_subtasks_config.yml",
        "--burn-in",
        "--burn-in-tests-command",
        "python3 tests/mocks/burn_in_tests.py run",
    ])
    .assert()
    .success();

    let tmp_dir_path = tmp_dir.path();
    assert!(tmp_dir_path.exists());

    let files = std::fs::read_dir(tmp_dir_path).unwrap();
    // Only one file `evergreen_config.json` should be generated.
    // That means non-burn-in tasks are NOT generated.
    assert_eq!(1, files.into_iter().collect::<Vec<_>>().len());
}

#[rstest]
#[should_panic(
    expected = r#"`enterprise-rhel-80-64-bit-dynamic-required` build variant is missing the `burn_in_tag_compile_task_dependency` expansion to run `burn_in_tags_gen`. Set the expansion in your project\'s config to continue."#
)]
#[case::panic_with_message("tests/data/burn_in/evergreen_with_no_burn_in_task_group.yml")]
#[should_panic(
    expected = r#"`enterprise-rhel-80-64-bit-dynamic-required` build variant is either missing or has an empty list for the `burn_in_tag_include_build_variants` expansion. Set the expansion in your project\'s config to run burn_in_tags_gen."#
)]
#[case::panic_with_message("tests/data/burn_in/evergreen_with_no_burn_in_variants.yml")]
#[should_panic(
    expected = r#"`enterprise-rhel-80-64-bit-dynamic-required` build variant is either missing or has an empty list for the `burn_in_tag_include_build_variants` expansion. Set the expansion in your project\'s config to run burn_in_tags_gen."#
)]
#[case::panic_with_message("tests/data/burn_in/evergreen_with_empty_burn_in_variants.yml")]
fn test_end2end_burn_in_with_no_distro(#[case] config_location: String) {
    let mut cmd = Command::cargo_bin("mongo-task-generator").unwrap();
    let tmp_dir = TempDir::new("generated_resmoke_config").unwrap();
    cmd.args(&[
        "--target-directory",
        tmp_dir.path().to_str().unwrap(),
        "--expansion-file",
        "tests/data/sample_expansions.yml",
        "--evg-project-file",
        &config_location,
        "--evg-auth-file",
        "tests/data/sample_evergreen_auth.yml",
        "--resmoke-command",
        "python3 tests/mocks/resmoke.py",
        "--use-task-split-fallback",
        "--generate-sub-tasks-config",
        "tests/data/sample_generate_subtasks_config.yml",
        "--burn-in",
        "--burn-in-tests-command",
        "python3 tests/mocks/burn_in_tests.py run",
    ])
    .unwrap();
}

#[rstest]
#[case("tests/data/burn_in/evergreen_burn_in_tasks_with_no_tasks.yml", 4)]
#[case(
    "tests/data/burn_in/evergreen_burn_in_tasks_with_large_distro_task.yml",
    305
)]
#[case(
    "tests/data/burn_in/evergreen_burn_in_tasks_with_non_large_distro_task.yml",
    265
)]
fn test_end2end_burn_in_tasks(#[case] config_location: String, #[case] expected_num_lines: usize) {
    let mut cmd = Command::cargo_bin("mongo-task-generator").unwrap();
    let tmp_dir = TempDir::new("generated_resmoke_config").unwrap();

    cmd.args(&[
        "--target-directory",
        tmp_dir.path().to_str().unwrap(),
        "--expansion-file",
        "tests/data/sample_expansions.yml",
        "--evg-project-file",
        &config_location,
        "--evg-auth-file",
        "tests/data/sample_evergreen_auth.yml",
        "--resmoke-command",
        "python3 tests/mocks/resmoke.py",
        "--use-task-split-fallback",
        "--generate-sub-tasks-config",
        "tests/data/sample_generate_subtasks_config.yml",
        "--burn-in",
        "--burn-in-tests-command",
        "python3 tests/mocks/burn_in_tests.py run",
    ])
    .assert()
    .success();

    let tmp_dir_path = tmp_dir.path();
    assert!(tmp_dir_path.exists());

    let config_file = tmp_dir_path.join("evergreen_config.json");
    assert!(config_file.exists());

    let num_lines = BufRead::lines(BufReader::new(File::open(config_file).unwrap())).count();
    assert_eq!(expected_num_lines, num_lines);
}
