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
    'android': '424a774364a86e1e0346114f383f11b0e3aa5e357818bcb4f817e8d4cc138921',
    'ios': '8708820e9568ca3987e357e307227c5c84c50741739c7f72102875783a2bc240',
    'linux-arm64': '58f14bf2e5e53b6f7ca30fff30232428d74fd851d2072d2d4073f970a66fb62c',
    'linux-x64': 'da7b2e92d0835d30737a54ef2b6b797b0ded7b868d3917bede25c35b44d7045d',
    'mac-arm64': 'fa658d2474872bcafb6f4512f44c1072d0d892ca2ddb5afb49c9ec025200eb78',
    'mac-x64': '3309b5aa960cadb69728c652973f7b5fa519fe0e70efa5bd0a4e4b2af23f8204',
    'windows-arm64': '8f88ef2a764b6bb80d199602147d8c82916503f7a62b370d452b1aad9105ba21',
    'windows-x64': '1cbd2162f44b16a853f9ca1d43f2162393946ba13e3fb4394fa5b8709a32d640',
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
    tar = tarfile.open(fileobj=open_archive)
    # Trust the contents because we checked the hash
    tar.extraction_filter = (lambda member, path: member)
    tar.extractall(path=args.output_dir)
    tar.close()


main()
