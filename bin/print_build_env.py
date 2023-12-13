#!/usr/bin/env python3

#
# Copyright 2019-2021 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

#
# Prints some details about the current build environment.
#
# Example Usage:
#   ./print_build_env.py --ringrtc-version 2.20.3 --webrtc-version 4896
#

from string import Template

try:
    import argparse
    import subprocess
    import os
    import re
    import sys
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


def sh_or_empty(args, **kwargs):
    try:
        return subprocess.check_output(args, **kwargs).decode("UTF-8")
    except FileNotFoundError as e:
        print(e, file=sys.stderr)
        return ""


def determine_git_branch(directory):
    with pushd(directory):
        git_branch_output = sh_or_empty(["git", "branch"])
        git_branch = [line.replace("* ", "") for line in git_branch_output.split("\n") if re.search(r"^\*", line)][0]
        return git_branch


def determine_git_sha(directory):
    with pushd(directory):
        return sh_or_empty(["git", "rev-parse", "HEAD"]).strip("\n")


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
    rustc_version = sh_or_empty(["rustc", "--version"]).strip("\n")
    cargo_version = sh_or_empty(["cargo", "--version"]).strip("\n")
    xcode_version = sh_or_empty(["xcodebuild", "-version"]).strip("\n")
    xcode_path = sh_or_empty(["xcode-select", "-p"]).strip("\n")
    gcc_version = sh_or_empty(["gcc", "-v"], stderr=subprocess.STDOUT).strip("\n")
    osx_version_details = sh_or_empty(["sw_vers"]).strip("\n")
    hostname = sh_or_empty(["scutil", "--get", "ComputerName"]).strip("\n")

    details = template.substitute(
        ringrtc_version=ringrtc_version,
        ringrtc_git_branch=ringrtc_git_branch,
        ringrtc_git_sha=ringrtc_git_sha,
        webrtc_version=webrtc_version,
        webrtc_git_branch=webrtc_git_branch,
        webrtc_git_sha=webrtc_git_sha,
        build_script_git_sha=build_script_git_sha,
        rustc_version=rustc_version,
        cargo_version=cargo_version,
        xcode_version=xcode_version,
        xcode_path=xcode_path,
        gcc_version=gcc_version,
        osx_version_details=osx_version_details,
        hostname=hostname
    )

    return details


def main():
    args = ParseArgs()
    print(get_build_details(args.ringrtc_version, args.webrtc_version))


if __name__ == "__main__":
    main()
