use assert_cmd::Command;
use rstest::rstest;
use serde_json::{json, Value};
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

    // `auth_gen` in tests/data/evergreen.yml is tagged with both team ownership tag flavors, plus
    // an unrelated `auth` tag. Only the ownership tags should reach the generated sub-tasks.
    let config: Value =
        serde_json::from_reader(File::open(tmp_dir_path.join("evergreen_config.json")).unwrap())
            .unwrap();
    let tasks = config["tasks"].as_array().unwrap();

    // Sub-tasks of `auth_gen` are named `auth_<index>...`; `auth_audit_gen` is a different,
    // untagged `_gen` task, so match on the index to avoid picking it up.
    let is_auth_sub_task = |t: &Value| {
        let name = t["name"].as_str().unwrap();
        name.strip_prefix("auth_")
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| c.is_ascii_digit())
    };

    let auth_sub_tasks: Vec<&Value> = tasks.iter().filter(|t| is_auth_sub_task(t)).collect();
    assert!(!auth_sub_tasks.is_empty());
    for sub_task in &auth_sub_tasks {
        assert_eq!(
            sub_task["tags"].as_array().unwrap(),
            &vec![
                json!("assigned_to_jira_team_a_team"),
                json!("assigned_to_mothra_team_b_team"),
            ],
            "unexpected tags on {}",
            sub_task["name"]
        );
    }

    // Tasks generated from "_gen" definitions with no ownership tags must stay untagged, so that
    // this change does not grow the generated configuration for every other task.
    assert!(
        tasks
            .iter()
            .filter(|t| !is_auth_sub_task(t))
            .all(|t| t["tags"].is_null()),
        "team tags leaked onto tasks whose _gen definition had none"
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
