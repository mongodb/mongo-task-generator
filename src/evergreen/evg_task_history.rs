//! Lookup the history of evergreen tasks.

use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use evg_api_rs::models::stats::{EvgTestStats, EvgTestStatsRequest};
use evg_api_rs::EvgApiClient;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::sync::Arc;

const HOOK_DELIMITER: char = ':';

/// Runtime information of hooks that ran in evergreen.
#[derive(Debug, Clone)]
pub struct HookRuntimeHistory {
    /// Name of test that hook ran with.
    pub test_name: String,
    /// Name of hook.
    pub hook_name: String,
    /// Average runtime of hook.
    pub average_runtime: f64,
}

impl Display for HookRuntimeHistory {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{} : {}",
            self.test_name, self.hook_name, self.average_runtime
        )
    }
}

/// Runtime history of a test in evergreen.
#[derive(Debug, Clone)]
pub struct TestRuntimeHistory {
    /// Name of test.
    pub test_name: String,
    /// Average runtime of test.
    pub average_runtime: f64,
    /// Hooks runtime information of hooks that ran with the test.
    pub hooks: Vec<HookRuntimeHistory>,
}

impl Display for TestRuntimeHistory {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}: {}", self.test_name, self.average_runtime)?;
        for hook in &self.hooks {
            writeln!(f, "- {}", hook)?;
        }
        Ok(())
    }
}

/// Runtime history of a task from evergreen.
#[derive(Debug, Clone)]
pub struct TaskRuntimeHistory {
    /// Name of task.
    pub task_name: String,
    /// Map of tests to the runtime history for that test.
    pub test_map: HashMap<String, TestRuntimeHistory>,
}

/// A service for querying task history from evergreen.
#[async_trait]
pub trait TaskHistoryService: Send + Sync {
    /// Get the test runtime history of the given task.
    ///
    /// # Arguments
    ///
    /// * `task` - Name of task to query.
    /// * `variant` - Name of build variant to query.
    ///
    /// # Returns
    ///
    /// The runtime history of tests belonging to the given suite on the given build variant.
    async fn get_task_history(&self, task: &str, variant: &str) -> Result<TaskRuntimeHistory>;
}

/// An implementation of the task history service.
pub struct TaskHistoryServiceImpl {
    /// Evergreen API client.
    evg_client: Arc<dyn EvgApiClient>,
    /// Number of days of history to query.
    lookback_days: u64,
    /// Evergreen project to query.
    evg_project: String,
}

impl TaskHistoryServiceImpl {
    /// Create a new instance of the task history service.
    ///
    /// # Arguments
    ///
    /// * `evg_client` - Evergreen API client.
    ///
    /// # Returns
    ///
    /// New instance of the task history service implementation.
    pub fn new(evg_client: Arc<dyn EvgApiClient>, lookback_days: u64, evg_project: String) -> Self {
        Self {
            evg_client,
            lookback_days,
            evg_project,
        }
    }
}

#[async_trait]
impl TaskHistoryService for TaskHistoryServiceImpl {
    /// Get the test runtime history of the given task.
    ///
    /// # Arguments
    ///
    /// * `task` - Name of task to query.
    /// * `variant` - Name of build variant to query.
    ///
    /// # Returns
    ///
    /// The runtime history of tests belonging to the given suite on the given build variant.
    async fn get_task_history(&self, task: &str, variant: &str) -> Result<TaskRuntimeHistory> {
        let today = Utc::now();
        let lookback = Duration::days(self.lookback_days as i64);
        let start_date = today - lookback;

        let request = EvgTestStatsRequest {
            after_date: date_to_string(&start_date),
            before_date: date_to_string(&today),
            group_num_days: self.lookback_days,
            variants: variant.to_string(),
            tasks: task.to_string(),
            tests: None,
        };

        let stats = self
            .evg_client
            .get_test_stats(&self.evg_project, &request)
            .await
            .unwrap();

        // Split the returned stats into stats for hooks and tests. Also attach the hook stats
        // to the test that they ran with.
        let hook_map = gather_hook_stats(&stats);
        let test_map = gather_test_stats(&stats, &hook_map);

        Ok(TaskRuntimeHistory {
            task_name: task.to_string(),
            test_map,
        })
    }
}

/// Convert the list of stats into a map of test names to test stats.
///
/// Also include hook information for all tests with their stats.
///
/// # Arguments
///
/// * `stat_list` - List of stats.
/// * `hook_map` - Map of test names to hook stats that ran with the test.
///
/// # Returns
///
/// Map of test names to stats belong to that test.
fn gather_test_stats(
    stat_list: &[EvgTestStats],
    hook_map: &HashMap<String, Vec<HookRuntimeHistory>>,
) -> HashMap<String, TestRuntimeHistory> {
    let mut test_map: HashMap<String, TestRuntimeHistory> = HashMap::new();
    for stat in stat_list {
        if !is_hook(&stat.test_file) {
            let test_name = get_test_name(&stat.test_file);
            if let Some(v) = test_map.get_mut(&test_name) {
                v.test_name = stat.test_file.clone();
                v.average_runtime += stat.avg_duration_pass;
            } else {
                test_map.insert(
                    test_name.clone(),
                    TestRuntimeHistory {
                        test_name: stat.test_file.clone(),
                        average_runtime: stat.avg_duration_pass,
                        hooks: hook_map
                            .get(&test_name.to_string())
                            .unwrap_or(&vec![])
                            .clone(),
                    },
                );
            }
        }
    }

    test_map
}

/// Gather all the hook stats in the given list into a map by the test the hooks ran with.
///
/// # Arguments
///
/// * `stat_list` - List of stats.
///
/// # Returns
///
/// Map of test name and hook stats for hooks that ran with the test.
fn gather_hook_stats(stat_list: &[EvgTestStats]) -> HashMap<String, Vec<HookRuntimeHistory>> {
    let mut hook_map: HashMap<String, Vec<HookRuntimeHistory>> = HashMap::new();
    for stat in stat_list {
        if is_hook(&stat.test_file) {
            let test_name = hook_test_name(&stat.test_file);
            let hook_name = hook_hook_name(&stat.test_file);
            if let Some(v) = hook_map.get_mut(&test_name.to_string()) {
                v.push(HookRuntimeHistory {
                    test_name: test_name.to_string(),
                    hook_name: hook_name.to_string(),
                    average_runtime: stat.avg_duration_pass,
                });
            } else {
                hook_map.insert(
                    test_name.to_string(),
                    vec![HookRuntimeHistory {
                        test_name: test_name.to_string(),
                        hook_name: hook_name.to_string(),
                        average_runtime: stat.avg_duration_pass,
                    }],
                );
            }
        }
    }
    hook_map
}

/// Convert the given date into a string for evergreen.
///
/// # Arguments
///
/// * `date` - Date object to convert to a string.
///
/// # Returns
///
/// String format of the given date for evergreen consumption.
fn date_to_string(date: &DateTime<Utc>) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// Determine if the given identifier is a hook.
///
/// Identifiers for hooks have a ':' in them separating the test name from the hook name.
///
/// # Arguments
///
/// * `identifier` - Identifier to check.
///
/// # Returns
///
/// # true if the given identifier is a hook.
fn is_hook(identifier: &str) -> bool {
    identifier.contains(HOOK_DELIMITER)
}

/// Get the test name part of a given hook identifier.
///
/// # Arguments
///
/// * `identifier` - Identifier to query.
///
/// # Returns
///
/// # test name of the given hook identifier.
fn hook_test_name(identifier: &str) -> &str {
    identifier.split(HOOK_DELIMITER).next().unwrap()
}

/// Get the hook name part of a given hook identifier.
///
/// # Arguments
///
/// * `identifier` - Identifier to query.
///
/// # Returns
///
/// # hook name of the given hook identifier.
fn hook_hook_name(identifier: &str) -> &str {
    identifier.split(HOOK_DELIMITER).last().unwrap()
}

/// Get the base name of the given test file.
///
/// # Arguments
///
/// * `test_file` - Relative path to test file.
///
/// # Returns
///
/// Base name of test file with extension removed.
pub fn get_test_name(test_file: &str) -> String {
    let s = test_file.split('/');
    s.last().unwrap().trim_end_matches(".js").to_string()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("some/random/test", false)]
    #[case("some/random/test:hook1", true)]
    fn test_is_hook(#[case] hook_name: &str, #[case] expected_is_hook: bool) {
        assert_eq!(is_hook(hook_name), expected_is_hook);
    }

    #[test]
    fn test_hook_test_name() {
        assert_eq!(hook_test_name("my_test:my_hook"), "my_test");
    }

    #[test]
    fn test_hook_hook_name() {
        assert_eq!(hook_hook_name("my_test:my_hook"), "my_hook");
    }

    // get_test_name tests.
    #[rstest]
    #[case("jstests/core/add1.js", "add1")]
    #[case("jstests/core/add1", "add1")]
    #[case("add1.js", "add1")]
    fn test_get_test_name(#[case] test_file: &str, #[case] expected_name: &str) {
        assert_eq!(get_test_name(test_file), expected_name.to_string());
    }
}
