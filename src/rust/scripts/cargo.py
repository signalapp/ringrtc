#!/usr/bin/env python

#
# Copyright (C) 2019 Signal Messenger, LLC.
# All rights reserved.
#
# SPDX-License-Identifier: GPL-3.0-only
#

# Simple pass through wrapper for calling cargo

# ------------------------------------------------------------------------------
#
# Imports
#

try:
    import os
    import sys
    import subprocess
except ImportError as e:
    raise ImportError(str(e) + "- required module not found")


# ------------------------------------------------------------------------------
#
# Main
#
def main():

    work_dir = os.path.normpath(os.path.join(os.path.dirname(sys.argv[0]), '..'))
    sys.stderr.write("cargo: Entering directory `" + work_dir + "'\n")
    os.chdir(work_dir)
    cmd = [ "cargo" ] + sys.argv[1:]
    sys.stderr.write("cargo command: " + str(cmd))
    subprocess.check_call(cmd)
    return 0


# --------------------
#
# execution check
#
if __name__ == '__main__':
    exit(main())
