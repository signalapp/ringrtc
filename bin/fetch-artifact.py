#!/usr/bin/env python3

#
# Copyright 2023 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

import argparse
import hashlib
import os
import platform
import sys
import tarfile
import urllib.request

from typing import BinaryIO

UNVERIFIED_DOWNLOAD_NAME = "unverified.tmp"

PREBUILD_CHECKSUMS = {
    'android': 'e0f54bf50ec84aa359de16166e26ccacf89aa68302f22a16e6b1c38c234bdd11',
    'ios': '4096085cb256dcb5c8d7c0c38a44f4b5e78d9d8ff68ba6b02dc6e9f687906e4f',
    'linux-arm64': '14d9a9696a7625ac6a8a6f7b28d646b9c0868e56f57d9b5a6eecf36027f63b8e',
    'linux-x64': '021e80ee4ede85f1bb57c7ccf7edc777980c6a69d431295fd3565965bd1dfda4',
    'mac-arm64': '0d51658a045f14daea9869c935d3b1e7f74f198da7d08a79e2f8976994f92aff',
    'mac-x64': '8a549973ed6109832518e3e0b363832613254ed82ddb1af989183074b45d96d7',
    'windows-arm64': 'd26579297bd665df6a345c1f8885f0f4b53351679ac67e3b100a25d7a88d8386',
    'windows-x64': '8238bfd4823d507589353989167f309b311391f78fb48437d82c84746e13b18b',
}


def resolve_platform(platform_name: str) -> str:
    if platform_name in PREBUILD_CHECKSUMS:
        return platform_name

    if platform_name in ['windows', 'mac', 'linux']:
        arch_name = platform.machine().lower()
        if arch_name in ['x86_64', 'amd64']:
            return resolve_platform(platform_name + '-x64')
        if arch_name in ['arm64', 'aarch64']:
            return resolve_platform(platform_name + '-arm64')
        raise AssertionError('unsupported architecture: ' + arch_name)

    if platform_name == 'desktop':
        os_name = platform.system().lower()
        if os_name == 'darwin':
            return resolve_platform('mac')
        return resolve_platform(os_name)

    raise AssertionError('unsupported platform: ' + platform_name)


def build_argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description='Download and unpack a build artifact archive for a given platform, or from an arbitrary URL.')

    source_group = parser.add_mutually_exclusive_group(required=True)
    source_group.add_argument('-u', '--url',
                              help='URL of an explicitly-specified artifact archive')
    source_group.add_argument('-p', '--platform',
                              help='WebRTC prebuild platform to fetch artifacts for')

    parser.add_argument('-c', '--checksum',
                        help='sha256sum of the unexpanded artifact archive (can be omitted for standard prebuilds)')
    parser.add_argument('--skip-extract', action='store_true',
                        help='download the archive if necessary, but skip extracting it')

    build_mode_group = parser.add_mutually_exclusive_group()
    build_mode_group.add_argument('--debug', action='store_true',
                                  help='Fetch debug prebuild instead of release')
    build_mode_group.add_argument('--release', action='store_false', dest='debug',
                                  help='Fetch release prebuild (default)')

    parser.add_argument('--webrtc-version', metavar='TAG',
                        help='WebRTC tag, used to identify a prebuild (provided implicitly if running through the shell wrapper)')
    parser.add_argument('--archive-dir',
                        help='Directory to download archives to (defaults to output directory)')
    parser.add_argument('-o', '--output-dir',
                        required=True,
                        help='Build directory (provided implicitly if running through the shell wrapper)')
    return parser


def download_if_needed(archive_file: str, url: str, checksum: str, archive_dir: str) -> BinaryIO:
    archive_path = os.path.join(archive_dir, archive_file)

    try:
        f = open(archive_path, 'rb')
        digest = hashlib.sha256()
        chunk = f.read1()
        while chunk:
            digest.update(chunk)
            chunk = f.read1()
        if digest.hexdigest() == checksum.lower():
            return f
        print("existing file '{}' has non-matching checksum {}; re-downloading...".format(archive_file, digest.hexdigest()), file=sys.stderr)
    except FileNotFoundError:
        pass

    print("downloading {}...".format(archive_file), file=sys.stderr)
    try:
        with urllib.request.urlopen(url) as response:
            digest = hashlib.sha256()
            download_path = os.path.join(archive_dir, UNVERIFIED_DOWNLOAD_NAME)
            f = open(download_path, 'w+b')
            chunk = response.read1()
            while chunk:
                digest.update(chunk)
                f.write(chunk)
                chunk = response.read1()
            assert digest.hexdigest() == checksum.lower(), "expected {}, actual {}".format(checksum.lower(), digest.hexdigest())
            f.close()
            os.replace(download_path, archive_path)
            f = open(archive_path, 'rb')
            return f
    except urllib.error.HTTPError as e:
        print(e, e.filename, file=sys.stderr)
        sys.exit(1)


def main() -> None:
    parser = build_argument_parser()
    args = parser.parse_args()
    os.makedirs(os.path.abspath(args.output_dir), exist_ok=True)

    url = args.url
    checksum = args.checksum
    if not url:
        if not args.webrtc_version:
            parser.error(message='--platform requires --webrtc-version')
        platform = resolve_platform(args.platform)
        build_mode = 'debug' if args.debug else 'release'
        url = "https://build-artifacts.signal.org/libraries/webrtc-{}-{}-{}.tar.bz2".format(args.webrtc_version, platform, build_mode)
        if not checksum:
            checksum = PREBUILD_CHECKSUMS[platform]

    if not checksum:
        parser.error(message='missing --checksum')

    archive_dir = args.archive_dir or args.output_dir

    archive_file = os.path.basename(url)
    open_archive = download_if_needed(archive_file, url, checksum, archive_dir)

    if args.skip_extract:
        return

    print("extracting {}...".format(archive_file), file=sys.stderr)
    open_archive.seek(0)
    tarfile.open(fileobj=open_archive).extractall(path=args.output_dir)


main()
