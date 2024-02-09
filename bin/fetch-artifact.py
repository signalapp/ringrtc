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
    'android': '49d569a1e69630f6fcb5374079a0f04c986f407daf24da9039e7444fc3b601fe',
    'ios': '0c3274547678276b0cf38ee715d73072c2b39e8114ebddf860976d3de63eaa45',
    'linux-arm64': '098a6e95f93e0272035249969df004b7ae5cceb0433c90d40b3e3da9d1d6f4cb',
    'linux-x64': '51c8579c2ea85fb63260b857fc58f771a3a3443ecbcebf2ac1c85532852cf978',
    'mac-arm64': 'a877715351f209e99478e9b5fec5a9c152eb2f18243a8cda0e9485f0140ede61',
    'mac-x64': '52cc7d43e756738af5eb75cda5497028eb1e47ff67170d244c2cea2f034f5061',
    'windows-arm64': '38b44c233cb518dd13a990dffd97efed9691c05df5c2c0b64471af7576bc5304',
    'windows-x64': '94266f6bbe7d2331f330276e08cc7a04039a22507bab6ef76c559b6a8f22b696',
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
