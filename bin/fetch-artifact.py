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
    'android': '89e41fd44896467d74e8d7efcc3b4ed2826c9a854adbbd0299092e51e175524e',
    'ios': 'c8eec28a5a923bef48cc34f674863b6212617354928f61141ef3e15657906216',
    'linux-x64-sim': 'fdc4b4d85de136c7ecb944463890e4721e3cf9b65136250e2f8da25a2ff77612',
    'linux-x64': 'b9b64ef2df61386fe4fcd81030078d43050aa497ae844ff65b1fd802cf7cd940',
    'linux-arm64-sim': '1a9151ddc9b4dcaaf3418cea16af03da7a54fb82971b55ae9335e0fb11bd2778',
    'linux-arm64': '69078c9f1083cee35307f4fc3d01083f7e7e4a417c54a3f835c57b580d63f45b',
    'mac-x64-sim': '9a0bf6bf269e5bf2042b329b3f7ccef9d6d300c96d575585b1b8b48d8a06f145',
    'mac-x64': '3919ce43a8334e5991185e4d67d7811118ad86dc06fd66670b51b7f0e6d5ed91',
    'mac-arm64-sim': 'e0c02550a0e095045aba27dc0c05b6178c39b0b5708eafdd9596cc3e916f1bd7',
    'mac-arm64': '2d28d957f7d7a64e6047676ec51925fcce7152613e6206856ad6807047a3f983',
    'windows-x64-sim': '45396cdec550d9e1cbe53794c5bee57a8c444244c6b173288ff9e91d530fbfb8',
    'windows-x64': 'e4fd3420961ba09b17683379cbe3064efc36fe6f4343083894f8f6dea18ca8aa',
    'windows-arm64-sim': 'b78e2462a2c0ca43c21d40ff09260821d6267fc88ef7e547e2208b1c09ec7a36',
    'windows-arm64': '21899f23f8c3635bc708f3f3ad00c7c08cbb6042fb01818b46ef3d8e46bc168e',
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
