#!/bin/sh

#
# Copyright (C) 2019, 2020 Signal Messenger, LLC.
# All rights reserved.
#
# SPDX-License-Identifier: GPL-3.0-only
#

# Specify WebRTC version.  This corresponds to the
# branch or tag of the signalapp/webrtc repository.
WEBRTC_VERSION="4147d"

RINGRTC_MAJOR_VERSION=2
RINGRTC_MINOR_VERSION=5
RINGRTC_REVISION=1

# Specify RingRTC version to publish.
RINGRTC_VERSION="${RINGRTC_MAJOR_VERSION}.${RINGRTC_MINOR_VERSION}.${RINGRTC_REVISION}"

# Release candidate -- for pre-release versions.  Uncomment to use.
# RC_VERSION="alpha"

# Project version is the combination of the two
PROJECT_VERSION="${OVERRIDE_VERSION:-${RINGRTC_VERSION}}${RC_VERSION:+-$RC_VERSION}"
