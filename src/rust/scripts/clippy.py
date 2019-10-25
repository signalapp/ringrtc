#!/usr/bin/env python

#
# Copyright (C) 2019 Signal Messenger, LLC.
# All rights reserved.
#
# SPDX-License-Identifier: GPL-3.0-only
#

# Simple wrapper for calling "cargo clippy"

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

    stamp_file = sys.argv[1]

    cmd = [ "cargo", "clippy" ] + sys.argv[2:]
    sys.stderr.write("cargo command: " + str(cmd))
    subprocess.check_call(cmd)

    # touch stamp_file
    with open(stamp_file, 'a'):
        os.utime(stamp_file, None)

    return 0


# --------------------
#
# execution check
#
if __name__ == '__main__':
    exit(main())
