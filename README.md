# Mongo Task Generator

Dynamically split evergreen tasks into subtasks for testing the mongodb/mongo project.

## Table of contents

- [Mongo Task Generator](#mongo-task-generator)
  - [Table of contents](#table-of-contents)
  - [Description](#description)
  - [Getting Help](#getting-help)
    - [What's the right channel to ask my question?](#whats-the-right-channel-to-ask-my-question)
    - [How can I request a change/report a bug in _Mongo Task Generator_?](#how-can-i-request-a-changereport-a-bug-in-mongo-task-generator)
    - [What should I include in my ticket or question?](#what-should-i-include-in-my-ticket-or-question)
  - [Dependencies](#dependencies)
  - [Installation](#installation)
  - [Usage](#usage)
  - [Documentation](#documentation)
  - [Contributor's Guide](#contributors-guide)
    - [High Level Architecture](#high-level-architecture)
    - [Setting up a local development environment](#setting-up-a-local-development-environment)
    - [linting/formatting](#lintingformatting)
    - [Running tests](#running-tests)
    - [Versioning](#versioning)
    - [Code Review](#code-review)
    - [Deployment](#deployment)
    - [Evergreen configuration](#evergreen-configuration)
  - [Resources](#resources)

## Description

_This project is under construction._

## Getting Help

### What's the right channel to ask my question?

If you have a question about _Mongo Task Generator_, please reach out on slack in the 
#server-testing channel, or email us at dev-prod-dag@mongodb.com.

### How can I request a change/report a bug in _Mongo Task Generator_?

Create a DAG ticket in Jira.

### What should I include in my ticket or question?

Please include as much information as possible. This can help avoid long information-gathering threads.

Please include the following:

* **Motivation for Request**: Why is this change being requested? (This help us understand the priority and urgency of the request)
* **Context**: Is there any background information we should be aware of for this request?
* **Description**: What would you like investigated or changed?


## Dependencies

_TBD_

## Installation

_TBD_

## Usage

_TBD_

## Documentation

_TBD_

## Contributor's Guide

### High Level Architecture

_TBD_

### Setting up a local development environment

_TBD_

### linting/formatting

_TBD_

### Running tests

_TBD_

### Versioning

This project uses [semver](https://semver.org/) for versioning.

Please include a description what is added for each new version in `CHANGELOG.md`.

### Code Review

This project uses the [Evergreen Commit Queue](https://github.com/evergreen-ci/evergreen/wiki/Commit-Queue#pr). 
Add a PR comment with `evergreen merge` to trigger a merge.

### Deployment

_TBD_

### Evergreen configuration

This project uses [evg-bonsai](https://github.com/dbradf/evg-bonsai) for generating its evergreen
configuration. If you need to make a change to the evergreen configuration, change the
[evergreen.landscape.yml](evergreen.landscape.yml) file and then regenerate the configuration
for evergreen with the `evg-bonsai` command.

You should not edit the [evergreen.yml](evergreen.yml) file directly.

You can get the `evg-bonsai` command [here](https://github.com/dbradf/evg-bonsai/releases/latest).

To regenerate the evergreen configuration use the following:

```bash
evg-bonsai build --source-file evergreen.landscape.yml
```

Both the generated `evergreen.yml` and the `evergreen.landscape.yml` files should be checked into
git.

## Resources

* [Evergreen's generate.tasks documentation](https://github.com/evergreen-ci/evergreen/wiki/Project-Commands#generatetasks)
