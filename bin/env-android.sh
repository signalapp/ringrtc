#!/bin/sh

#
# Copyright (C) 2020 Signal Messenger, LLC.
# All rights reserved.
#
# SPDX-License-Identifier: GPL-3.0-only
#

# Android specific environment variables
NDK_TOOLCHAIN_INSTALL_DIR="${OUTPUT_DIR}/ndk"
NDK_ENV="${NDK_TOOLCHAIN_INSTALL_DIR}/ndk.env"

ANDROID_CONFIG_DIR="${CONFIG_DIR}/android"

ANDROID_DEPS_DIR="${OUTPUT_DIR}/android-deps"

# android gradle directory
ANDROID_GRADLE_DIR="${PUBLISH_DIR}/android"

ANDROID_SRC_DIR="${RINGRTC_SRC_DIR}/android"

prepare_workspace_platform() {
    echo "Preparing workspace for Android..."

    # Setup NDK toolchains
    $BIN_DIR/install-ndk-toolchains

    $BIN_DIR/fetch-android-deps
}
