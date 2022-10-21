#!/bin/sh

#
# Copyright 2019-2022 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

# Windows specific environment variables

# Use the locally-installed Visual Studio rather than depot_tools' hermetic toolchain.
export DEPOT_TOOLS_WIN_TOOLCHAIN=0

prepare_workspace_platform() {
    echo "Preparing workspace for Windows..."

    # @note Nothing here yet.
}
