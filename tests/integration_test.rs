use assert_cmd::Command;
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
    assert_eq!(2647, files.into_iter().collect::<Vec<_>>().len());
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
