#!/bin/sh

#
# Copyright 2019-2021 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

# Allow non-exported environment variables
# shellcheck disable=SC2034

# Android specific environment variables
ANDROID_CONFIG_DIR="${CONFIG_DIR}/android"

ANDROID_DEPS_DIR="${OUTPUT_DIR}/android-deps"

# android gradle directory
ANDROID_GRADLE_DIR="${PUBLISH_DIR}/android"

ANDROID_SRC_DIR="${RINGRTC_SRC_DIR}/android"

prepare_workspace_platform() {
    echo "Preparing workspace for Android..."

    "$BIN_DIR"/fetch-android-deps
}
