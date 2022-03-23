"""Mock of resmoke.py for testing task generation."""
import sys


def multiversion_config():
    print("""
last_versions:
- last_lts
- last_continuous
requires_fcv_tag: requires_fcv_51,requires_fcv_52,requires_fcv_53,requires_fcv_60
    """)


def suiteconfig():
    print("""
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
  fixture:
    class: ReplicaSetFixture
    num_nodes: 3
    """)


def test_discovery():
    test_list = "\n".join([f"- tests/data/tests/test_{test}.js" for test in range(15)])
    print(f"""
suite_name: my_suite
tests:
{test_list}
    """)


def main():
    subcommand = sys.argv[1]
    if subcommand == "multiversion-config":
        multiversion_config()
    elif subcommand == "suiteconfig":
        suiteconfig()
    elif subcommand == "test-discovery":
        test_discovery()
    else:
        raise ValueError(f"Unknown subcommand: {subcommand}")


if __name__ == '__main__':
    main()
