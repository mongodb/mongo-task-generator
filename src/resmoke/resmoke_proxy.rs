use std::{
    path::Path,
    str::FromStr,
    sync::{Arc, Mutex},
    time::Instant,
};

use anyhow::Result;
use serde::Deserialize;
use tracing::{error, event, Level};

use crate::evergreen::evg_config_utils::is_bazel_suite;

use super::{external_cmd::run_command, resmoke_suite::ResmokeSuiteConfig};
use std::collections::HashMap;

/// Interface for discovering details about test suites.
pub trait TestDiscovery: Send + Sync {
    /// Get a list of tests that belong to the given suite.
    ///
    /// # Arguments
    ///
    /// * `suite_name` - Name of test suite to query.
    ///
    /// # Returns
    ///
    /// A list of tests belonging to given suite.
    fn discover_tests(&self, suite_name: &str) -> Result<Vec<String>>;

    /// Get the configuration for the given suite.
    ///
    /// # Arguments
    ///
    /// * `suite_name` - Name of test suite to query.
    ///
    /// # Return
    ///
    /// Resmoke configuration for the given suite.
    fn get_suite_config(&self, suite_name: &str) -> Result<ResmokeSuiteConfig>;

    /// Get the multiversion configuration to generate against.
    fn get_multiversion_config(&self) -> Result<MultiversionConfig>;

    /// Discover the given suites ahead of time so later `discover_tests` calls are cheap.
    ///
    /// Best-effort: implementations may ignore this, and failures should fall back to
    /// per-suite discovery rather than failing generation.
    fn prewarm(&self, _suite_names: &[String]) -> Result<()> {
        Ok(())
    }
}

/// Implementation of `TestDiscovery` that queries details from resmoke.
#[derive(Debug, Clone)]
pub struct ResmokeProxy {
    /// Primary command to invoke resmoke (usually `python`).
    resmoke_cmd: String,
    /// Script to invoke resmoke.
    resmoke_script: Vec<String>,
    /// True if the generator should skip tests already run in more complex suites.
    skip_covered_tests: bool,
    /// True if test discovery should include tests that are tagged with fully disabled features.
    include_fully_disabled_feature_tests: bool,
    bazel_suite_configs: BazelConfigs,
    /// Cache of test discovery results, keyed by suite name. The same suite is queried
    /// several times per run (once per multiversion/variant combination) and discovery
    /// shells out to resmoke, so repeat lookups are worth avoiding. Each entry has its
    /// own lock so identical concurrent lookups wait for the first one instead of
    /// spawning duplicate resmoke processes, while lookups of different suites proceed
    /// in parallel.
    discovery_cache: DiscoveryCache,
    /// Cache of suite configurations, keyed by suite name, with the same per-entry
    /// locking scheme as `discovery_cache`. `suiteconfig` also costs a full resmoke
    /// startup and is requested once per generated task while only depending on the
    /// suite name.
    suite_config_cache: SuiteConfigCache,
}

/// Cache of test discovery results, keyed by suite name. Each entry has its own
/// lock so identical concurrent lookups share one resmoke invocation.
type DiscoveryCache = Arc<Mutex<HashMap<String, Arc<Mutex<Option<Vec<String>>>>>>>;

/// Cache of suite configurations, keyed by suite name, with the same per-entry
/// locking scheme as `DiscoveryCache`.
type SuiteConfigCache = Arc<Mutex<HashMap<String, Arc<Mutex<Option<ResmokeSuiteConfig>>>>>>;

impl ResmokeProxy {
    /// Create a new `ResmokeProxy` instance.
    ///
    /// # Arguments
    ///
    /// * `resmoke_cmd` - Command to invoke resmoke.
    /// * `skip_covered_tests` - Whether the generator should skip tests run in more complex suites.
    /// * `include_fully_disabled_feature_tests` - If the generator should include tests that are tagged with fully disabled features.
    /// * `bazel_suite_configs` - Optional bazel suite configurations.
    pub fn new(
        resmoke_cmd: &str,
        skip_covered_tests: bool,
        include_fully_disabled_feature_tests: bool,
        bazel_suite_configs: BazelConfigs,
    ) -> Self {
        let cmd_parts: Vec<_> = resmoke_cmd.split(' ').collect();
        let cmd = cmd_parts[0];
        let script = cmd_parts[1..].iter().map(|s| s.to_string()).collect();
        Self {
            resmoke_cmd: cmd.to_string(),
            resmoke_script: script,
            skip_covered_tests,
            include_fully_disabled_feature_tests,
            bazel_suite_configs,
            discovery_cache: Arc::new(Mutex::new(HashMap::new())),
            suite_config_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BazelConfigs {
    /// Map of bazel resmoke config targets to their generated suite config YAMLs.
    configs: HashMap<String, String>,
}

impl BazelConfigs {
    pub fn from_yaml_file(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        let configs: Result<HashMap<String, String>, serde_yaml::Error> =
            serde_yaml::from_str(&contents);
        if configs.is_err() {
            error!(
                file = path.display().to_string(),
                contents = &contents,
                "Failed to parse bazel configs from yaml file",
            );
        }
        Ok(Self { configs: configs? })
    }

    /// Get the generated suite config for a bazel resmoke target.
    ///
    /// # Arguments
    ///
    /// * `target` - Bazel resmoke test target, like "//buildscripts/resmoke:core".
    ///
    /// # Returns
    ///
    /// The path the the generated suite config YAML, like "bazel-out/buildscripts/resmoke/core_config.yml".
    pub fn get(&self, target: &str) -> &str {
        match self.configs.get(&format!("{}_config", target)) {
            Some(config) => config,
            None => {
                panic!("No bazel config found for target: {}", target);
            }
        }
    }
}

/// Details about tests comprising a test suite.
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TestDiscoveryOutput {
    /// Name of suite.
    pub suite_name: String,

    /// Name of tests comprising suite.
    pub tests: Vec<String>,
}

impl TestDiscovery for ResmokeProxy {
    /// Get a list of tests that belong to the given suite.
    ///
    /// # Arguments
    ///
    /// * `suite_name` - Name of test suite to query.
    ///
    /// # Returns
    ///
    /// A list of tests belonging to given suite.
    fn discover_tests(&self, suite_name: &str) -> Result<Vec<String>> {
        let entry = {
            // Recover from a poisoned lock (a panic in another discovery thread)
            // rather than failing the whole generation over a cache optimization.
            let mut cache = self
                .discovery_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            cache
                .entry(suite_name.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(None)))
                .clone()
        };
        let mut entry = entry.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(tests) = entry.as_ref() {
            return Ok(tests.clone());
        }
        let tests = self.run_test_discovery(suite_name)?;
        *entry = Some(tests.clone());
        Ok(tests)
    }

    /// Discover all the given suites with a small number of batched resmoke invocations
    /// and seed the discovery cache with the results.
    ///
    /// Requires resmoke's `test-discovery` to accept repeated `--suite` arguments. Any
    /// batch that fails is skipped: its suites are discovered lazily one-by-one later.
    fn prewarm(&self, suite_names: &[String]) -> Result<()> {
        const BATCH_SIZE: usize = 100;

        for batch in suite_names.chunks(BATCH_SIZE) {
            let start = Instant::now();
            let output = match self.run_batch_test_discovery(batch) {
                Ok(output) => output,
                Err(err) => {
                    error!(
                        error = err.to_string(),
                        suites = batch.join(","),
                        "Batch test discovery failed; falling back to per-suite discovery"
                    );
                    continue;
                }
            };

            // Documents are expected in request order. If any document is missing or
            // fails to parse, discard the whole batch rather than risk seeding results
            // under the wrong suite name; those suites are discovered lazily instead.
            let parsed: Vec<TestDiscoveryOutput> = match serde_yaml::Deserializer::from_str(&output)
                .map(TestDiscoveryOutput::deserialize)
                .collect::<Result<_, _>>()
            {
                Ok(parsed) => parsed,
                Err(err) => {
                    error!(
                        error = err.to_string(),
                        suites = batch.join(","),
                        "Failed to parse batch test discovery output; falling back to per-suite discovery"
                    );
                    continue;
                }
            };
            if parsed.len() != batch.len() {
                error!(
                    expected = batch.len(),
                    parsed = parsed.len(),
                    "Batch test discovery output did not match request; falling back to per-suite discovery"
                );
                continue;
            }

            // Verify each document lines up with the suite we requested at that position.
            // Resmoke echoes back the `--suite` value as `suite_name`, so a mismatch means
            // the output is out of order; discard the whole batch and discover lazily.
            if let Some((suite_name, doc)) = batch
                .iter()
                .zip(&parsed)
                .find(|(suite_name, doc)| self.suite_arg(suite_name) != doc.suite_name)
            {
                error!(
                    expected = self.suite_arg(suite_name),
                    actual = doc.suite_name,
                    "Batch test discovery returned suites out of order; falling back to per-suite discovery"
                );
                continue;
            }

            let mut seeded = 0;
            for (suite_name, doc) in batch.iter().zip(parsed) {
                let tests: Vec<String> = doc
                    .tests
                    .into_iter()
                    .filter(|f| Path::new(f).exists())
                    .collect();
                let entry = {
                    let mut cache = self
                        .discovery_cache
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    cache
                        .entry(suite_name.to_string())
                        .or_insert_with(|| Arc::new(Mutex::new(None)))
                        .clone()
                };
                *entry.lock().unwrap_or_else(|e| e.into_inner()) = Some(tests);
                seeded += 1;
            }

            event!(
                Level::INFO,
                batch_size = batch.len(),
                seeded,
                duration_ms = start.elapsed().as_millis() as u64,
                "Batch resmoke test discovery finished"
            );
        }

        Ok(())
    }

    /// Get the configuration for the given suite.
    ///
    /// # Arguments
    ///
    /// * `suite_name` - Name of test suite to query.
    ///
    /// # Return
    ///
    /// Resmoke configuration for the given suite.
    fn get_suite_config(&self, suite_name: &str) -> Result<ResmokeSuiteConfig> {
        let entry = {
            let mut cache = self
                .suite_config_cache
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            cache
                .entry(suite_name.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(None)))
                .clone()
        };
        let mut entry = entry.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(config) = entry.as_ref() {
            return Ok(config.clone());
        }

        let suite_config = self.suite_arg(suite_name);

        let mut cmd = vec![&*self.resmoke_cmd];
        cmd.append(&mut self.resmoke_script.iter().map(|s| s.as_str()).collect());
        cmd.append(&mut vec!["suiteconfig", "--suite", suite_config]);
        let start = Instant::now();
        let cmd_output = run_command(&cmd)?;
        event!(
            Level::INFO,
            suite_config,
            duration_ms = start.elapsed().as_millis() as u64,
            "Resmoke suiteconfig finished"
        );

        let config = ResmokeSuiteConfig::from_str(&cmd_output)?;
        *entry = Some(config.clone());
        Ok(config)
    }

    /// Get the multiversion configuration to generate against.
    fn get_multiversion_config(&self) -> Result<MultiversionConfig> {
        MultiversionConfig::from_resmoke(&self.resmoke_cmd, &self.resmoke_script)
    }
}

impl ResmokeProxy {
    /// Resolve the value passed as resmoke's `--suite` argument for the given suite.
    /// For bazel suites this is the generated suite config path; otherwise the suite name.
    fn suite_arg<'a>(&'a self, suite_name: &'a str) -> &'a str {
        if is_bazel_suite(suite_name) {
            self.bazel_suite_configs.get(suite_name)
        } else {
            suite_name
        }
    }

    /// Build a `test-discovery` command line covering the given suites.
    fn test_discovery_command<'a>(&'a self, suite_names: &[&'a str]) -> Vec<&'a str> {
        let mut cmd = vec![&*self.resmoke_cmd];
        cmd.append(&mut self.resmoke_script.iter().map(|s| s.as_str()).collect());
        cmd.push("test-discovery");
        for suite_name in suite_names {
            cmd.append(&mut vec!["--suite", self.suite_arg(suite_name)]);
        }

        // When running in a patch build, we use the --skipTestsCoveredByMoreComplexSuites
        // flag to tell Resmoke to exclude any tests in the given suite that will
        // also be run on a more complex suite.
        if self.skip_covered_tests {
            cmd.append(&mut vec!["--skipTestsCoveredByMoreComplexSuites"]);
        }

        if self.include_fully_disabled_feature_tests {
            cmd.append(&mut vec!["--includeFullyDisabledFeatureTests"]);
        }

        cmd
    }

    /// Run a single batched `test-discovery` invocation covering the given suites.
    fn run_batch_test_discovery(&self, suite_names: &[String]) -> Result<String> {
        let suite_refs: Vec<&str> = suite_names.iter().map(|s| s.as_str()).collect();
        let cmd = self.test_discovery_command(&suite_refs);
        run_command(&cmd)
    }

    /// Query resmoke for the list of tests in the given suite.
    fn run_test_discovery(&self, suite_name: &str) -> Result<Vec<String>> {
        let cmd = self.test_discovery_command(&[suite_name]);

        let start = Instant::now();
        let cmd_output = run_command(&cmd)?;

        event!(
            Level::INFO,
            suite_name,
            duration_ms = start.elapsed().as_millis() as u64,
            "Resmoke test discovery finished"
        );

        let output: Result<TestDiscoveryOutput, serde_yaml::Error> =
            serde_yaml::from_str(&cmd_output);
        if output.is_err() {
            error!(
                command = cmd.join(" "),
                command_output = &cmd_output,
                "Failed to parse yaml from discover tests command output",
            );
        }

        Ok(output?
            .tests
            .into_iter()
            .filter(|f| Path::new(f).exists())
            .collect())
    }
}

/// Multiversion configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct MultiversionConfig {
    /// Previous version of MongoDB to test against.
    pub last_versions: Vec<String>,

    /// Tags for required FCV version.
    pub requires_fcv_tag: String,

    /// Tags for last LTS FCV versions.
    pub requires_fcv_tag_lts: Option<String>,

    /// Tags for last continuous FCV versions.
    pub requires_fcv_tag_continuous: Option<String>,
}

impl MultiversionConfig {
    /// Query the multiversion configuration from resmoke.
    pub fn from_resmoke(cmd: &str, script: &[String]) -> Result<MultiversionConfig> {
        let mut cmd = vec![cmd];
        let file_name = "multiversion-config.yml";
        cmd.append(&mut script.iter().map(|s| s.as_str()).collect());
        cmd.append(&mut vec!["multiversion-config"]);
        let file_arg = format!("--config-file-output={}", file_name);
        cmd.append(&mut vec![&file_arg]);
        run_command(&cmd).unwrap();
        let multiversion_config_output =
            std::fs::read_to_string(file_name).expect("Multiversion config file not found.");
        let multiversion_config: Result<MultiversionConfig, serde_yaml::Error> =
            serde_yaml::from_str(&multiversion_config_output);
        if multiversion_config.is_err() {
            error!(
                command = cmd.join(" "),
                command_output = &multiversion_config_output,
                "Failed to parse yaml from multiversion config command output",
            );
        }
        Ok(multiversion_config?)
    }

    /// Get the required FCV tag for the lts version.
    pub fn get_fcv_tags_for_lts(&self) -> String {
        if let Some(requires_fcv_tag_lts) = &self.requires_fcv_tag_lts {
            requires_fcv_tag_lts.clone()
        } else {
            self.requires_fcv_tag.clone()
        }
    }

    /// Get the required FCV tag for the continuous version.
    pub fn get_fcv_tags_for_continuous(&self) -> String {
        if let Some(requires_fcv_tag_continuous) = &self.requires_fcv_tag_continuous {
            requires_fcv_tag_continuous.clone()
        } else {
            self.requires_fcv_tag.clone()
        }
    }

    /// Get the required FCV tag for the last patch version.
    ///
    /// `last_patch` runs against the latest patch release of the current version, whose FCV
    /// matches the version under test, so the default `requires_fcv_tag` set applies.
    pub fn get_fcv_tags_for_patch(&self) -> String {
        self.requires_fcv_tag.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // tests for discover_tests caching.
    // Uses `sh` and shell redirection, so restrict to Unix platforms.
    #[cfg(unix)]
    #[test]
    fn test_discover_tests_only_runs_discovery_once_per_suite() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "resmoke_proxy_cache_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let counter_file = tmp_dir.join("calls.txt");
        let _ = std::fs::remove_file(&counter_file);
        let script_file = tmp_dir.join("fake_resmoke.sh");
        std::fs::write(
            &script_file,
            format!(
                "echo called >> \"{}\"\necho 'suite_name: my_suite'\necho 'tests: []'\n",
                counter_file.display()
            ),
        )
        .unwrap();
        let proxy = ResmokeProxy::new(
            &format!("sh {}", script_file.display()),
            false,
            false,
            BazelConfigs::default(),
        );

        for _ in 0..3 {
            assert_eq!(
                proxy.discover_tests("my_suite").unwrap(),
                Vec::<String>::new()
            );
        }
        proxy.discover_tests("other_suite").unwrap();

        let calls = std::fs::read_to_string(&counter_file).unwrap();
        assert_eq!(calls.lines().count(), 2);
        std::fs::remove_dir_all(&tmp_dir).unwrap();
    }

    // Uses `sh` and shell redirection, so restrict to Unix platforms.
    #[cfg(unix)]
    #[test]
    fn test_prewarm_seeds_cache_from_batched_discovery() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "resmoke_proxy_prewarm_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let counter_file = tmp_dir.join("calls.txt");
        let _ = std::fs::remove_file(&counter_file);
        let script_file = tmp_dir.join("fake_resmoke.sh");
        std::fs::write(
            &script_file,
            format!(
                concat!(
                    "echo called >> {}\n",
                    "echo 'suite_name: suite_a'\n",
                    "echo 'tests: []'\n",
                    "echo '---'\n",
                    "echo 'suite_name: suite_b'\n",
                    "echo 'tests: []'\n",
                ),
                counter_file.display()
            ),
        )
        .unwrap();
        let proxy = ResmokeProxy::new(
            &format!("sh {}", script_file.display()),
            false,
            false,
            BazelConfigs::default(),
        );

        proxy
            .prewarm(&["suite_a".to_string(), "suite_b".to_string()])
            .unwrap();
        assert_eq!(
            proxy.discover_tests("suite_a").unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            proxy.discover_tests("suite_b").unwrap(),
            Vec::<String>::new()
        );

        let calls = std::fs::read_to_string(&counter_file).unwrap();
        assert_eq!(calls.lines().count(), 1);
        std::fs::remove_dir_all(&tmp_dir).unwrap();
    }

    // Uses `sh` and shell redirection, so restrict to Unix platforms.
    #[cfg(unix)]
    #[test]
    fn test_get_suite_config_only_runs_suiteconfig_once_per_suite() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "resmoke_proxy_suiteconfig_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let counter_file = tmp_dir.join("calls.txt");
        let _ = std::fs::remove_file(&counter_file);
        let script_file = tmp_dir.join("fake_resmoke.sh");
        std::fs::write(
            &script_file,
            format!(
                concat!(
                    "echo called >> {}\n",
                    "echo 'test_kind: js_test'\n",
                    "echo 'selector:'\n",
                    "echo '  roots: []'\n",
                    "echo 'executor: {{}}'\n",
                ),
                counter_file.display()
            ),
        )
        .unwrap();
        let proxy = ResmokeProxy::new(
            &format!("sh {}", script_file.display()),
            false,
            false,
            BazelConfigs::default(),
        );

        for _ in 0..3 {
            proxy.get_suite_config("my_suite").unwrap();
        }

        let calls = std::fs::read_to_string(&counter_file).unwrap();
        assert_eq!(calls.lines().count(), 1);
        std::fs::remove_dir_all(&tmp_dir).unwrap();
    }

    // tests for get_fcv_tags_for_lts.
    #[test]
    fn test_get_fcv_tags_for_lts_should_use_lts_if_provided() {
        let mv_config = MultiversionConfig {
            last_versions: vec![],
            requires_fcv_tag: "fcv_fallback".to_string(),
            requires_fcv_tag_lts: Some("fcv_lts_explicit".to_string()),
            requires_fcv_tag_continuous: None,
        };

        assert_eq!(&mv_config.get_fcv_tags_for_lts(), "fcv_lts_explicit")
    }

    #[test]
    fn test_get_fcv_tags_for_lts_should_fallback_if_no_lts_provided() {
        let mv_config = MultiversionConfig {
            last_versions: vec![],
            requires_fcv_tag: "fcv_fallback".to_string(),
            requires_fcv_tag_lts: None,
            requires_fcv_tag_continuous: None,
        };

        assert_eq!(&mv_config.get_fcv_tags_for_lts(), "fcv_fallback")
    }

    // tests for get_fcv_tags_for_continuous.
    #[test]
    fn test_get_fcv_tags_for_continuous_should_use_continuous_if_provided() {
        let mv_config = MultiversionConfig {
            last_versions: vec![],
            requires_fcv_tag: "fcv_fallback".to_string(),
            requires_fcv_tag_lts: None,
            requires_fcv_tag_continuous: Some("fcv_continuous_explicit".to_string()),
        };

        assert_eq!(
            &mv_config.get_fcv_tags_for_continuous(),
            "fcv_continuous_explicit"
        )
    }

    #[test]
    fn test_get_fcv_tags_for_continuous_should_fallback_if_no_continuous_provided() {
        let mv_config = MultiversionConfig {
            last_versions: vec![],
            requires_fcv_tag: "fcv_fallback".to_string(),
            requires_fcv_tag_lts: None,
            requires_fcv_tag_continuous: None,
        };

        assert_eq!(&mv_config.get_fcv_tags_for_continuous(), "fcv_fallback")
    }
}
