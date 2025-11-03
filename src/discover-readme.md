
# Run from mongo:

# this uses rust to discover tests
time ../mongo-task-generator/target/debug/mongo-task-generator --discover core.yml --expansion-file . --discover-directory .
# Need to properly exclude
# Need to make this a function in mongo for both resmoke.py and MGT to call

# this is native resmoke
time buildscripts/resmoke.py test-discovery --suite core

# this calls native resmoke from MGT
time ../mongo-task-generator/target/debug/mongo-task-generator --expansion-file . --discover-directory . --discover-tests core
