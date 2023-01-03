#!/bin/sh

#
# Copyright 2019-2021 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

# Allow non-exported environment variables
# shellcheck disable=SC2034

if [ -n "${PROJECT_DIR}" ]; then
    SCRIPT_DIR="${PROJECT_DIR}/config"
else
    SCRIPT_DIR=$(dirname "$0")
fi

property() {
    grep "${1}" "${SCRIPT_DIR}/version.properties" | cut -d'=' -f2
}

# Specify WebRTC version.  This corresponds to the
# branch or tag of the signalapp/webrtc repository.
WEBRTC_VERSION=$(property 'webrtc.version')

RINGRTC_MAJOR_VERSION=$(property 'ringrtc.version.major')
RINGRTC_MINOR_VERSION=$(property 'ringrtc.version.minor')
RINGRTC_REVISION=$(property 'ringrtc.version.revision')

# Specify RingRTC version to publish.
RINGRTC_VERSION="${RINGRTC_MAJOR_VERSION}.${RINGRTC_MINOR_VERSION}.${RINGRTC_REVISION}"

# Release candidate -- for pre-release versions.  Uncomment to use.
# RC_VERSION="alpha"

# Project version is the combination of the two
PROJECT_VERSION="${OVERRIDE_VERSION:-${RINGRTC_VERSION}}${RC_VERSION:+-$RC_VERSION}"

echo "WebRTC : ${WEBRTC_VERSION}"
echo "RingRTC: ${RINGRTC_VERSION}"
