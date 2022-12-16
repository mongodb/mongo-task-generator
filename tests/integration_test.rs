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
    ])
    .assert()
    .success();

    let tmp_dir_path = tmp_dir.path();
    assert!(tmp_dir_path.exists());

    let files = std::fs::read_dir(tmp_dir_path).unwrap();
    assert_eq!(846, files.into_iter().collect::<Vec<_>>().len());
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
        "python3 tests/mocks/burn_in_tests.py",
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
    expected = "`enterprise-rhel-80-64-bit-dynamic-required` build variant is missing the `burn_in_tag_compile_task_group_name` expansion to run `burn_in_tags_gen`. Set the expansion in your project\\'s config to continue.\\"
)]
#[case::panic_with_message("tests/data/burn_in/evergreen_with_no_burn_in_task_group.yml")]
#[should_panic(
    expected = "`enterprise-rhel-80-64-bit-dynamic-required` build variant is either missing or has an empty list for the `burn_in_tag_buildvariants` expansion. Set the expansion in your project\\'s config to run burn_in_tags_gen.\\"
)]
#[case::panic_with_message("tests/data/burn_in/evergreen_with_no_burn_in_variants.yml")]
#[should_panic(
    expected = "`enterprise-rhel-80-64-bit-dynamic-required` build variant is either missing or has an empty list for the `burn_in_tag_buildvariants` expansion. Set the expansion in your project\\'s config to run burn_in_tags_gen.\\"
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
        "python3 tests/mocks/burn_in_tests.py",
    ])
    .unwrap();
}

#[rstest]
#[case("tests/data/burn_in/evergreen_burn_in_tasks_no_tasks.yml", 4)]
#[case(
    "tests/data/burn_in/evergreen_burn_in_tasks_with_large_distro_task.yml",
    315
)]
#[case(
    "tests/data/burn_in/evergreen_burn_in_tasks_with_non_large_distro_task.yml",
    275
)]
#[case("tests/data/burn_in/evergreen_burn_in_tasks_with_two_tasks.yml", 565)]
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
        "python3 tests/mocks/burn_in_tests.py",
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
