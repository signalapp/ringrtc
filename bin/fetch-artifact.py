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
    'android': 'efb5b6a22e81269a33dc0c53aa22496db2d46b841c17e19e62abfae955af6a32',
    'ios': '1beeeb5cac18c0b46577104b50413f9afd016cae6c002302d09308c91b2b5107',
    'linux-x64-sim': '29a0073c8d52dfd52571d1c2d1ae251c12f15404ec89ee69eb4879115d5220fa',
    'linux-x64': '995a264c6a35290d87c254f52a3e46aeb786bb06e19c62d4bcd318e82ff837ca',
    'linux-arm64-sim': '743eb5d141f4fcfc8c8216a5505cc8172904dfe1b41e516102368bd48912ec83',
    'linux-arm64': '5a991262cd71b6fd04a6722282114a8a8da5b1003fe01b009dc87c0b42c5eb82',
    'mac-x64-sim': '31a3124c68a9170e0edc6d11f5125ec2b59727844d6a6c87b13108363a87801d',
    'mac-x64': 'bd43c4cc5e81d8afa4622b6d27a9e951fb9ff42fbe89c0989a62dd5121cb9894',
    'mac-arm64-sim': 'bbfe6422c9feed0efb821d3211acb3713b3b41b0cffeab388c38654a375e12c4',
    'mac-arm64': 'febbb07e676d64def1a3aab76297c914173c6aaa82c5d9405fc134ecd5222b00',
    'windows-x64-sim': '18194b6cf77937adcb968af44b33b3e6132c30de0c93d0bf5a6a868ac6b806e3',
    'windows-x64': '054046f60c77205e187f0ef17597ef127569981573f4ae054e086e12e49df3cd',
    'windows-arm64-sim': 'a3048466bde8032fb9c441637ae4181b8803098c6c43adf7926e9c88e227d1a7',
    'windows-arm64': '08d2d41c86e0375bad8c8363efd5a81ac77943931475b8ddc7d2693832632d0f',
}


def resolve_os(os_name: str) -> str:
    if os_name in ['darwin', 'macos']:
        return 'mac'
    return os_name


def resolve_arch(arch_name: str) -> str:
    if arch_name in ['x86_64', 'amd64']:
        return 'x64'
    if arch_name in ['arm64', 'aarch64']:
        return 'arm64'
    return arch_name


def resolve_platform(platform_name: str) -> str:
    if platform_name in PREBUILD_CHECKSUMS:
        return platform_name
    if platform_name == 'desktop':
        return resolve_platform(platform.system().lower())

    splits = platform_name.split('-')
    os_name = resolve_os(splits[0])
    if len(splits) > 2:
        raise AssertionError('unsupported platform format: ' + platform_name)
    elif len(splits) == 2:
        arch_name = resolve_arch(splits[1])
    else:
        arch_name = resolve_arch(platform.machine().lower())

    resolved_platform_name = "{}-{}".format(os_name, arch_name)
    if resolved_platform_name not in PREBUILD_CHECKSUMS:
        raise AssertionError('unsupported platform: ' + resolved_platform_name)
    return resolved_platform_name


def build_argument_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description='Download and unpack a build artifact archive for a given platform, or from an arbitrary URL.')

    source_group = parser.add_mutually_exclusive_group(required=True)
    source_group.add_argument('-u', '--url',
                              help='URL of an explicitly-specified artifact archive')
    source_group.add_argument('-p', '--platform',
                              help='WebRTC prebuild platform to fetch artifacts for')

    parser.add_argument('--for-simulator', action='store_true',
                        help='get WebRTC prebuild for a Desktop platform')

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
        f_check = open(archive_path, 'rb')
        digest = hashlib.sha256()
        chunk = f_check.read1()
        while chunk:
            digest.update(chunk)
            chunk = f_check.read1()
        if digest.hexdigest() == checksum.lower():
            return f_check
        print("existing file '{}' has non-matching checksum {}; re-downloading...".format(archive_file, digest.hexdigest()), file=sys.stderr)
    except FileNotFoundError:
        pass

    print("downloading {}...".format(archive_file), file=sys.stderr)
    try:
        with urllib.request.urlopen(url) as response:
            digest = hashlib.sha256()
            download_path = os.path.join(archive_dir, UNVERIFIED_DOWNLOAD_NAME)
            f_download = open(download_path, 'w+b')
            chunk = response.read1()
            while chunk:
                digest.update(chunk)
                f_download.write(chunk)
                chunk = response.read1()
            assert digest.hexdigest() == checksum.lower(), "expected {}, actual {}".format(checksum.lower(), digest.hexdigest())
            f_download.close()
            os.replace(download_path, archive_path)
            return open(archive_path, 'rb')
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
        platform_name = resolve_platform(args.platform)
        if platform_name in ["android", "ios"] and args.for_simulator:
            raise Exception("Simulator artifacts are only for desktop platforms")

        build_mode = 'debug' if args.debug else 'release'
        sim = '-sim' if args.for_simulator else ''
        url = "https://build-artifacts.signal.org/libraries/webrtc-{}-{}-{}{}.tar.bz2".format(args.webrtc_version, platform_name, build_mode, sim)
        prebuild_platform_name = "{}{}".format(platform_name, sim)
        if not checksum:
            checksum = PREBUILD_CHECKSUMS[prebuild_platform_name]

    if not checksum:
        parser.error(message='missing --checksum')

    archive_dir = args.archive_dir or args.output_dir

    archive_file = os.path.basename(url)
    open_archive = download_if_needed(archive_file, url, checksum, archive_dir)

    if args.skip_extract:
        return

    print("extracting {} to {}".format(archive_file, args.output_dir), file=sys.stderr)
    open_archive.seek(0)
    tar = tarfile.open(fileobj=open_archive)
    # Trust the contents because we checked the hash
    tar.extraction_filter = (lambda member, path: member)
    tar.extractall(path=args.output_dir)
    tar.close()


main()
