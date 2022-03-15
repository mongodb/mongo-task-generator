//! An actor for building and writing resmoke configuration files to disk.
//!
//! This actor will create several instances of itself and send requests to the instances
//! in a round-robbin pattern. The number of instances to create can be specified with the
//! `n_workers` argument when creating the actor.
//!
//! When using this actor, to ensure that all in-flight requests have been completed, you
//! will want to a `flush` message. This message will wait for all actor instance to complete
//! any work they have queued up before returning.
use std::{path::PathBuf, sync::Arc};

use async_trait::async_trait;
use tokio::sync::{mpsc, oneshot};

use crate::{
    resmoke::{resmoke_proxy::TestDiscovery, resmoke_suite::ResmokeSuiteConfig},
    utils::fs_service::FsService,
};

use super::{
    multiversion::MultiversionService,
    resmoke_tasks::{ResmokeSuiteGenerationInfo, SubSuite},
};

#[derive(Debug)]
/// Messages that can be sent to the `ResmokeConfigWriter` actor.
enum ResmokeConfigMessage {
    /// Generate and write resmoke configuration files for the given list of sub-suites.
    SuiteFiles(ResmokeSuiteGenerationInfo),

    /// Wait for all in-flight config files to be written to disk.
    Flush(oneshot::Sender<()>),
}

/// The actor implementation that performs actions based on received messages.
struct WriteConfigActorImpl {
    /// Test discovery service.
    test_discovery: Arc<dyn TestDiscovery>,

    multiversion_service: Arc<dyn MultiversionService>,

    /// Filesystem service.
    fs_service: Arc<dyn FsService>,

    /// Receiver to wait for messages on.
    receiver: mpsc::Receiver<ResmokeConfigMessage>,

    /// Directory to write generated files to.
    target_dir: String,
}

impl WriteConfigActorImpl {
    /// Create a new instance of the actor.
    ///
    /// # Arguments
    ///
    /// * `test_discovery` - Instance of the test discovery service.
    /// * `multiversion_service` - Service to get multiversion information.
    /// * `fs_service` - Service to work with the filesystem.
    /// * `receiver` - Mailbox to query for messages.
    /// * `target_dir` - Directory to write generated files to.
    ///
    /// # Returns
    ///
    /// An instance of the actor.
    fn new(
        test_discovery: Arc<dyn TestDiscovery>,
        multiversion_service: Arc<dyn MultiversionService>,
        fs_service: Arc<dyn FsService>,
        receiver: mpsc::Receiver<ResmokeConfigMessage>,
        target_dir: String,
    ) -> Self {
        WriteConfigActorImpl {
            test_discovery,
            multiversion_service,
            fs_service,
            target_dir,
            receiver,
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
            ResmokeConfigMessage::Flush(sender) => sender.send(()).unwrap(),
        }
    }

    /// Write the suite files for the given configuration out to disk.
    ///
    /// # Arguments
    ///
    /// * `suite_info` - Details about the suite that was generated.
    fn write_suite_files(&self, suite_info: ResmokeSuiteGenerationInfo) {
        if suite_info.generate_multiversion_combos {
            self.write_multiversion_suite(suite_info);
        } else {
            self.write_standard_suite(suite_info);
        }
    }

    /// Write resmoke configurations for a multiversion generated resmoke task.
    ///
    /// # Arguments
    ///
    /// * `suite_info` - Details about the generated task.
    fn write_multiversion_suite(&self, suite_info: ResmokeSuiteGenerationInfo) {
        for (old_version, version_combination) in self
            .multiversion_service
            .multiversion_iter(&suite_info.origin_suite)
            .unwrap()
        {
            // This is potentially confusing. We have 2 suite names, that are just slightly
            // different. The first, `suite`, is the existing suite name to look up in resmoke.
            // We look up the base suite configuration using this value. The second,
            // `sub_task_name`, is the prefix of the suite file that we will write to disk and
            // the generated tasks will use. Since multiple tasks can use the same suite, we
            // cannot use the `suite` value for this, we have to base it off the task name.
            let suite = self.multiversion_service.name_multiversion_suite(
                &suite_info.origin_suite,
                &old_version,
                &version_combination,
            );
            let sub_task_name = self.multiversion_service.name_multiversion_suite(
                &suite_info.task_name,
                &old_version,
                &version_combination,
            );
            let origin_config = self.test_discovery.get_suite_config(&suite).unwrap();

            // Create suite files for all the sub-suites.
            self.write_sub_suites(&suite_info.sub_suites, &origin_config, &sub_task_name);
            // Create a suite file for the '_misc' sub-task.
            self.write_misc_suite(&suite_info.sub_suites, &origin_config, &sub_task_name);
        }
    }

    /// Write resmoke configurations for a standard generated resmoke task.
    ///
    /// # Arguments
    ///
    /// * `suite_info` - Details about the generated task.
    fn write_standard_suite(&self, suite_info: ResmokeSuiteGenerationInfo) {
        let origin_config = self
            .test_discovery
            .get_suite_config(&suite_info.origin_suite)
            .unwrap();

        // Create suite files for all the sub-suites.
        self.write_sub_suites(
            &suite_info.sub_suites,
            &origin_config,
            &suite_info.task_name,
        );

        // Create a suite file for the '_misc' sub-task.
        self.write_misc_suite(
            &suite_info.sub_suites,
            &origin_config,
            &suite_info.task_name,
        );
    }

    /// Write resmoke configurations for the given sub-suites.
    ///
    /// # Arguments
    ///
    /// * `sub_suites` - List of sub-suites to write configuration for.
    /// * `origin_config` - Configuration to base sub-suite configuration on.
    /// * `target_name` - Name to base generated file on.
    fn write_sub_suites(
        &self,
        sub_suites: &[SubSuite],
        origin_config: &ResmokeSuiteConfig,
        target_name: &str,
    ) {
        sub_suites.iter().for_each(|s| {
            let config = origin_config.with_new_tests(Some(&s.test_list), None);
            let mut path = PathBuf::from(&self.target_dir);
            path.push(format!("{}_{}.yml", target_name, s.index.unwrap()));

            self.fs_service
                .write_file(&path, &config.to_string())
                .unwrap();
        });
    }

    /// Write resmoke configurations for a "_misc" suite.
    ///
    /// # Arguments
    ///
    /// * `sub_suites` - List of sub-suites comprising the generated suite.
    /// * `origin_config` - Configuration to base _misc configuration on.
    /// * `target_name` - Name to base generated file on.
    fn write_misc_suite(
        &self,
        sub_suites: &[SubSuite],
        origin_config: &ResmokeSuiteConfig,
        target_name: &str,
    ) {
        let all_tests: Vec<String> = sub_suites
            .iter()
            .flat_map(|s| s.test_list.clone())
            .collect();
        let misc_config = origin_config.with_new_tests(None, Some(&all_tests));
        let mut path = PathBuf::from(&self.target_dir);
        path.push(format!("{}_misc.yml", target_name));
        self.fs_service
            .write_file(&path, &misc_config.to_string())
            .unwrap();
    }
}

#[async_trait]
pub trait ResmokeConfigActor: Sync + Send {
    /// Send a message to write a configuration file to disk.
    async fn write_sub_suite(&mut self, gen_suite: &ResmokeSuiteGenerationInfo);

    /// Wait for all in-progress writes to be completed before returning.
    async fn flush(&mut self);
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
        multiversion_service: Arc<dyn MultiversionService>,
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
                    multiversion_service.clone(),
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
    async fn flush(&mut self) {
        for sender in &self.senders {
            let (send, recv) = oneshot::channel();
            let msg = ResmokeConfigMessage::Flush(send);
            sender.send(msg).await.unwrap();
            recv.await.unwrap();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashMap, ops::AddAssign, str::FromStr, sync::Mutex};

    use crate::{
        resmoke::resmoke_suite::ResmokeSuiteConfig,
        task_types::{multiversion::MultiversionIterator, resmoke_tasks::SubSuite},
    };

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
        pub call_counts: Arc<Mutex<RefCell<HashMap<String, usize>>>>,
    }
    impl MockFsService {
        pub fn new() -> Self {
            Self {
                call_counts: Arc::new(Mutex::new(RefCell::new(HashMap::new()))),
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

    struct MockMultiversionService {
        old_version: Vec<String>,
        version_combos: Vec<String>,
    }
    impl MultiversionService for MockMultiversionService {
        fn get_version_combinations(&self, _suite_name: &str) -> anyhow::Result<Vec<String>> {
            todo!()
        }

        fn multiversion_iter(
            &self,
            _suite: &str,
        ) -> anyhow::Result<crate::task_types::multiversion::MultiversionIterator> {
            Ok(MultiversionIterator::new(
                &self.old_version,
                &self.version_combos,
            ))
        }

        fn name_multiversion_suite(
            &self,
            base_name: &str,
            old_version: &str,
            version_combination: &str,
        ) -> String {
            format!("{}_{}_{}", base_name, old_version, version_combination)
        }
    }

    fn build_mock_service(
        fs_service: Arc<dyn FsService>,
        old_version: Vec<String>,
        version_combos: Vec<String>,
    ) -> WriteConfigActorImpl {
        let test_discovery = Arc::new(MockTestDiscovery {});
        let multiversion_service = Arc::new(MockMultiversionService {
            old_version,
            version_combos,
        });
        let (_tx, rx) = mpsc::channel(1);

        WriteConfigActorImpl::new(
            test_discovery,
            multiversion_service,
            fs_service,
            rx,
            "target".to_string(),
        )
    }

    #[test]
    fn test_write_suite_files() {
        let fs_service = Arc::new(MockFsService::new());
        let resmoke_config_actor = build_mock_service(fs_service.clone(), vec![], vec![]);
        let suite_info = ResmokeSuiteGenerationInfo {
            task_name: "my_task".to_string(),
            origin_suite: "original_suite".to_string(),
            generate_multiversion_combos: false,
            sub_suites: vec![
                SubSuite {
                    index: Some(0),
                    name: "suite".to_string(),
                    test_list: vec!["test_0.js".to_string(), "test_1.js".to_string()],
                },
                SubSuite {
                    index: Some(1),
                    name: "suite".to_string(),
                    test_list: vec!["test_2.js".to_string(), "test_3.js".to_string()],
                },
            ],
        };

        resmoke_config_actor.write_suite_files(suite_info);

        assert_eq!(fs_service.get_call_counts("target/my_task_0.yml"), 1);
        assert_eq!(fs_service.get_call_counts("target/my_task_1.yml"), 1);
        assert_eq!(fs_service.get_call_counts("target/my_task_misc.yml"), 1);
    }

    #[test]
    fn test_write_multiversion_suite_files() {
        let old_version = vec!["last_lts".to_string(), "continuous".to_string()];
        let version_combos = vec!["new_new_new".to_string(), "old_new_old".to_string()];
        let fs_service = Arc::new(MockFsService::new());
        let resmoke_config_actor = build_mock_service(
            fs_service.clone(),
            old_version.clone(),
            version_combos.clone(),
        );
        let suite_info = ResmokeSuiteGenerationInfo {
            task_name: "my_task".to_string(),
            origin_suite: "original_suite".to_string(),
            generate_multiversion_combos: true,
            sub_suites: vec![
                SubSuite {
                    index: Some(0),
                    name: "suite".to_string(),
                    test_list: vec!["test_0.js".to_string(), "test_1.js".to_string()],
                },
                SubSuite {
                    index: Some(1),
                    name: "suite".to_string(),
                    test_list: vec!["test_2.js".to_string(), "test_3.js".to_string()],
                },
            ],
        };

        resmoke_config_actor.write_suite_files(suite_info.clone());

        for version in old_version {
            for combo in &version_combos {
                for sub_suite in &suite_info.sub_suites {
                    let task_name = format!(
                        "target/{}_{}_{}_{}.yml",
                        &suite_info.task_name,
                        version,
                        combo,
                        sub_suite.index.unwrap()
                    );
                    assert_eq!(fs_service.get_call_counts(&task_name), 1);
                }
            }
        }
    }
}
