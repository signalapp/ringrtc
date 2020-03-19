#!/usr/bin/env python

#  Copyright (C) 2019 Signal Messenger, LLC.
#  All rights reserved.
#
#  SPDX-License-Identifier: GPL-3.0-only

##
# Prints some details about the current build environment.
#
# Example Usage:
#   ./print_build_env.py
#

from string import Template

try:
    import argparse
    import subprocess
    import os
    import re
    import contextlib

except ImportError as e:
    raise ImportError(str(e) + "- required module not found")

def ParseArgs():
    parser = argparse.ArgumentParser(
        description='Gather build environment information for reference')
    parser.add_argument('--ringrtc-version',
                        required=True,
                        help='RingRTC library version to publish')
    parser.add_argument('--webrtc-version',
                        required=True,
                        help='WebRTC library version to publish')

    return parser.parse_args()

BIN_DIR = os.path.dirname(__file__)
webrtc_src_dir = os.path.join(BIN_DIR, '../src/webrtc/src')
ringrtc_src_dir = os.path.join(BIN_DIR, '..')

@contextlib.contextmanager
def pushd(new_dir):
    previous_dir = os.getcwd()
    os.chdir(new_dir)
    yield
    os.chdir(previous_dir)

def determine_git_branch(directory):
    with pushd(directory):
        git_branch_output = subprocess.check_output(["git", "branch"])
        git_branch = [line.replace("* ", "") for line in git_branch_output.split("\n") if re.search("^\*", line)][0]
        return git_branch

def determine_git_sha(directory):
    with pushd(directory):
        return subprocess.check_output(["git", "rev-parse", "HEAD"]).strip("\n")

def get_build_details(ringrtc_version, webrtc_version):
    template = Template("""## RingRTC Build Details

To track down potential future issues, we log some of our build environment details.

ringrtc version:
$ringrtc_version

ringrtc git branch:
$ringrtc_git_branch

ringrtc git sha:
$ringrtc_git_sha

webrtc version:
$webrtc_version

webrtc git branch:
$webrtc_git_branch

webrtc git sha:
$webrtc_git_sha

build_script git sha:
$build_script_git_sha

rustc --version:
$rustc_version

cargo --version:
$cargo_version

xcodebuild -version:
$xcode_version

xcode-select -p:
$xcode_path

gcc -v:
$gcc_version

osx_version_details:
$osx_version_details

hostname:
$hostname
""")

    ringrtc_git_branch = determine_git_branch(ringrtc_src_dir)
    ringrtc_git_sha = determine_git_sha(ringrtc_src_dir)
    webrtc_git_branch = determine_git_branch(webrtc_src_dir)
    webrtc_git_sha = determine_git_sha(webrtc_src_dir)
    build_script_git_sha = determine_git_sha("./")
    rustc_version = subprocess.check_output(["rustc", "--version"]).strip("\n")
    cargo_version = subprocess.check_output(["cargo", "--version"]).strip("\n")
    xcode_version = subprocess.check_output(["xcodebuild", "-version"]).strip("\n")
    xcode_path = subprocess.check_output(["xcode-select", "-p"]).strip("\n")
    gcc_version = subprocess.check_output(["gcc", "-v"], stderr=subprocess.STDOUT).strip("\n")
    osx_version_details = subprocess.check_output(["sw_vers"]).strip("\n")
    hostname = subprocess.check_output(["scutil", "--get", "ComputerName"]).strip("\n")

    details = template.substitute(
            ringrtc_version = ringrtc_version,
            ringrtc_git_branch = ringrtc_git_branch,
            ringrtc_git_sha = ringrtc_git_sha,
            webrtc_version = webrtc_version,
            webrtc_git_branch = webrtc_git_branch,
            webrtc_git_sha = webrtc_git_sha,
            build_script_git_sha = build_script_git_sha,
            rustc_version = rustc_version,
            cargo_version = cargo_version,
            xcode_version = xcode_version,
            xcode_path = xcode_path,
            gcc_version = gcc_version,
            osx_version_details = osx_version_details,
            hostname = hostname
    )

    return details

def main():
    args = ParseArgs()
    print(get_build_details(args.ringrtc_version, args.webrtc_version))

if __name__ == "__main__":
    main()
