#!/bin/sh

#
# Copyright 2019-2021 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

# Allow non-exported environment variables
# shellcheck disable=SC2034

BIN_DIR=$(dirname "$0")
BIN_DIR=$(realpath -e "$BIN_DIR")

[ -d "$BIN_DIR" ] || {
    echo "ERROR: project bin directory does not exist: $BIN_DIR"
    exit 1
}

# project root directory
PROJECT_DIR=$(dirname "$BIN_DIR")

# project configuration directory
CONFIG_DIR="${PROJECT_DIR}/config"

# project patches directory
PATCH_DIR="${PROJECT_DIR}/patches/webrtc"

RINGRTC_SRC_DIR="${PROJECT_DIR}/src"

# build products
OUTPUT_DIR=$(realpath "${OUTPUT_DIR:-${PROJECT_DIR}/out}")

# publish directory
PUBLISH_DIR="${PROJECT_DIR}/publish"

# patch hash file
PATCH_HASH="${OUTPUT_DIR}/patch-hash"

WEBRTC_DIR="${PROJECT_DIR}/src/webrtc"
WEBRTC_SRC_DIR="${WEBRTC_DIR}/src"

RINGRTC_WEBRTC_SRC_DIR="${WEBRTC_DIR}/src/ringrtc"

VERSION_INFO="${CONFIG_DIR}/version.sh"
[ -f "$VERSION_INFO" ] || {
    echo "ERROR: unable to load version configuration: $VERSION_INFO"
    exit 1
}
# shellcheck source=config/version.sh
. "$VERSION_INFO"

if [ -f "${OUTPUT_DIR}/webrtc-version.env" ] ; then
    # shellcheck disable=SC1090,SC1091 # can't check generated file
    . "${OUTPUT_DIR}/webrtc-version.env"
fi

# This is the release branch of webrtc to check out
WEBRTC_REVISION="branch-heads/${WEBRTC_VERSION}"

# This function should be overridden by a platform specific
# implementation.
prepare_workspace_platform() {
    echo "ERROR: prepare_workspace_platform() is undefined for this platform: $WEBRTC_PLATFORM"
    exit 1
}

INTENDED_WEBRTC_PLATFORM=$WEBRTC_PLATFORM

# current platform if it exists
if [ -f "${OUTPUT_DIR}/platform.env" ] ; then
    # shellcheck disable=SC1090,SC1091 # can't check generated file
    . "${OUTPUT_DIR}/platform.env"
fi
if [ -n "$WEBRTC_PLATFORM" ] ; then

    # don't mix platforms
    if [ -n "$INTENDED_WEBRTC_PLATFORM" ] && [ "$WEBRTC_PLATFORM" != "$INTENDED_WEBRTC_PLATFORM" ] ; then
        echo "ERROR: $WEBRTC_PLATFORM platform already exists, try 'make distclean' first."
        exit 1
    fi

    # platform specific env if it exists
    PLATFORM_ENV="${BIN_DIR}/env-${WEBRTC_PLATFORM}.sh"
    if [ -f "$PLATFORM_ENV" ] ; then
        # shellcheck disable=SC1090 # can't check platform-specific file
        .  "$PLATFORM_ENV"
    else
        echo "ERROR: Unable to find platform specific environment settings: $PLATFORM_ENV"
        exit 1
    fi
fi
