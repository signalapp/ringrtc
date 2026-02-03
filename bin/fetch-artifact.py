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
    'android': '520be7a10fa6c796cba29a790c8bf839b2e9f08b06b9314ebb165f0d76bbf7f9',
    'ios': '5421d640ce6f598a42cbc22eeb74223135d6d9869f8929d6bd5acaf0722527ff',
    'linux-x64': '9135254b5b230b50ddbe6787d85715676ccc41636af331bef670303762153c99',
    'linux-arm64': '3e9a4eab524c386fa87a30f3ff2156f375e563bed2b9e3ba5a78917f8dd76e70',
    'mac-x64': '36d19d0ee14a0bbe45dc24298b1b211ec4ee11b7d891c585feb43383e4d8920f',
    'mac-arm64': '30cd81aca0f9c8f971895b584104f8412abd3c7a067f49c5b8322e1b7beebf2f',
    'windows-x64': '51fa1684901009f04684c45f039a9cbf48be2b7cf5242ad730b203adf69bd0c0',
    'windows-arm64': '5cf450c5fc0cb50840e5c18dc97afeb9a17c5ea7b359949b8c9af62adcf7770a',
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
