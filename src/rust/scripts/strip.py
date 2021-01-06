#!/usr/bin/env python

#
# Copyright 2019-2021 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

# ------------------------------------------------------------------------------
#
# Imports
#

try:
    import subprocess
    import argparse
except ImportError as e:
    raise ImportError(str(e) + "- required module not found")


# ------------------------------------------------------------------------------
#
# Main
#
def main():

    parser = argparse.ArgumentParser(
        description='strip rust shared library')
    parser.add_argument('-c', '--command',
                        required=True,
                        help='strip executable')
    parser.add_argument('-i', '--input',
                        required=True,
                        help='input binary to strip')
    parser.add_argument('-o', '--output',
                        required=True,
                        help='output, stripped binary')


    args = parser.parse_args()
    strip_cmd = [ args.command,
                  '-o', args.output,
                  args.input
    ]

    subprocess.check_call(strip_cmd)

    return 0


# --------------------
#
# execution check
#
if __name__ == '__main__':
    exit(main())
