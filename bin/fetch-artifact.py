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
    'android': '08c23cd1d0ae69be418592ca5be49caa784c739ab3dea7a739176b415a595418',
    'ios': '91b0d736dba254e8cc531e0a4376e23d6516a72849ba2dd179bb02e3397a1687',
    'linux-x64-sim': 'ed21c9f807ec7bec01f41ecfeee77d726210a81326635725c33c5a163bd05475',
    'linux-x64': '66b8eeba007b8d2b73fdccbb15bf9317f4576d17ed76ef605d29e7ad6af90264',
    'linux-arm64-sim': 'c2f2fd5a0923494dcd5f808e4b67771f6eb5dc88740c3717ffc68c0123bc695b',
    'linux-arm64': '3df466023aab533946bfc941c8d42ea47f3b7355c6bd7d0748feaa9958180440',
    'mac-x64-sim': 'ca9a13cb3b680faea3287c76643e4bcfc18fa3f57b97bad1d3ca72bb998c25d6',
    'mac-x64': 'b7d115a134007b648a3db0cab410def26142a00c32485f0304e8b6c5a398b548',
    'mac-arm64-sim': '972f27f541b4004675c9f4a5f70cbb1eba0e40fbb33c1c2a815707323e24d02d',
    'mac-arm64': '7cec53985b322448c70ca71052d6a334e40ffd0262cb7937d7374cb80ef5a72c',
    'windows-x64-sim': '2f8340c69a6ca4298453267175a4282e44d4f05e68d2315e0010da7cde4a5120',
    'windows-x64': 'd9159b0c803e7e3f1962b562cf2ebd4fce8426ed74462a78654908b55c96075f',
    'windows-arm64-sim': '28af6f7e18e10775931d348ab32e7b625fda95952c0bd18eecade7c1399f483c',
    'windows-arm64': '18fbf91306d2fb339f85381c1f9a324b48cd032e4b135cc776799a2d83811c87',
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
