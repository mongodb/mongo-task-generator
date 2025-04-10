# Changelog
## 0.7.24 - 2025-04-10
* Include fully disabled feature flags when using resmoke's test-discovery

## 0.7.23 - 2025-03-17
* Add a timeout to Evergreen test stats requests

## 0.7.22 - 2025-01-22
* Avoid a division-by-zero when processing empty suites.

## 0.7.21 - 2025-01-16
* Add additional logging for task generation that takes a long time.

## 0.7.20 - 2025-01-16
* DEVPROD-14172 Update shrub-rs dependency that includes papertrail.trace command

## 0.7.19 - 2024-11-18
* DEVPROD-11914 Update shrub-rs dependency that includes ec2.assume_role command

## 0.7.18 - 2024-10-11
* DEVPROD-11978 use --skipTestsCoveredByMoreComplexSuites when generating tasks in a patch build.

## 0.7.17 - 2024-10-09
* DEVPROD-11914 Update shrub-rs dependency that includes github.generate_token command

## 0.7.16 - 2024-08-12
* SERVER-93485 Upgrade Dependencies

## 0.7.15 - 2024-06-25
* DEVPROD-9236 Update burn-in task generation to handle each multiversion sub-suite individually

## 0.7.14 - 2024-05-08
* DEVPROD-7218 Update shrub-rs dependency that includes downstream_expansions.set command

## 0.7.13 - 2024-04-08
* SERVER-88917 Update method for determining enterprise build variants without using modules in mongo-task-generator

## 0.7.12 - 2024-02-29
* DEVPROD-5087 allow variants to override last_versions

## 0.7.11 - 2024-02-16
* DEVPROD-4926 add use_xlarge_distro option to mongo-task-generator

## 0.7.10 - 2022-09-25
* SERVER-81436 Read multiversion config from file instead of resmoke output.

## 0.7.9 - 2022-07-31
* DAG-2777: Added better logging around yaml failures

## 0.7.8 - 2022-05-25
* Update burn in tags API to accommodate all required and suggested buildvariants.

## 0.7.7 - 2022-05-08
* Make generated multiversion tasks explicit.

## 0.7.6 - 2022-05-02
* Pass suite description and matrix_suite to subsuites

## 0.7.5 - 2022-04-25
* Upgrade build artifact for github runner service

## 0.7.4 - 2022-04-14
* Make resmoke errors easier to see in version_gen.

## 0.7.3 - 2022-03-30
* Pass evergreen file location when calling burn_in_tests.py.

## 0.7.2 - 2022-01-25
* Use the compile variant of burn_in_tag_buildvariant for generating burn_in_tags.

## 0.7.1 - 2022-01-12
* Apply appropriate large distros to tasks according to build variant configuration.

## 0.7.0 - 2022-12-16
* Add support for burn_in_tasks generation.

## 0.6.7 - 2022-11-18
* Switch to using evergreen test stats from S3.

## 0.6.6 - 2022-10-23
* Add license and description to Cargo.toml.

## 0.6.5 - 2022-10-23
* Remove workaround for EVG-18112 introduced in 0.6.4.

## 0.6.4 - 2022-10-14
* Update burn_in_tags to depend on existing compile tasks.

## 0.6.3 - 2022-10-07
* Remove _misc task generation.

## 0.6.2 - 2022-09-14
* Propogate up errors when calling resmoke.

## 0.6.1 - 2022-09-06
* Add the ability to get `distro_name` and `task_group_name` for `burn_in` tasks.

## 0.6.0 - 2022-08-26
* Generate only burn_in tasks when --burn-in is passed.

## 0.5.3 - 2022-08-19
* Distribute tests without historic runtime data evenly between subsuites.

## 0.5.2 - 2022-08-17
* Improve task splitting based on historic tests runtime.

## 0.5.1 - 2022-08-12
* Fix parsing the suite name from evergreen.yml for burn_in_* tasks.

## 0.5.0 - 2022-08-01

* Generate tasks separately for Windows, MacOS, Linux distro groups.

## 0.4.7 - 2022-07-14

* Add support for burn_in_tags generation.

## 0.4.6 - 2022-07-01

* Add support for burn_in_tests generation.

## 0.4.5 - 2022-07-01

* Randomize test order when creating all resmoke sub-tasks.

## 0.4.4 - 2022-06-30

* Randomize test order when creating sub-tasks and historic runtime information is not available.

## 0.4.3 - 2022-06-28

* Relax requirement to have the enterprise repo configuration defined.

## 0.4.2 - 2022-06-23

* Remove usage of evg-bonsai for evergreen configuration.

## 0.4.1 - 2022-06-22

* Refactor extraction of evergreen config into a service.

## 0.4.0 - 2022-06-06

* Pass through vars from `generate resmoke tasks` to `run generated tests` func.

## 0.3.6 - 2022-05-16

* Support using the fallback multiversion

## 0.3.5 - 2022-05-16

* Support separate exclude tags for last lts and last continuous.

## 0.3.4 - 2022-04-28

* Properly handle origin suites for multiversion tasks.

## 0.3.3 - 2022-04-28

* Ensure multiversion tags are passed to sub-tasks.

## 0.3.2 - 2022-04-27

* Use fallback split method if historic information is incomplete.

## 0.3.1 - 2022-04-26

* Generate consistent suites names for large multiversion suites.

## 0.3.0 - 2022-04-22

* Use matrix suites for looking up multiversion suite information.

## 0.2.3 - 2022-04-21

* Remember fixture settings for created tasks.
* Normalize test_files returned from evergreen.
* Don't create multiversion _misc suites.

## 0.2.2 - 2022-04-20

* Filter out enterprise tests from non-enterprise build variants.

## 0.2.1 - 2022-04-19

* Separate tasks generated for build variants with the enterprise modules enabled.

## 0.2.0 - 2022-04-18

* Fail tasks that define `use_large_distros`, but don't define `large_distro_name`.

## 0.1.6 - 2022-04-15

* Make evergreen failures result in fallback splitting.

## 0.1.5 - 2022-04-14

* Filter current task from task dependency.

## 0.1.4 - 2022-04-04

* Enforce dependencies from task definitions.
* Fix bug where ~ was not expanded in command arguments.

## 0.1.3 - 2022-03-24

* Improve shrub support and support for multi-argument resmoke.

## 0.1.2 - 2022-03-23

* Improve documentation.

## 0.1.1 - 2022-03-22

* Add integration testing.
