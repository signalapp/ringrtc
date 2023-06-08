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
    'android': '0c6d3c7fb5216ccbc9e62ac215e26a15cce52fe806ede21f46e2cdba1c40c20a',
    'ios': 'fd85379fc6c3e4ad3813d0ffbde34c1e0ab546c3750c0e9210fa62bdfc1f4564',
    'linux-arm64': 'df2481f556863264b0c6395fab9d8c2052995b1cd93016df5fe545f1bd01db2b',
    'linux-x64': '8a2cfdc5efa9d47aac421fbc7c27f98fcbaf9f380b2e462fdaf59b22b4cfe6de',
    'mac-arm64': '5fc992776d036afe6f818693447b6cfae19647bead5a38b4be43ec7867822e22',
    'mac-x64': 'c28cbedee4a5a7fe1ae8015b6d6af4a94dc8bcb3ba2c54f111cb383be229047c',
    'windows-arm64': 'eac196d933a7d89d73527b1593c964285ebc41a1661f9de1dae706b14dc78c2e',
    'windows-x64': '72b1a5cb06039d99d11a6da839eb5250cecf5c2cb10000328cf84e818410d8f1',
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
