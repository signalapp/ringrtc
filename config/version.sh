#!/bin/sh

#
# Copyright (C) 2019 Signal Messenger, LLC.
# All rights reserved.
#
# SPDX-License-Identifier: GPL-3.0-only
#

# Specify WebRTC upstream version.  This corresponds to the
# "branch-heads" git branch of the webrtc repository.
WEBRTC_VERSION="3987"

RINGRTC_MAJOR_VERSION=0
RINGRTC_MINOR_VERSION=3
RINGRTC_REVISION=3

# Specify RingRTC version to publish.
RINGRTC_VERSION="${RINGRTC_MAJOR_VERSION}.${RINGRTC_MINOR_VERSION}.${RINGRTC_REVISION}"

# Release candidate -- for pre-release versions.  Uncomment to use.
# RC_VERSION="alpha"

# Project version is the combination of the two
PROJECT_VERSION="${OVERRIDE_VERSION:-${RINGRTC_VERSION}}${RC_VERSION:+-$RC_VERSION}"
