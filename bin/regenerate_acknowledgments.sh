#!/bin/bash

#
# Copyright 2023 Signal Messenger, LLC.
# SPDX-License-Identifier: AGPL-3.0-only
#

set -euo pipefail

SCRIPT_DIR=$(dirname "$0")
cd "${SCRIPT_DIR}"/..

for template in acknowledgments/*.hbs; do
    template_basename=$(basename "${template%.hbs}")
    echo "Generating ${template_basename}" ... >&2
    cargo about generate --config acknowledgments/about.toml --features electron --manifest-path src/rust/Cargo.toml --fail "$template" --output-file "${template%.hbs}"
done
