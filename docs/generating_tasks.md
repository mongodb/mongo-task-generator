# Generating tasks

Generating tasks is a way to dynamically create tasks in Evergreen builds. This is done via the
['generate.tasks'](https://github.com/evergreen-ci/evergreen/wiki/Project-Commands#generatetasks)
evergreen command.

## Use-cases

The `mongo-task-generator` is used by the [mongodb/mongo](https://github.com/mongodb/mongo) project
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
  name: aggregation_expression_multiversion_fuzzer_gen
  tags: ["aggfuzzer", "multiversion", "require_npm", "random_name"]
  commands:
  - func: "generate resmoke tasks"
    vars:
      <<: *jstestfuzz_config_vars
      num_files: 5
      num_tasks: 5
      suite: generational_fuzzer
      resmoke_args: "--mongodSetParameters='{logComponentVerbosity: {command: 2}}'"
      npm_command: agg-expr-fuzzer
```

When generating a fuzzer most of the variables under the `"generate resmoke tasks"` function will
be passed along to the generated tasks. The one exception is the `num_tasks` variable. This
variable controls how many instances of this task will be created and executed. Since these
generated tasks can be executed independently, they can be executed on multiple hosts in parallel.

In this sample, we would generated 5 tasks of this fuzzer and each of them would create and run 5
fuzzer test files, executing a total of 25 fuzzer tests.

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
[runtime stats](https://github.com/evergreen-ci/evergreen/wiki/REST-V2-Usage#teststats)
for the last 2 weeks and use those to divide up the tests into sub-suite with roughly even
runtimes. It will then generate "sub-tasks" for each of the "sub-suites" to actually run the
tests.

We also generate a sub-suite with the suffix "_misc". Since the generated sub-suites are based
on the runtime history of tests, there is a chance that a test exists that has no history -- for
example, a newly added tests. The "_misc" sub-task will try to run all the tests, but exclude any
tests that were included in the generated sub-tasks. This is used to catch any tasks without test
runtime history.

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
Additionally, the 2 parameters here will impact how the task is generated.

* **suite**: By default, the name of the task (with the `_gen` suffix stripped off) will be used
  to determine which resmoke suite to base the generated sub-tasks on. This can be overwritten with
  the `suite` variable.
* **use_large_distro**: Certain test suites require more machine resources in order to run
  successfully. When generated sub-tasks are run on build_variants with a `large_distro_name`
  expansion defined, they will run on that large distro instead of the default distro by setting
  the `use_large_distro` variable to `"true"`.

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
  tags: ["auth", "multiversion", "no_version_combination", "multiversion_future_git_tag"]
  commands:
  - func: "generate resmoke tasks"
    vars:
      suite: multiversion_auth_future_git_tag
```

A task is marked as a multiversion version task by including `"multiversion"` in the `tags` section
of the task definition. When this tag is present, both the extra setup steps and the generation
of multiversion sub-tasks will be preformed. In order to only perform the extra setup steps
the `"no_version_combinations"` tag should also be included.

## Working with generated tasks

A generated tasks is typically composed of a number of related sub-tasks. Because evergreen does
not actually support the concept of sub-tasks, [display tasks](https://github.com/evergreen-ci/evergreen/wiki/Project-Configuration-Files#display-tasks)
are used to instead.

In evergreen, a display task is a container for a number of "execution tasks". The "execution tasks"
are tasks that actually executed and performed some work. When generating tasks, we group all the
sub-tasks generated from a task definition into a single display task.

Grouping sub-tasks into a single display task provides 2 benefits: (1) the tasks show up as a
single entity in the evergreen UI, and (2) queries to the evergreen API can be made via the
display task, which is important for things like querying the historic test runtime of a task.
