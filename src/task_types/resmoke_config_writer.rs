//! An actor for building and writing resmoke configuration files to disk.
//!
//! This actor will create several instances of itself and send requests to the instances
//! in a round-robbin pattern. The number of instances to create can be specified with the
//! `n_workers` argument when creating the actor.
//!
//! When using this actor, to ensure that all in-flight requests have been completed, you
//! will want to a `flush` message. This message will wait for all actor instance to complete
//! any work they have queued up before returning.
use std::{collections::HashMap, path::PathBuf, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

use crate::{
    resmoke::{resmoke_proxy::TestDiscovery, resmoke_suite::ResmokeSuiteConfig},
    utils::{fs_service::FsService, task_name::name_generated_task},
};

use super::resmoke_tasks::{ResmokeSuiteGenerationInfo, SubSuite};

#[derive(Debug)]
/// Messages that can be sent to the `ResmokeConfigWriter` actor.
enum ResmokeConfigMessage {
    /// Generate and write resmoke configuration files for the given list of sub-suites.
    SuiteFiles(ResmokeSuiteGenerationInfo),

    /// Wait for all in-flight config files to be written to disk.
    Flush(oneshot::Sender<Vec<String>>),
}

/// The actor implementation that performs actions based on received messages.
struct WriteConfigActorImpl {
    /// Test discovery service.
    test_discovery: Arc<dyn TestDiscovery>,

    /// Filesystem service.
    fs_service: Arc<dyn FsService>,

    /// Receiver to wait for messages on.
    receiver: mpsc::Receiver<ResmokeConfigMessage>,

    /// Directory to write generated files to.
    target_dir: String,

    /// Errors encountered during execution.
    errors: Vec<String>,
}

impl WriteConfigActorImpl {
    /// Create a new instance of the actor.
    ///
    /// # Arguments
    ///
    /// * `test_discovery` - Instance of the test discovery service.
    /// * `fs_service` - Service to work with the filesystem.
    /// * `receiver` - Mailbox to query for messages.
    /// * `target_dir` - Directory to write generated files to.
    ///
    /// # Returns
    ///
    /// An instance of the actor.
    fn new(
        test_discovery: Arc<dyn TestDiscovery>,
        fs_service: Arc<dyn FsService>,
        receiver: mpsc::Receiver<ResmokeConfigMessage>,
        target_dir: String,
    ) -> Self {
        WriteConfigActorImpl {
            test_discovery,
            fs_service,
            target_dir,
            receiver,
            errors: vec![],
        }
    }

    /// Handle received messages as long as the receiver has messages to handle.
    async fn run(&mut self) {
        while let Some(msg) = self.receiver.recv().await {
            self.handle_message(msg);
        }
    }

    /// Perform the action specified by the given message.
    ///
    /// # Arguments
    ///
    /// * `msg` - Message to act on.
    fn handle_message(&mut self, msg: ResmokeConfigMessage) {
        match msg {
            ResmokeConfigMessage::SuiteFiles(suite_info) => self.write_suite_files(suite_info),
            ResmokeConfigMessage::Flush(sender) => sender.send(self.errors.clone()).unwrap(),
        }
    }

    /// Write the suite files for the given configuration out to disk.
    ///
    /// # Arguments
    ///
    /// * `suite_info` - Details about the suite that was generated.
    fn write_suite_files(&mut self, suite_info: ResmokeSuiteGenerationInfo) {
        let result = self.write_standard_suite(&suite_info);

        // If we encountered an error, save it off so we can report it on flush.
        if let Err(error) = result {
            self.errors
                .push(format!("ERROR: {}: {}", &suite_info.task_name, error));
        }
    }

    /// Write resmoke configurations for a standard generated resmoke task.
    ///
    /// # Arguments
    ///
    /// * `suite_info` - Details about the generated task.
    fn write_standard_suite(&self, suite_info: &ResmokeSuiteGenerationInfo) -> Result<()> {
        let mut resmoke_config_cache = ResmokeConfigCache::new(self.test_discovery.clone());

        // Create suite files for all the sub-suites.
        self.write_sub_suites(&suite_info.sub_suites, &mut resmoke_config_cache)?;

        // Create a suite file for the '_misc' sub-task.
        self.write_misc_suites(&suite_info.sub_suites, &mut resmoke_config_cache)?;

        Ok(())
    }

    /// Write resmoke configurations for the given sub-suites.
    ///
    /// # Arguments
    ///
    /// * `sub_suites` - List of sub-suites to write configuration for.
    /// * `resmoke_config_cache` - Cache to get resmoke suite configurations.
    fn write_sub_suites(
        &self,
        sub_suites: &[SubSuite],
        resmoke_config_cache: &mut ResmokeConfigCache,
    ) -> Result<()> {
        let total_tasks = sub_suites.len();
        let results: Result<Vec<()>> = sub_suites
            .iter()
            .filter(|s| s.exclude_test_list.is_none())
            .map(|s| {
                let origin_config = resmoke_config_cache.get_config(&s.origin_suite)?;
                let config = origin_config.with_new_tests(Some(&s.test_list), None);

                let filename = format!(
                    "{}.yml",
                    name_generated_task(&s.name, s.index, total_tasks, s.is_enterprise)
                );
                let mut path = PathBuf::from(&self.target_dir);
                path.push(filename);

                self.fs_service.write_file(&path, &config.to_string())?;
                Ok(())
            })
            .collect();
        results?;
        Ok(())
    }

    /// Write resmoke configurations for a "_misc" suite.
    ///
    /// # Arguments
    ///
    /// * `sub_suites` - List of sub-suites comprising the generated suite.
    /// * `resmoke_config_cache` - Cache to get resmoke suite configurations.
    fn write_misc_suites(
        &self,
        sub_suites: &[SubSuite],
        resmoke_config_cache: &mut ResmokeConfigCache,
    ) -> Result<()> {
        let total_tasks = sub_suites.len();
        let results: Result<Vec<()>> = sub_suites
            .iter()
            .filter(|s| s.exclude_test_list.is_some())
            .map(|s| {
                let origin_config = resmoke_config_cache.get_config(&s.origin_suite)?;
                let test_list = s.exclude_test_list.clone().unwrap();
                let misc_config = origin_config.with_new_tests(None, Some(&test_list));
                let filename = format!(
                    "{}.yml",
                    name_generated_task(&s.name, s.index, total_tasks, s.is_enterprise)
                );
                let mut path = PathBuf::from(&self.target_dir);
                path.push(filename);
                self.fs_service
                    .write_file(&path, &misc_config.to_string())?;
                Ok(())
            })
            .collect();
        results?;
        Ok(())
    }
}

#[async_trait]
pub trait ResmokeConfigActor: Sync + Send {
    /// Send a message to write a configuration file to disk.
    async fn write_sub_suite(&mut self, gen_suite: &ResmokeSuiteGenerationInfo);

    /// Wait for all in-progress writes to be completed before returning.
    async fn flush(&mut self) -> Result<Vec<String>>;
}

#[derive(Clone, Debug)]
/// Actor interface for generating and writing resmoke configuration files.
pub struct ResmokeConfigActorService {
    /// Actor workers to send messages to.
    senders: Vec<mpsc::Sender<ResmokeConfigMessage>>,

    /// Next actor worker to send a message to.
    index: usize,
}

impl ResmokeConfigActorService {
    /// Create an new instance of the actor.
    ///
    /// # Arguments
    ///
    /// * `target_dir` - Directory to write generated configuration file to.
    ///
    /// # Returns
    ///
    /// An instance of the actor.
    pub fn new(
        test_discovery: Arc<dyn TestDiscovery>,
        fs_service: Arc<dyn FsService>,
        target_dir: &str,
        n_workers: usize,
    ) -> Self {
        let senders_and_receivers = (0..n_workers).map(|_| mpsc::channel(100));
        let mut senders = vec![];
        senders_and_receivers
            .into_iter()
            .for_each(|(sender, receiver)| {
                senders.push(sender);
                let mut actor = WriteConfigActorImpl::new(
                    test_discovery.clone(),
                    fs_service.clone(),
                    receiver,
                    target_dir.to_string(),
                );
                tokio::spawn(async move { actor.run().await });
            });

        Self { senders, index: 0 }
    }

    /// Send messages to the actor workers with a round-robbin strategy.
    ///
    /// # Arguments
    ///
    /// * `msg` - Message to send to a worker.
    async fn round_robbin(&mut self, msg: ResmokeConfigMessage) {
        let next = self.index;
        self.index = (next + 1) % self.senders.len();
        self.senders[next].send(msg).await.unwrap();
    }
}

#[async_trait]
impl ResmokeConfigActor for ResmokeConfigActorService {
    /// Send a message to write a configuration file to disk.
    async fn write_sub_suite(&mut self, gen_suite: &ResmokeSuiteGenerationInfo) {
        let msg = ResmokeConfigMessage::SuiteFiles(gen_suite.clone());
        self.round_robbin(msg).await;
    }

    /// Wait for all in-progress writes to be completed before returning.
    ///
    /// # Returns
    ///
    /// List of any errors that have occurred.
    async fn flush(&mut self) -> Result<Vec<String>> {
        let mut errors = vec![];
        for sender in &self.senders {
            let (send, recv) = oneshot::channel();
            let msg = ResmokeConfigMessage::Flush(send);
            sender.send(msg).await?;
            errors.extend(recv.await?.iter().map(|e| e.to_string()));
        }
        Ok(errors)
    }
}

/// A cache for querying resmoke suite configurations.
struct ResmokeConfigCache {
    /// Service to query test suite configurations.
    test_discovery: Arc<dyn TestDiscovery>,
    /// Resmoke suite configurations that have already been queried.
    resmoke_configs: HashMap<String, ResmokeSuiteConfig>,
}

impl ResmokeConfigCache {
    /// Create a new instance of a cache.
    pub fn new(test_discovery: Arc<dyn TestDiscovery>) -> Self {
        Self {
            test_discovery,
            resmoke_configs: HashMap::new(),
        }
    }

    /// Get the resmoke suite configuration for the given suite.
    ///
    /// # Arguments
    ///
    /// * `suite_name` - Name of suite to retrieve.
    ///
    /// # Returns
    ///
    /// Resmoke suite configuration for given suite.
    pub fn get_config<'a>(&'a mut self, suite_name: &str) -> Result<&'a ResmokeSuiteConfig> {
        if !self.resmoke_configs.contains_key(suite_name) {
            let config = self.test_discovery.get_suite_config(suite_name)?;
            self.resmoke_configs.insert(suite_name.to_string(), config);
        }

        Ok(self
            .resmoke_configs
            .get(suite_name)
            .expect("Could not find suite"))
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashMap, ops::AddAssign, str::FromStr, sync::Mutex};

    use anyhow::bail;

    use crate::{resmoke::resmoke_suite::ResmokeSuiteConfig, task_types::resmoke_tasks::SubSuite};

    use super::*;

    struct MockTestDiscovery {}
    impl TestDiscovery for MockTestDiscovery {
        fn discover_tests(&self, _suite_name: &str) -> anyhow::Result<Vec<String>> {
            todo!()
        }

        fn get_suite_config(
            &self,
            _suite_name: &str,
        ) -> anyhow::Result<crate::resmoke::resmoke_suite::ResmokeSuiteConfig> {
            let sample_config = "
                test_kind: js_test

                selector:
                  roots:
                    - jstests/auth/*.js
                  exclude_files:
                    - jstests/auth/repl.js
            
                executor:
                    config:
                        shell_options:
                        global_vars:
                            TestData:
                            roleGraphInvalidationIsFatal: true
                        nodb: '' 
            ";
            Ok(ResmokeSuiteConfig::from_str(sample_config).unwrap())
        }

        fn get_multiversion_config(
            &self,
        ) -> anyhow::Result<crate::resmoke::resmoke_proxy::MultiversionConfig> {
            todo!()
        }
    }

    struct MockFsService {
        call_counts: Arc<Mutex<RefCell<HashMap<String, usize>>>>,
        raise_errors: bool,
    }
    impl MockFsService {
        pub fn new() -> Self {
            Self {
                call_counts: Arc::new(Mutex::new(RefCell::new(HashMap::new()))),
                raise_errors: false,
            }
        }

        pub fn new_failure_mode() -> Self {
            Self {
                call_counts: Arc::new(Mutex::new(RefCell::new(HashMap::new()))),
                raise_errors: true,
            }
        }

        pub fn get_call_counts(&self, path: &str) -> usize {
            let call_counts = self.call_counts.lock().unwrap();
            let call_counts_table = call_counts.borrow();
            *call_counts_table.get(path).unwrap()
        }
    }
    impl FsService for MockFsService {
        fn file_exists(&self, _path: &str) -> bool {
            todo!()
        }

        fn write_file(&self, path: &std::path::Path, _contents: &str) -> anyhow::Result<()> {
            if self.raise_errors {
                bail!("Error injected for {:?}", path);
            }
            let call_count_wrapper = self.call_counts.lock().unwrap();
            let mut call_count = call_count_wrapper.borrow_mut();
            if let Some(path_calls) = call_count.get_mut(path.to_str().unwrap()) {
                path_calls.add_assign(1);
            } else {
                call_count.insert(path.to_str().unwrap().to_string(), 1);
            }
            Ok(())
        }
    }

    fn build_mock_service(fs_service: Arc<dyn FsService>) -> WriteConfigActorImpl {
        let test_discovery = Arc::new(MockTestDiscovery {});
        let (_tx, rx) = mpsc::channel(1);

        WriteConfigActorImpl::new(test_discovery, fs_service, rx, "target".to_string())
    }

    #[test]
    fn test_write_suite_files() {
        let fs_service = Arc::new(MockFsService::new());
        let mut resmoke_config_actor = build_mock_service(fs_service.clone());
        let suite_info = ResmokeSuiteGenerationInfo {
            task_name: "my_task".to_string(),
            origin_suite: "original_suite".to_string(),
            generate_multiversion_combos: false,
            sub_suites: vec![
                SubSuite {
                    index: Some(0),
                    name: "suite_name".to_string(),
                    origin_suite: "suite".to_string(),
                    test_list: vec!["test_0.js".to_string(), "test_1.js".to_string()],
                    ..Default::default()
                },
                SubSuite {
                    index: Some(1),
                    name: "suite_name".to_string(),
                    origin_suite: "suite".to_string(),
                    test_list: vec!["test_2.js".to_string(), "test_3.js".to_string()],
                    ..Default::default()
                },
                SubSuite {
                    index: None,
                    name: "suite_name".to_string(),
                    origin_suite: "suite".to_string(),
                    test_list: vec![],
                    exclude_test_list: Some((0..4).map(|i| format!("test_{}.js", i)).collect()),
                    ..Default::default()
                },
            ],
        };

        resmoke_config_actor.write_suite_files(suite_info);

        assert_eq!(fs_service.get_call_counts("target/suite_name_0.yml"), 1);
        assert_eq!(fs_service.get_call_counts("target/suite_name_1.yml"), 1);
        assert_eq!(fs_service.get_call_counts("target/suite_name_misc.yml"), 1);
    }

    #[tokio::test]
    async fn test_errors_encountered_during_execution() {
        let fs_service = Arc::new(MockFsService::new_failure_mode());
        let test_discovery = Arc::new(MockTestDiscovery {});
        let mut resmoke_config_actor =
            ResmokeConfigActorService::new(test_discovery, fs_service, "target_dir", 3);
        let suite_info = ResmokeSuiteGenerationInfo {
            task_name: "my_task".to_string(),
            origin_suite: "original_suite".to_string(),
            generate_multiversion_combos: false,
            sub_suites: vec![
                SubSuite {
                    index: Some(0),
                    name: "suite".to_string(),
                    origin_suite: "suite".to_string(),
                    test_list: vec!["test_0.js".to_string(), "test_1.js".to_string()],
                    ..Default::default()
                },
                SubSuite {
                    index: Some(1),
                    name: "suite".to_string(),
                    origin_suite: "suite".to_string(),
                    test_list: vec!["test_2.js".to_string(), "test_3.js".to_string()],
                    ..Default::default()
                },
            ],
        };
        let n_operations = 8;

        for _ in 0..n_operations {
            resmoke_config_actor.write_sub_suite(&suite_info).await;
        }
        let errors = resmoke_config_actor.flush().await.unwrap();

        assert_eq!(errors.len(), n_operations);
    }
}
