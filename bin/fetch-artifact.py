#!/usr/bin/env python3

#
# Copyright 2023 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

import argparse
import hashlib
import os
import platform
import ssl
import sys
import tarfile
import urllib.request

from typing import BinaryIO

UNVERIFIED_DOWNLOAD_NAME = "unverified.tmp"

PREBUILD_CHECKSUMS = {
    'android': '3d3085cc67aa1a234a9df2ffca13c885d7131d6da7dd31c2f2eb36ffa1f25675',
    'ios': '04124c80d022663e8ef48620cac381df265f5abca3969019305c0b4fdcc98ed9',
    'linux-x64-sim': '45f27f1fb93c41e33c6a0bbc3621d6d3051c0379fef78b6efbca5c06a6a32b59',
    'linux-x64': 'a00d94cd31d88fb736e7007808b356402141aa51d98598eb10d91093928ce1dd',
    'linux-arm64-sim': '10348fd70fa3660ff0f2114ac2477a9606a38cd9ea3b4cb39781ebe5b77bf3e1',
    'linux-arm64': 'b5386df484f905a96aeea92efe9707f5d15eb3931d1ae49d233abb5c737cefdc',
    'mac-x64-sim': '2706f2d8892ce9a7e45da8c3a2479ea1a9df3919272b938b09d64f603cffb504',
    'mac-x64': 'c566ff58727145b4e0190ec697e63fde5b8d0e92d0e1fd64c00be253bd323052',
    'mac-arm64-sim': '2d0735282950fbece48960ef4939cf85c13537bdb93a0c0d3ea7fc63bf93ef51',
    'mac-arm64': 'e571ca75dd96f5bad5bfa160e52d330a9aac63ee4b5ddff2efd75690aeaa7710',
    'windows-x64-sim': 'c8c1adf65443560f9652290282dae0a3127c51d6196f2618ad030b6a724df3c2',
    'windows-x64': 'a2a04565b8e28bc2646cb54bf064bc28910a6066272ddb37ec8fcc4d86cb47ae',
    'windows-arm64-sim': 'f07553cd7414b3fd8141a303bb3f339fb52d9ee89aceafeeac83e27ed898b760',
    'windows-arm64': 'e48dfdc34a84ddf003cf5d38ed574fd942a027d1fbda4ed7b5bc25084557f51a',
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
    except (urllib.error.HTTPError, urllib.error.URLError) as e:
        if isinstance(e.reason, ssl.SSLCertVerificationError):
            # See:
            #
            # - https://stackoverflow.com/questions/27835619/urllib-and-ssl-certificate-verify-failed-error
            # - https://stackoverflow.com/a/77491061
            print("Failed to verify SSL certificate. Do you need to `pip install pip-system-certs`?", file=sys.stderr)
        else:
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
