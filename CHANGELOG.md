# Changelog

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
