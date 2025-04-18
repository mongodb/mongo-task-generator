# Generating tasks

Generating tasks is a way to dynamically create tasks in Evergreen builds. This is done via the
['generate.tasks'](https://docs.devprod.prod.corp.mongodb.com/evergreen/Project-Configuration/Project-Commands#generatetasks)
evergreen command.

## Use-cases

The `mongo-task-generator` is used by the [10gen/mongo](https://github.com/10gen/mongo) project
testing to generate most of the dynamic tasks in an evergreen version.

The following 3 use-cases of dynamic task creation are supported:

### Fuzzer coverage

The mongo repository has a number of fuzzer tools that are used in testing.  Each of these
follows a pattern of generating a number of test files that are then executed. One way to
increase the test coverage while maintaining the "wall-clock" runtime is to run multiple
tasks that generate different tests and can be run in parallel. These tasks are dynamically
generated to make it simple to configure.

Looking at a [sample fuzzer configuration](https://github.com/mongodb/mongo/blob/852c5d290fbe141a655501e5fefb23da4ed503c2/etc/evergreen.yml#L3349-L3361),
we can see how this is controlled:

```yaml
- &jstestfuzz_config_vars
  is_jstestfuzz: true
  num_files: 15
  num_tasks: 5  
  resmoke_args: --help 
  resmoke_jobs_max: 1
  should_shuffle: false
  continue_on_failure: false
  timeout_secs: 1800

...

- <<: *jstestfuzz_template
  name: initial_sync_fuzzer_gen
  tags: ["require_npm", "random_name"]
  commands:
  - func: "generate resmoke tasks"
    vars:
      <<: *jstestfuzz_config_vars
      num_files: 10
      num_tasks: 5
      npm_command: initsync-fuzzer
      suite: initial_sync_fuzzer
      resmoke_args: "--mongodSetParameters='{logComponentVerbosity: {command: 2}}'"
```

When generating a fuzzer most of the variables under the `"generate resmoke tasks"` function will
be passed along to the generated tasks. The one exception is the `num_tasks` variable. This
variable controls how many instances of this task will be created and executed. Since these
generated tasks can be executed independently, they can be executed on multiple hosts in parallel.

In this sample, we would generated 5 tasks of this fuzzer and each of them would create and run 10
fuzzer test files, executing a total of 50 fuzzer tests.

It is important to note that the `mongo-task-generator` can tell this is a task it should generate
configuration for because it runs the `"generate resmoke tasks"` function. Additionally, it is able
to tell it should use fuzzer generation logic because the `is_jstestfuzz` variable exists and is
set to `true`.

### Runtime-based sub-tasks

A number of tasks for testing the mongo repository are suites run by [resmoke.py](https://github.com/mongodb/mongo/blob/852c5d290fbe141a655501e5fefb23da4ed503c2/buildscripts/resmoke.py).
These typically consist of a number of jstests that are run against various configuration of
mongo. Some of these suites contain 1000s or even 10,000s of tests and can have runtimes measured
in hours. In order to minimize the wall-clock time of these tasks and prevent them from being a
bottleneck in the overall runtime of a build, we can use dynamic task generation to split these
test suites into sub-suites that can be run in parallel on different hosts.

For tasks appropriately marked, the `mongo-task-generator` will query the
[runtime stats](https://docs.devprod.prod.corp.mongodb.com/evergreen/Project-Configuration/Evergreen-Data-for-Analytics#evergreen-test-statistics)
endpoint https://mongo-test-stats.s3.amazonaws.com/{evg-project-name}/{variant-name}/{task-name}
and use those stats to divide up the tests into sub-suite with roughly even runtimes.
It will then generate "sub-tasks" for each of the "sub-suites" to actually run the tests.

Since the generated sub-suites are based on the runtime history of tests, there is a chance that
a test exists that has no history -- for example, a newly added tests. Such tests will be
distributed with a roughly equal number of tests among all sub-tasks.

If for any reason the runtime history cannot be obtained (e.g. errors in querying, a task having no
runtime history, etc), task splitting will fallback to splitting the tests into sub-tasks that
contains a roughly equal number of tests.

Looking at a [sample resmoke-based](https://github.com/mongodb/mongo/blob/852c5d290fbe141a655501e5fefb23da4ed503c2/etc/evergreen.yml#L4951-L4958) generated task:

```yaml
- <<: *gen_task_template
  name: noPassthrough_gen
  tags: ["misc_js"]
  commands:
  - func: "generate resmoke tasks"
    vars:
      suite: no_passthrough
      use_large_distro: "true"
```

Like fuzzer tasks, task generation is indicated by including the `"generate resmoke tasks"` function.
Additionally, the 4 parameters here will impact how the task is generated.

* **suite**: By default, the name of the task (with the `_gen` suffix stripped off) will be used
  to determine which resmoke suite to base the generated sub-tasks on. This can be overwritten with
  the `suite` variable.
* **use_large_distro**: Certain test suites require more machine resources in order to run
  successfully. When generated sub-tasks are run on build_variants with a `large_distro_name`
  expansion defined, they will run on that large distro instead of the default distro by setting
  the `use_large_distro` variable to `"true"`.
* **use_xlarge_distro**: For when `use_large_distro` is not enough. When the `use_xlarge_distro`
  variable is set to `"true"`, certain tasks will use an even larger distro that can be defined with
  the `xlarge_distro_name` expansion in the build variant. When the `xlarge_distro_name` expansion
  is not defined, it will fallback to the defined `large_distro_name` expansion in the build variant
* **num_tasks**: The number of generated sub-tasks to split into. (Default 5).

**Note**: If a task has the `use_large_distro` value defined, but is added to a build variant
without a `large_distro_name`, it will trigger a failure. This can be supported by using the
`--generate-sub-tasks-config` file. This file should be YAML and supports a list of build variants
that can safely generate `use_large_distro` tasks without a large distro.

The file should look like:

```yaml
build_variant_large_distro_exceptions:
  - build_variant_0
  - build_variant_1
```

### Multiversion testing

We frequently want to run tests suites against configuration with mixed versions of mongo
included. We use generated tasks to create a number of different configurations to run the
tests against.

Multiversion configuration can be included with either fuzzer tasks or resmoke tasks. The
multiversion configurations are applied on top of what is generated in the non-multiversion
execution.

There are two aspects of multiversion generation: (1) Including the necessary steps in task execution
to be able to test against multiple mongo version and (2) generating sub-tasks to actually execute
against mixed version configurations. Certain tasks contain embedded logic to test against multiple
versions and so do not need extra generated configurations.

Looking at a [sample multiversion](https://github.com/mongodb/mongo/blob/852c5d290fbe141a655501e5fefb23da4ed503c2/etc/evergreen.yml#L4321-L4327)
tasks configuration:

```yaml
- <<: *gen_task_template
  name: multiversion_auth_future_git_tag_gen
  tags: ["auth", "multiversion", "no_multiversion_generate_tasks", "multiversion_future_git_tag"]
  commands:
  - func: "generate resmoke tasks"
    vars:
      suite: multiversion_auth_future_git_tag
```

A task is marked as a multiversion version task by including `"multiversion"` in the `tags` section
of the task definition. When this tag is present, both the extra setup steps and the generation
of multiversion sub-tasks will be performed. In order to only perform the extra setup steps
the `"no_multiversion_generate_tasks"` tag should also be included. This is typically used for [explicit multiversion](https://github.com/10gen/mongo/blob/99f7a334eee4b724a231c0db75052eb8199ad8e1/docs/evergreen-testing/multiversion.md#explicit-and-implicit-multiversion-suites) tasks since those suites explicitly test against various mongodb topologies/versions and do not require running additional suites/tasks to ensure multiversion suite converage.

[Implicit multiversion](https://github.com/10gen/mongo/blob/99f7a334eee4b724a231c0db75052eb8199ad8e1/docs/evergreen-testing/multiversion.md#explicit-and-implicit-multiversion-suites) tasks on the other hand must be configured differently to account for various multiversion topologies/version combinations. Here is an example:
```yaml
- <<: *gen_task_template
  name: concurrency_replication_multiversion_gen
  tags: ["multiversion", "multiversion_passthrough"]
  commands:
  - func: "initialize multiversion tasks"
    vars:
      concurrency_replication_last_continuous_new_new_old: last_continuous
      concurrency_replication_last_continuous_new_old_new: last_continuous
      concurrency_replication_last_continuous_old_new_new: last_continuous
      concurrency_replication_last_lts_new_new_old: last_lts
      concurrency_replication_last_lts_new_old_new: last_lts
      concurrency_replication_last_lts_old_new_new: last_lts
  - func: "generate resmoke tasks"
    vars:
      run_no_feature_flag_tests: "true
```
The `"initialize multiversion tasks"` function has all of the related suites to run as sub-tasks of this task as variable names and the "old" version to run against as the values. The absence of the `"no_multiversion_generate_tasks"` tag indicates to the task generator to generate sub-tasks for this task according to the `"initialize multiversion tasks"` function variables. Because the `suite` name is embedded in the `"initialize multiversion tasks"` variables, a `suite` variable passed to `"generate resmoke tasks"` will have no effect. Additionally, the variable/suite names in `"initialize multiversion tasks"` must be globally unique because these are ultimately going to become the sub-task name and evergreen requires task names to be unique.

### Burn in tests, burn in tags and burn in tasks

Newly added or modified tests might become flaky. In order to avoid that, those tests can be run
continuously multiple times in a row to see if the results are consistent. This process is called
burn-in.

#### Burn in tests

`burn_in_tests_gen` task is used to generate burn-in tasks on the same buildvariant the task is
added to. The [example](https://github.com/mongodb/mongo/blob/81c41bdfdc56f05973fae70e80e80919f18f50c9/etc/evergreen_yml_components/definitions.yml#L3252-L3256)
of task configuration:

```yaml
- <<: *gen_task_template
  name: burn_in_tests_gen
  tags: []
  commands:
  - func: "generate resmoke tasks"
```

#### Burn in tags

`burn_in_tags_gen` task is used to generate separate burn-in buildvariants. This way we can burn-in
on the requested buildvariant as well as the other, additional buildvariants to ensure there is no
difference between them.

The [example](https://github.com/mongodb/mongo/blob/81c41bdfdc56f05973fae70e80e80919f18f50c9/etc/evergreen_yml_components/definitions.yml#L4317-L4321)
of task configuration:

```yaml
- <<: *gen_task_template
  name: burn_in_tags_gen
  tags: []
  commands:
  - func: "generate resmoke tasks"
```

`burn_in_tag_include_build_variants` buildvariant expansion is used to configure base buildvariant names.
Base buildvariant names should be delimited by spaces. The [example](https://github.com/mongodb/mongo/blob/81c41bdfdc56f05973fae70e80e80919f18f50c9/etc/evergreen.yml#L1257)
of `burn_in_tag_include_build_variants` buildvariant expansion:

```yaml
burn_in_tag_include_build_variants: enterprise-rhel-80-64-bit-inmem enterprise-rhel-80-64-bit-multiversion
burn_in_tag_compile_task_dependency: archive_dist_test_debug
```

You can also use `burn_in_tag_include_all_required_and_suggested` to bulk add all `!` or `*` prefixed build variants.
And use `burn_in_tag_exclude_build_variants` to exclude build variants.

```yaml
burn_in_tag_include_all_required_and_suggested: true
burn_in_tag_exclude_build_variants: >-
  macos-debug-suggested
burn_in_tag_include_build_variants: >-
  enterprise-rhel-80-64-bit-inmem
  enterprise-rhel-80-64-bit-multiversio
```

#### Burn in tasks

`burn_in_tasks_gen` task is used to generate several copies of the task. The example of task
configuration:

```yaml
- <<: *gen_burn_in_task_template
  name: burn_in_tasks_gen
  tags: []
  commands:
  - func: "generate resmoke tasks"
```

`burn_in_task_name` buildvariant expansion is used to configure which task to burn-in. The
example of `burn_in_task_name` buildvariant expansion:

```yaml
burn_in_task_name: replica_sets_jscore_passthrough
```

WARNING! Task splitting is not supported for burn-in tasks. Large unsplitted `_gen` tasks may
run too long and hit execution timeouts.

Burn-in related tasks are generated when `--burn-in` is passed.

## Working with generated tasks

A generated tasks is typically composed of a number of related sub-tasks. Because evergreen does
not actually support the concept of sub-tasks, [display tasks](https://docs.devprod.prod.corp.mongodb.com/evergreen/Project-Configuration/Project-Configuration-Files#display-tasks)
are used to instead.

In evergreen, a display task is a container for a number of "execution tasks". The "execution tasks"
are tasks that actually executed and performed some work. When generating tasks, we group all the
sub-tasks generated from a task definition into a single display task.

Grouping sub-tasks into a single display task provides 2 benefits: (1) the tasks show up as a
single entity in the evergreen UI, and (2) queries to the evergreen API can be made via the
display task, which is important for things like querying the historic test runtime of a task.

## Generating the configuration

The generate.tasks configuration is generated by running the `mongo-task-generator` command. This
will generate both the generate.tasks configuration and the required resmoke configuration for
generated tasks. All the configuration files will be stored in the "generated_resmoke_config"
directory. The generate.tasks configuration will be the "evergreen_config.json" file.

### expansions-file

In order to execute the command, you must provide an "expansion" file. When running in
evergreen, the [expansions.write](https://docs.devprod.prod.corp.mongodb.com/evergreen/Project-Configuration/Project-Commands#expansionswrite)
command will generate this file for you.

This file should be yaml and must contain the following entries:

* **project**: The evergreen project id of the project being run.
* **revision**: The git revision being run against.
* **task_name**: Name of the task running the generation.
* **version_id**: The evergreen version being run.

A sample file would look like this:

```yaml
project: mongodb-mongo-master
revision: abc123
task_name: generate_version
version_id: 321abc
```

You must provide the expansion file when running the `mongo-task-generator` command:

```bash
mongo-task-generator --expansion-file expansions.yml
```

## Usage help

You can run with the `--help` options to get information on the command usage:

```bash
$ mongo-task-generator --help
Usage: mongo-task-generator [OPTIONS] --expansion-file <EXPANSION_FILE>

Options:
      --evg-project-file <EVG_PROJECT_FILE>
          File containing evergreen project configuration [default: etc/evergreen.yml]
      --expansion-file <EXPANSION_FILE>
          File containing expansions that impact task generation
      --evg-auth-file <EVG_AUTH_FILE>
          File with information on how to authenticate against the evergreen API [default: ~/.evergreen.yml]
      --target-directory <TARGET_DIRECTORY>
          Directory to write generated configuration files [default: generated_resmoke_config]
      --use-task-split-fallback
          Disable evergreen task-history queries and use task splitting fallback
      --resmoke-command <RESMOKE_COMMAND>
          Command to invoke resmoke [default: "python buildscripts/resmoke.py"]
      --generate-sub-tasks-config <GENERATE_SUB_TASKS_CONFIG>
          File containing configuration for generating sub-tasks
      --burn-in
          Generate burn_in related tasks
      --burn-in-tests-command <BURN_IN_TESTS_COMMAND>
          Command to invoke burn_in_tests [default: "python buildscripts/burn_in_tests.py run"]
      --s3-test-stats-endpoint <S3_TEST_STATS_ENDPOINT>
          S3 endpoint to get test stats from [default: https://mongo-test-stats.s3.amazonaws.com]
  -h, --help
          Print help
```
