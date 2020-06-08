#!/bin/bash

#
# copy_repo.sh <source> <destination>
#
# Copy the given node directory to the artifact repository.
#
# Example:
# ringrtc/src/node$ scripts/copy_repo.sh . ../../../signal-ringrtc-node
#

rsync -avrq \
  --exclude='dist/test' \
  --exclude='node_modules' \
  --exclude='scripts' \
  --exclude='test' \
  --exclude='.gitignore' \
  --exclude='.nvmrc' \
  --exclude='tsconfig.json' \
  --exclude='.eslintignore' \
  --exclude='.eslintrc.js' \
  --exclude='.prettierrc.js' \
  --exclude='tslint.json' \
  $1 $2

# Ensure that the LICENSE file is up to date.
cp -f $1/../../LICENSE $2

# Ensure that the README.md file is up to date.
cp -f $1/../../README.md $2
