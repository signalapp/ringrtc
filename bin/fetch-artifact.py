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
    'android': 'fb04dce57a3a50ec9aa26508bc068b93fed133984112f83a1a92f0c489a858d4',
    'ios': 'fbc2a22ed4f2e37b452c9fd95d535746cd49b5c40f5f2814b7a7884fe761abb2',
    'linux-arm64': 'b8488d3e92ca992f68df37b49c7a3bf4c2108d7ba9108bf32e8b6415b20cb592',
    'linux-x64': '2fbcb67c45d7aaa9d69c1c8f35877f0e3bb120d3474bb0c5e67b992c988050ea',
    'mac-arm64': 'e8a08302e64d5fa396a3dfca4d9fe00cbf090b8e08a55cd7e521ac5a3bd780c0',
    'mac-x64': '08ca3f4d1a84ce9877842250572da0a11a36ad43b9e4600ea66ce1cbbdeff054',
    'windows-arm64': 'c908bbcdf633140b27b3f1a13ac3c26947782a43b3c6a9d3a5d0a4d028b26392',
    'windows-x64': '89cd453eb0ccf8e20c387397605276c945ec854e75f0fa28c78b26942e9466da',
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
        platform_name = resolve_platform(args.platform)
        build_mode = 'debug' if args.debug else 'release'
        url = "https://build-artifacts.signal.org/libraries/webrtc-{}-{}-{}.tar.bz2".format(args.webrtc_version, platform_name, build_mode)
        if not checksum:
            checksum = PREBUILD_CHECKSUMS[platform_name]

    if not checksum:
        parser.error(message='missing --checksum')

    archive_dir = args.archive_dir or args.output_dir

    archive_file = os.path.basename(url)
    open_archive = download_if_needed(archive_file, url, checksum, archive_dir)

    if args.skip_extract:
        return

    print("extracting {} to {}".format(archive_file, args.output_dir), file=sys.stderr)
    open_archive.seek(0)
    tarfile.open(fileobj=open_archive).extractall(path=args.output_dir)


main()
