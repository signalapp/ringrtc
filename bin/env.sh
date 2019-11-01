#!/bin/sh

#
# Copyright (C) 2019 Signal Messenger, LLC.
# All rights reserved.
#
# SPDX-License-Identifier: GPL-3.0-only
#

[ -d "$BIN_DIR" ] || {
    echo "ERROR: project bin directory does not exist: $BIN_DIR"
    exit 1
}

# project root directory
PROJECT_DIR="$(dirname $BIN_DIR)"

# project configuration directory
CONFIG_DIR="${PROJECT_DIR}/config"

# project patches directory
PATCH_DIR="${PROJECT_DIR}/patches/webrtc"

RINGRTC_SRC_DIR="${PROJECT_DIR}/src"

# build products
OUTPUT_DIR="${PROJECT_DIR}/out"

# publish directory
PUBLISH_DIR="${PROJECT_DIR}/publish"

# patch hash file
PATCH_HASH="${OUTPUT_DIR}/patch-hash"

WEBRTC_DIR="${OUTPUT_DIR}/webrtc"
WEBRTC_SRC_DIR="${WEBRTC_DIR}/src"

RINGRTC_WEBRTC_SRC_DIR="${WEBRTC_DIR}/src/ringrtc"

VERSION_INFO="${CONFIG_DIR}/version.sh"
[ -f "$VERSION_INFO" ] || {
    echo "ERROR: unable to load version configuration: $VERSION_INFO"
    exit 1
}
. "$VERSION_INFO"

if [ -f "${OUTPUT_DIR}/webrtc-version.env" ] ; then
    . "${OUTPUT_DIR}/webrtc-version.env"
fi

# This is the release branch of webrtc to check out
WEBRTC_REVISION="branch-heads/${WEBRTC_VERSION}"

# This function should be overriden by a platform specific
# implementation.
prepare_workspace_platform() {
    echo "ERROR: prepare_workspace_platform() is undefined for this platform: $WEBRTC_PLATFORM"
    exit 1
}

create_directory_hash() {
    dir="$1"
    hash_file="$2"

    git ls-files --stage --others --modified --abbrev "$dir" | \
        sha256sum > "$hash_file"

}

check_directory_hash() {
    dir="$1"
    hash_file="$2"

    git ls-files --stage --others --modified --abbrev "$dir" | \
        sha256sum --check --status "$hash_file"

}

# This function should be called by platform specific build scripts to
# verify that the prepared workspace is sane.
check_build_env() {
    # Verify the requested WebRTC version in the version file matches
    # the WebRTC version in the prepared workspace.
    if [ -n "$CONFIGURED_WEBRTC_VERSION" ] ; then
        if [ "$CONFIGURED_WEBRTC_VERSION" != "$WEBRTC_VERSION" ] ; then
            echo "ERROR: previously configured WebRTC version does not match currently selected version."
            echo "  CONFIGURED_WEBRTC_VERSION: $CONFIGURED_WEBRTC_VERSION"
            echo "  WEBRTC_VERSION           : $WEBRTC_VERSION"
            echo
            echo "Recommend running 'make distclean' to start fresh."
            exit 1
        fi
    fi

    # Verify that the patches directory has not changed since the
    # WebRTC workspace was configured.
    if [ -r "$PATCH_HASH" -a -z "$BYPASS_PATCH_CHECK" ] ; then
        if ! check_directory_hash "$PATCH_DIR" "$PATCH_HASH" ; then
            echo "WARNING: The patches applied to the prepared workspace"
            echo "         does not match the current patch directory."
            echo
            echo "Recommend running 'make distclean' to start fresh."
            echo
            echo "NOTE: To bypass this check, set BYPASS_PATCH_CHECK=1 in the environment."
            exit 1
        fi
    fi
}

# current platform if it exists
if [ -f "${OUTPUT_DIR}/platform.env" ] ; then
    . "${OUTPUT_DIR}/platform.env"
fi
if [ -n "$WEBRTC_PLATFORM" ] ; then
    # platform specific env if it exists
    PLATFORM_ENV="${BIN_DIR}/env-${WEBRTC_PLATFORM}.sh"
    if [ -f "$PLATFORM_ENV" ] ; then
        .  "$PLATFORM_ENV"
    else
        echo "ERROR: Unable to find platform specific environment settings: $PLATFORM_ENV"
        exit 1
    fi
fi
