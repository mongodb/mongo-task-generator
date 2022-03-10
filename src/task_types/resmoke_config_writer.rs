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

use crate::{resmoke::resmoke_proxy::TestDiscovery, utils::fs_service::FsService};

use super::resmoke_tasks::ResmokeSuiteGenerationInfo;

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
        let origin_config = self
            .test_discovery
            .get_suite_config(&suite_info.origin_suite)
            .unwrap();

        // Create suite files for all the sub-suites.
        suite_info.sub_suites.iter().for_each(|s| {
            let config = origin_config.with_new_tests(Some(&s.test_list), None);
            let mut path = PathBuf::from(&self.target_dir);
            path.push(format!("{}.yml", s.name));

            self.fs_service
                .write_file(&path, &config.to_string())
                .unwrap();
        });

        // Create a suite file for the '_misc' sub-task.
        let all_tests: Vec<String> = suite_info
            .sub_suites
            .iter()
            .map(|s| s.test_list.clone())
            .flatten()
            .collect();
        let misc_config = origin_config.with_new_tests(None, Some(&all_tests));
        let mut path = PathBuf::from(&self.target_dir);
        path.push(format!("{}_misc.yml", suite_info.task_name));
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

    fn build_mock_service(fs_service: Arc<dyn FsService>) -> WriteConfigActorImpl {
        let test_discovery = Arc::new(MockTestDiscovery {});
        let (_tx, rx) = mpsc::channel(1);

        WriteConfigActorImpl::new(test_discovery, fs_service, rx, "target".to_string())
    }

    #[test]
    fn test_write_suite_files() {
        let fs_service = Arc::new(MockFsService::new());
        let resmoke_config_actor = build_mock_service(fs_service.clone());
        let suite_info = ResmokeSuiteGenerationInfo {
            task_name: "my_task".to_string(),
            origin_suite: "original_suite".to_string(),
            sub_suites: vec![
                SubSuite {
                    name: "suite_0".to_string(),
                    test_list: vec!["test_0.js".to_string(), "test_1.js".to_string()],
                },
                SubSuite {
                    name: "suite_1".to_string(),
                    test_list: vec!["test_2.js".to_string(), "test_3.js".to_string()],
                },
            ],
        };

        resmoke_config_actor.write_suite_files(suite_info);

        assert_eq!(fs_service.get_call_counts("target/suite_0.yml"), 1);
        assert_eq!(fs_service.get_call_counts("target/suite_1.yml"), 1);
        assert_eq!(fs_service.get_call_counts("target/my_task_misc.yml"), 1);
    }
}
