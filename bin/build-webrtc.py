#!/usr/bin/env python3

#
# Copyright 2023 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

"""
This script builds webrtc artifacts for the specified target
"""

try:
    import argparse
    import logging
    import platform
    import subprocess
    import os

except ImportError as e:
    raise ImportError(str(e) + '- required module not found')

TARGET_PLATFORMS = ['android', 'ios', 'linux', 'mac', 'windows']


def parse_args():
    parser = argparse.ArgumentParser(
        description='Build webrtc')
    parser.add_argument('--clean',
                        action='store_true',
                        help='Remove all the build products. Default is false')
    parser.add_argument('--debug',
                        action='store_true',
                        help='Build a debug version. Default is both')
    parser.add_argument('--dry-run',
                        action='store_true',
                        help='Dry Run: print what would happen, but do not actually do anything')
    parser.add_argument('--target',
                        required=True,
                        help='build target: ' + ', '.join(TARGET_PLATFORMS))
    parser.add_argument('--build-for-simulator',
                        action='store_true',
                        help='Also build simulator version. Only for Desktop platforms. Default is only non-simulator version')
    parser.add_argument('--release',
                        action='store_true',
                        help='Build a release version. Default is both')
    parser.add_argument('-v', '--verbose',
                        action='store_true',
                        help='Verbose output')
    return parser.parse_args()


def run_cmd(dry_run, cmd, cwd=None, env=os.environ.copy()):
    logging.debug('Running: {}'.format(cmd))
    if dry_run is False:
        subprocess.check_call(cmd, cwd=cwd, env=env)


def verify_build_host_platform(target_platform):
    if target_platform == 'android' or target_platform == 'linux':
        expected_os_major_version = 'Ubuntu 22'
        actual_os = subprocess.check_output(['lsb_release', '--short', '--description']).decode('UTF-8')
        if expected_os_major_version not in actual_os:
            raise Exception(f"Invalid Host OS Major Version. Expected: {expected_os_major_version} Actual: {actual_os}")
    elif target_platform == 'ios' or target_platform == 'mac':
        expected_system = 'Darwin'
        if platform.system() != expected_system:
            raise Exception(f"Invalid Host OS. Expected: {expected_system} Actual: {platform.system()}")
        expected_major_version = '12'
        version = subprocess.check_output(['sw_vers', '-productVersion']).decode('UTF-8').rstrip()
        major_version = version.split('.')[0]
        if int(major_version) < int(expected_major_version):
            raise Exception(f"Invalid Host OS version. Expected: {expected_major_version} Actual: {major_version}")
    elif target_platform == 'windows':
        expected_system = 'Windows'
        if platform.system() != expected_system:
            raise Exception(f"Invalid Host OS. Expected: {expected_system} Actual: {platform.system()}")


def main() -> None:
    args = parse_args()

    if args.dry_run is True:
        args.verbose = True

    if args.verbose is True:
        log_level = logging.DEBUG
    else:
        log_level = logging.INFO

    logging.basicConfig(level=log_level, format='%(levelname).1s:%(message)s')

    build_types = []
    if args.debug:
        build_types.append("debug")
    if args.release:
        build_types.append("release")
    if not args.debug and not args.release:
        build_types.append("debug")
        build_types.append("release")

    sim_targets = [[]]
    if args.target in ['android', 'ios'] and args.build_for_simulator:
        raise Exception('Simulator builds are only supported for Desktop platforms')
    elif args.build_for_simulator:
        sim_targets = [["--build-for-simulator"], []]

    logging.info('''
Target platform : {}
Build type      : {}
Sim targets     : {}
    '''.format(args.target, build_types, sim_targets))

    verify_build_host_platform(args.target)

    if args.clean is True:
        run_cmd(args.dry_run, ['make', 'distclean'])
        run_cmd(args.dry_run, ['rm', '-rf', 'out_arm'])

    # Install Chromium depot tools
    if not os.path.isdir('out/depot_tools'):
        run_cmd(args.dry_run, ['mkdir', '-p', 'out'])
        run_cmd(args.dry_run, ['git',
                               'clone',
                               '--depth',
                               '1',
                               'https://chromium.googlesource.com/chromium/tools/depot_tools.git',
                               'out/depot_tools'])

    # Add depot tools to PATH environment variable
    cwd = os.getcwd()
    env = os.environ.copy()
    env["PATH"] = f"{cwd}/out/depot_tools:{env['PATH']}"

    if args.target == 'android' or args.target == 'linux':
        # Install build dependencies
        run_cmd(args.dry_run, ['sudo', 'apt', 'install', 'make', 'pkg-config'])

        if args.target == 'android':
            # Prepare workspace
            run_cmd(args.dry_run, ['bin/prepare-workspace', 'android'], env=env)

            # Build WebRTC for android
            for build_type in build_types:
                run_cmd(args.dry_run,
                        ['bin/build-aar', '--webrtc-only', '--archive-webrtc', '--' + build_type],
                        env=env)
        elif args.target == 'linux':
            # Prepare workspace
            run_cmd(args.dry_run, ['bin/prepare-workspace', 'unix'], env=env)

            # Build WebRTC for x86_64
            env['TARGET_ARCH'] = "x64"
            for build_type in build_types:
                for sim_target in sim_targets:
                    run_cmd(args.dry_run,
                            ['bin/build-desktop', '--webrtc-only', '--archive-webrtc', '--' + build_type] + sim_target,
                            env=env)

            # Build WebRTC for arm64
            run_cmd(args.dry_run,
                    ['src/webrtc/src/build/linux/sysroot_scripts/install-sysroot.py', '--arch=arm64'],
                    env=env)
            env['TARGET_ARCH'] = "arm64"
            env['OUTPUT_DIR'] = "out_arm"
            for build_type in build_types:
                for sim_target in sim_targets:
                    run_cmd(args.dry_run,
                            ['bin/build-desktop', '--webrtc-only', '--archive-webrtc', '--' + build_type] + sim_target,
                            env=env)

    elif args.target == 'ios' or args.target == 'mac':
        # Get grealpath
        run_cmd(args.dry_run, ['brew', 'install', 'coreutils'])

        # Assume xcode is already installed
        run_cmd(args.dry_run, ['sudo', 'xcodes', 'select', '15.3'])

        # Accept the license
        run_cmd(args.dry_run, ['sudo', 'xcodebuild', '-license', 'accept'])

        # Install components
        run_cmd(args.dry_run, ['sudo', 'xcodebuild', '-runFirstLaunch'])

        if args.target == 'ios':
            # Prepare workspace
            run_cmd(args.dry_run, ['bin/prepare-workspace', 'ios'], env=env)

            # Ensure this library path exists so that the build succeeds
            run_cmd(args.dry_run, ['sudo', 'mkdir', '-p', '/usr/local/lib'])

            for build_type in build_types:
                run_cmd(args.dry_run,
                        ['bin/build-ios', '--webrtc-only', '--archive-webrtc', '--' + build_type],
                        env=env)

        elif args.target == 'mac':
            # Prepare workspace
            run_cmd(args.dry_run, ['bin/prepare-workspace', 'mac'], env=env)

            env['TARGET_ARCH'] = "x64"
            for build_type in build_types:
                for sim_target in sim_targets:
                    run_cmd(args.dry_run,
                            ['bin/build-desktop', '--webrtc-only', '--archive-webrtc', '--' + build_type] + sim_target,
                            env=env)

            env['TARGET_ARCH'] = "arm64"
            env['OUTPUT_DIR'] = "out_arm"
            for build_type in build_types:
                for sim_target in sim_targets:
                    run_cmd(args.dry_run,
                            ['bin/build-desktop', '--webrtc-only', '--archive-webrtc', '--' + build_type] + sim_target,
                            env=env)

    elif args.target == 'windows':
        bash = 'C:\\Program Files\\Git\\bin\\bash.exe'

        # Prepare workspace
        run_cmd(args.dry_run, [bash, 'bin/prepare-workspace', 'windows'], env=env)

        env['TARGET_ARCH'] = "x64"
        for build_type in build_types:
            for sim_target in sim_targets:
                run_cmd(args.dry_run,
                        [bash, 'bin/build-desktop', '--webrtc-only', '--archive-webrtc', '--' + build_type] + sim_target,
                        env=env)

        # Prepare workspace for arm
        env['OUTPUT_DIR'] = "out_arm"
        run_cmd(args.dry_run, [bash, 'bin/prepare-workspace', 'windows'], env=env)

        env['TARGET_ARCH'] = "arm64"
        for build_type in build_types:
            for sim_target in sim_targets:
                run_cmd(args.dry_run,
                        [bash, 'bin/build-desktop', '--webrtc-only', '--archive-webrtc', '--' + build_type] + sim_target,
                        env=env)


if __name__ == '__main__':
    main()
