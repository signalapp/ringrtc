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
    'android': '69958caaefe350057cd2c77d001ee99afe646cb8e52a1f24873ef0db2a8f0aba',
    'ios': 'd1bd0e604f60a1799c8453b4c1f374389d5a3572547cc41d7576db1c81482c15',
    'linux-arm64': 'f67592faccf361c099b38e1de35ae2d1956e18e98160543f03f67df9d0418f7d',
    'linux-x64': '48f7259397c3fc7d811087fa4cc649f49a78d9e53f1d2b23cae8c6f1e1c1b4cd',
    'mac-arm64': 'd91539dcb73241702b7eacb4d4c4518d962432eafa2410b718af6c084ed667b4',
    'mac-x64': '461a5aee558c3f7bd43fe4e85fe929bc9b87d8ad58af23c95f2f327603a9dbef',
    'windows-arm64': '97a0190de3797c45f1b86348e139bef5ef910d1255decb4872569853bb53ec95',
    'windows-x64': 'aa1c12170acce307e9d46253f241ec7f91bbb3709939d3c59a8b43f13ae91fcd',
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
