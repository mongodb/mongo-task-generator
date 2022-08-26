"""Mock of burn_in_tests.py for testing task generation."""
def burn_in_discovery():
    print("""
discovered_tasks:
- task_name: jsCore
  test_list:
  - tests/data/tests/test_0.js
- task_name: sharding_jscore_passthrough
  test_list:
  - tests/data/tests/test_0.js
- task_name: replica_sets_jscore_passthrough
  test_list:
  - tests/data/tests/test_0.js
    """)


def main():
    burn_in_discovery()


if __name__ == '__main__':
    main()
