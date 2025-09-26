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
    'android': '13c79248735eff97c199c4441536801f23f567b92eebf82eb5d92427badf0566',
    'ios': '74b54b53cad8cc93c357052c9b043e2c8acdcbc96e38b4e7166a897c2c41c829',
    'linux-x64-sim': '0affbe6605ad0a8e5ecace8e15973712cfe639c439fccf6b4bc1f54985ed99a4',
    'linux-x64': '0343e7cae0df194d6f2c4fdddb9d6e6405740f59591a5e75e13690dddcf51805',
    'linux-arm64-sim': '5c359fec9f93877e9203e05af037b5b2e27e948e319cc1df59aaaa638b174310',
    'linux-arm64': '349967550a4a1ea18c3b2f24174a8c00abc0ab08a955a58dbccd0879c54055a2',
    'mac-x64-sim': '399338a88f0e5939e3b2d7e810d15fd76aa0374aa569322e74c735025f79444a',
    'mac-x64': '295615c38b15a5232633d8ea23f20d4fac5b12de9fe343c8097b01feb7a98e13',
    'mac-arm64-sim': 'a3ec690633f9c604b702fcfb1b1632c01f519f4f07409405ea93dd009ff4ee78',
    'mac-arm64': 'fd1d07b31e269f89c72f9fb94ca74bc807754f67b7a33eae44546ff9ef952f3d',
    'windows-x64-sim': '9918dfb1dee28b800163b1ec5e408a005c5776a4afbd8317722965833e0f40f0',
    'windows-x64': 'b8f0c99eb9b510ed5e31977807c80b1cfaef156c7b74417976600faf03ec4ad7',
    'windows-arm64-sim': '8895b0b9c6d9ac6159f1467e782e6488477351eaab2da9bc27b7f448d45ad7e9',
    'windows-arm64': 'b9d57dea1ac9448e7e6eb0e9b93cc53bf682938a566e4d141e518a6860e6d999',
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
