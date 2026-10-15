#!/usr/bin/env python3

#
# Copyright 2019-2021 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

"""
This script generates libringrtc.aar for distribution
"""

# ------------------------------------------------------------------------------
#
# Imports
#

try:
    import argparse
    import enum
    import logging
    import subprocess
    import os
    import platform
    import shutil
    import sys
    import tarfile

except ImportError as e:
    raise ImportError(str(e) + '- required module not found')


ARCHS = ['arm', 'arm64', 'x86', 'x64']
NINJA_TARGETS = ['ringrtc']
JAR_FILES = [
    'lib.java/sdk/android/libwebrtc.jar',
]
WEBRTC_SO_LIBS = ['libringrtc_rffi.so']
SO_LIBS = WEBRTC_SO_LIBS + ['libringrtc.so']
# Android NDK used in webrtc/src/third_party/android_toolchain/README.chromium
NDK_REVISION = '28.0.13004108'
PROJECT_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


class Project(enum.Flag):
    WEBRTC = enum.auto()
    WEBRTC_ARCHIVE = enum.auto()
    RINGRTC = enum.auto()
    AAR = enum.auto()
    DEFAULT = WEBRTC | RINGRTC | AAR

    def __sub__(self, other):
        return self & ~other


# ------------------------------------------------------------------------------
#
# Main
#
def ParseArgs():
    parser = argparse.ArgumentParser(
        description='Build and package libringrtc.aar')
    parser.add_argument('-v', '--verbose',
                        action='store_true',
                        help='Verbose output')
    parser.add_argument('-q', '--quiet',
                        action='store_true',
                        help='Quiet output')
    parser.add_argument('-b', '--build-dir',
                        required=True,
                        help='Build directory')
    parser.add_argument('-w', '--webrtc-src-dir',
                        required=True,
                        help='WebRTC source root directory')
    parser.add_argument('-d', '--debug',
                        action='store_true',
                        help='Build a debug version of the AAR.  Default is both')
    parser.add_argument('-r', '--release',
                        action='store_true',
                        help='Build a release version of the AAR.  Default is both')
    parser.add_argument('--gradle-dir',
                        required=True,
                        help='Android gradle directory')
    parser.add_argument('--publish-version',
                        required=True,
                        help='Library version to publish')
    parser.add_argument('--webrtc-version',
                        required=True,
                        help='WebRTC version')
    parser.add_argument('--install-local',
                        action='store_true',
                        help='Install to local maven repo')
    parser.add_argument('--install-dir',
                        help='Install to local directory')
    parser.add_argument('--webrtc-only', dest='disabled_projects',
                        action='append_const', const=Project.RINGRTC | Project.AAR,
                        help='''Compile WebRTC's libraries only, then stop building''')
    parser.add_argument('--ringrtc-only', dest='disabled_projects',
                        action='append_const', const=Project.WEBRTC,
                        help='Compile RingRTC only, assuming WebRTC is already built')
    parser.add_argument('--archive-webrtc',
                        action='store_true',
                        help='After building WebRTC, archive its libraries')
    parser.add_argument('--clean',
                        action='store_true',
                        help='Remove all the build products. Default is false')

    return parser.parse_args()


def RunCmd(cmd, cwd=None, stdout=None):
    logging.debug('Running: {}'.format(cmd))
    subprocess.check_call(cmd, cwd=cwd, stdout=stdout)


def GetArchBuildRoot(build_dir, arch):
    return os.path.join(build_dir, 'android-{}'.format(arch))


def StringifyDebug(debug):
    if debug is True:
        return 'debug'
    else:
        return 'release'


def GetArchBuildDir(build_dir, arch, debug):
    return os.path.join(GetArchBuildRoot(build_dir, arch),
                        StringifyDebug(debug))


def GetOutputDir(build_dir, debug):
    return os.path.join(build_dir, StringifyDebug(debug))


def GetGradleBuildDir(build_dir):
    return os.path.join(build_dir, 'gradle')


def GetAarAssetDir(build_dir):
    return os.path.join(build_dir, 'aar-assets')


def BuildArch(webrtc_src_dir, build_dir, arch, debug,
              build_projects, publish_to_gcs):

    logging.info('Building: {} ...'.format(arch))

    output_dir = GetArchBuildDir(build_dir, arch, debug)
    if Project.WEBRTC in build_projects:
        gn_args = {
            'target_os': '"android"',
            'target_cpu': '"{}"'.format(arch),
            'is_debug': 'false',
            'rtc_include_tests': 'false',
            'rtc_build_examples': 'false',
            'rtc_build_tools': 'false',
            'rtc_enable_protobuf': 'false',
            'rtc_enable_sctp': 'false',
            'rtc_libvpx_build_vp9': 'true',
            'rtc_disable_metrics': 'true',
            'rtc_disable_trace_events': 'true',
            'android_static_analysis': '"off"',
            'use_siso': 'true',
            'rtc_opus_support_dred': 'true',
            'rtc_video_psnr': 'false',
            'use_debug_fission': 'false'
        }
        if debug is True:
            gn_args['is_debug'] = 'true'
            gn_args['symbol_level'] = '2'

        gn_args_string = '--args=' + ' '.join(
            [k + '=' + v for k, v in gn_args.items()])

        webrtc_output_dir = GetArchBuildDir(os.path.join(webrtc_src_dir, 'out'), arch, debug)
        gn_total_args = ['gn', 'gen', webrtc_output_dir, gn_args_string]
        RunCmd(gn_total_args, cwd=webrtc_src_dir)

        ninja_args = ['third_party/siso/cipd/siso', 'ninja', '-C', webrtc_output_dir] + NINJA_TARGETS
        RunCmd(ninja_args, cwd=webrtc_src_dir)

        # for each arch we need:
        # * JAR_FILES (which will be in a nested subdirectory,
        #     and should stay that way!)
        # * WEBRTC_SO_LIBS
        shutil.copytree(webrtc_output_dir, output_dir, dirs_exist_ok=True, ignore_dangling_symlinks=True)
        for jar in JAR_FILES:
            d = os.path.basename(jar)
            os.makedirs(d, exist_ok=True)
            shutil.copyfile(os.path.join(webrtc_output_dir, jar),
                            os.path.join(output_dir, jar))
        for so in WEBRTC_SO_LIBS:
            shutil.copyfile(os.path.join(webrtc_output_dir, so),
                            os.path.join(output_dir, so))

    if Project.RINGRTC in build_projects:
        ndk_dir = os.environ['ANDROID_NDK_HOME']
        with open(os.path.join(ndk_dir, 'source.properties'), "r") as f:
            kvs = {}
            for line in f.readlines():
                key, value = line.split("=")
                kvs[key.strip()] = value.strip()
            if kvs['Pkg.Revision'] != NDK_REVISION and publish_to_gcs:
                raise Exception('Android NDK must be ' + NDK_REVISION)

        ndk_host_os = platform.system().lower()
        ndk_toolchain_dir = os.path.join(
            ndk_dir,
            'toolchains',
            'llvm',
            'prebuilt',
            ndk_host_os + '-x86_64'  # contains universal binaries on macOS
        )

        cargo_target = GetCargoTarget(arch)
        # Set the linker as an environment variable, so it's available to dependencies as well.
        linker = '{}/bin/{}21-clang'.format(ndk_toolchain_dir, GetClangTarget(arch))
        os.environ['CARGO_TARGET_{}_LINKER'.format(cargo_target.replace('-', '_').upper())] = linker

        cargo_args = [
            'cargo', 'rustc',
            '--target', cargo_target,
            '--target-dir', output_dir,
            '--manifest-path', os.path.join(PROJECT_DIR, 'src', 'rust', 'Cargo.toml'),
        ]
        if not debug:
            cargo_args += ['--release']
        # Arguments directly for rustc
        cargo_args += [
            '--',
            '-C', 'debuginfo=2',
            '-C', 'link-arg=-fuse-ld=lld',
            # Don't try to link against getifaddrs, which isn't available before Android 24
            # As long as we don't call it this should be okay.
            '-C', 'link-arg=-Wl,--defsym=getifaddrs=0',
            '-C', 'link-arg=-Wl,--defsym=freeifaddrs=0',
            '-L', 'native=' + output_dir,
        ]

        # Use 16KB pages for 64-bit platforms
        if arch in ['arm64', 'x64']:
            cargo_args += ['-C', 'link-arg=-Wl,-z,max-page-size=16384']

        RunCmd(cargo_args)

        # Copy the built library alongside libringrtc_rffi.so.
        shutil.copyfile(
            os.path.join(output_dir, GetCargoTarget(arch), StringifyDebug(debug), 'libringrtc.so'),
            os.path.join(output_dir, 'lib.unstripped', 'libringrtc.so'))
        # And strip another copy.
        strip_args = [
            '{}/bin/llvm-strip'.format(ndk_toolchain_dir),
            '-s',
            os.path.join(output_dir, 'lib.unstripped', 'libringrtc.so'),
            '-o', os.path.join(output_dir, 'libringrtc.so'),
        ]
        RunCmd(strip_args)


def GetABI(arch):
    if arch == 'arm':
        return 'armeabi-v7a'
    elif arch == 'arm64':
        return 'arm64-v8a'
    elif arch == 'x86':
        return 'x86'
    elif arch == 'x64':
        return 'x86_64'
    else:
        raise Exception('Unknown architecture: ' + arch)


def GetCargoTarget(arch):
    if arch == 'arm':
        return 'armv7-linux-androideabi'
    elif arch == 'arm64':
        return 'aarch64-linux-android'
    elif arch == 'x86':
        return 'i686-linux-android'
    elif arch == 'x64':
        return 'x86_64-linux-android'
    else:
        raise Exception('Unknown architecture: ' + arch)


def GetClangTarget(arch):
    if arch == 'arm':
        return 'armv7a-linux-androideabi'
    else:
        return GetCargoTarget(arch)


def CollectWebrtcLicenses(webrtc_src_dir, build_dir, debug):
    assert len(NINJA_TARGETS) == 1, 'need to make this a loop'
    md_gen_args = [
        'vpython3',
        os.path.join('tools_webrtc', 'libs', 'generate_licenses.py'),
        '--target',
        NINJA_TARGETS[0],
        build_dir,
    ] + [GetArchBuildDir(build_dir, arch, debug) for arch in ARCHS]
    RunCmd(md_gen_args, cwd=webrtc_src_dir)


def ArchiveWebrtc(build_dir, debug, webrtc_version):
    build_mode = StringifyDebug(debug)
    archive_name = f'webrtc-{webrtc_version}-android-{build_mode}.tar.bz2'
    logging.info(f'Archiving to {archive_name} ...')
    with tarfile.open(os.path.join(build_dir, archive_name), 'w:bz2') as archive:
        def add(rel_path):
            archive.add(os.path.join(build_dir, rel_path), arcname=rel_path)

        for arch in ARCHS:
            logging.debug('  For arch: {} ...'.format(arch))
            output_arch_rel_path = GetArchBuildDir('.', arch, debug)
            # All archs will have the same jars, but storing it in every directory
            # makes it easier to build single-arch RingRTC later.
            # The jars are small anyway.
            for jar in JAR_FILES:
                logging.debug('  Adding jar: {} ...'.format(jar))
                add(os.path.join(output_arch_rel_path, jar))
            for lib in WEBRTC_SO_LIBS:
                logging.debug('  Adding lib: {} ...'.format(lib))
                add(os.path.join(output_arch_rel_path, lib))
                logging.debug('  Adding lib: {} (unstripped) ...'.format(lib))
                add(os.path.join(output_arch_rel_path, 'lib.unstripped', lib))

        logging.debug('  Adding acknowledgments file')
        add('LICENSE.md')


def CreateLibs(webrtc_src_dir, build_dir, debug, build_projects, webrtc_version,
               publish_to_gcs):

    for arch in ARCHS:
        BuildArch(webrtc_src_dir, build_dir, arch, debug, build_projects,
                  publish_to_gcs)

    if Project.WEBRTC in build_projects:
        CollectWebrtcLicenses(webrtc_src_dir, build_dir, debug)

    if Project.WEBRTC_ARCHIVE in build_projects:
        ArchiveWebrtc(build_dir, debug, webrtc_version)

    # The rest is considered part of the AAR build rather than the WebRTC or
    # RingRTC Rust builds mostly by process of elimination: sometimes we want
    # to do a "compile-only" build that skips assembling the libs/ directory.
    if Project.AAR not in build_projects:
        return

    output_dir = os.path.join(GetOutputDir(build_dir, debug),
                              'libs')
    clean_dir(GetOutputDir(build_dir, debug))
    os.makedirs(output_dir)

    for jar in JAR_FILES:
        logging.debug('  Adding jar: {} ...'.format(jar))
        output_arch_dir = GetArchBuildDir(build_dir, ARCHS[0], debug)
        shutil.copyfile(os.path.join(output_arch_dir, jar),
                        os.path.join(output_dir, os.path.basename(jar)))

    for arch in ARCHS:
        for lib in SO_LIBS:
            output_arch_dir = GetArchBuildDir(build_dir, arch, debug)
            # package the unstripped libraries
            lib_file = os.path.join('lib.unstripped', lib)
            target_dir = os.path.join(output_dir, GetABI(arch))
            logging.debug('  Adding lib: {}/{} to {}...'.format(GetABI(arch), lib_file, target_dir))
            os.makedirs(target_dir, exist_ok=True)
            shutil.copyfile(os.path.join(output_arch_dir, lib_file),
                            os.path.join(target_dir,
                                         os.path.basename(lib)))


def CollectAarAssets(build_dir):
    # Assets in AARs get merged into one directory in the final app,
    # so we have to think about what files we're going to put in here.
    aar_asset_dir = GetAarAssetDir(build_dir)
    clean_dir(aar_asset_dir)

    acknowledgments_dir = os.path.join(aar_asset_dir, 'acknowledgments')
    acknowledgments_file = os.path.join(acknowledgments_dir, 'ringrtc.md')
    logging.debug('Copying RingRTC acknowledgments to {}'.format(aar_asset_dir))
    os.makedirs(acknowledgments_dir)
    shutil.copyfile(os.path.join(PROJECT_DIR, 'acknowledgments', 'acknowledgments.md'),
                    acknowledgments_file)

    logging.debug('Appending WebRTC acknowledgments')
    acknowledgments_file_for_appending = open(acknowledgments_file, mode='ab')
    convert_exec = [
        sys.executable,
        os.path.join(PROJECT_DIR, 'bin', 'convert_webrtc_acknowledgments.py'),
        '--format', 'md',
        os.path.join(build_dir, 'LICENSE.md'),
    ]
    RunCmd(convert_exec, stdout=acknowledgments_file_for_appending)


def PerformBuild(version, webrtc_version, gradle_dir, publish_to_gcs,
                 build_projects, install_local, install_dir, webrtc_src_dir,
                 build_dir, debug, release):

    build_types = []
    if not (debug or release):
        # build both
        build_types = ['debug', 'release']
    else:
        if debug:
            build_types = ['debug']
        if release:
            build_types = build_types + ['release']

    gradle_build_dir = GetGradleBuildDir(build_dir)
    clean_dir(gradle_build_dir)
    gradle_exec = [
        './gradlew',
        '-PringrtcVersion={}'.format(version),
        '-PbuildDir={}'.format(gradle_build_dir),
        '-PassetDir={}'.format(GetAarAssetDir(build_dir)),
    ]

    for build_type in build_types:
        if build_type == 'debug':
            build_debug = True
            output_dir = GetOutputDir(build_dir, build_debug)
            lib_dir = os.path.join(output_dir, 'libs')
            gradle_exec = gradle_exec + [
                "-PdebugRingrtcLibDir={}".format(lib_dir),
                "-PwebrtcJar={}/libwebrtc.jar".format(lib_dir),
            ]
        else:
            build_debug = False
            output_dir = GetOutputDir(build_dir, build_debug)
            lib_dir = os.path.join(output_dir, 'libs')
            gradle_exec = gradle_exec + [
                "-PreleaseRingrtcLibDir={}".format(lib_dir),
                "-PwebrtcJar={}/libwebrtc.jar".format(lib_dir),
            ]
        CreateLibs(webrtc_src_dir, build_dir, build_debug, build_projects,
                   webrtc_version, publish_to_gcs)

    if Project.AAR not in build_projects:
        return

    CollectAarAssets(build_dir=build_dir)

    gradle_exec.extend(('assembleDebug' if build_type == 'debug' else 'assembleRelease' for build_type in build_types))

    if install_local is True:
        if 'release' not in build_types:
            raise Exception('The `debug` build type is not supported with '
                            '--install-local. Remove --install-local and build again to '
                            'have a debug AAR created in the Gradle output directory.')

        gradle_exec.append('publishToMavenLocal')

    if publish_to_gcs:
        gradle_exec.append('publish')

    # Run gradle
    RunCmd(gradle_exec, cwd=gradle_dir)

    if install_dir is not None:
        for build_type in build_types:
            if build_type == 'debug':
                build_debug = True
                output_dir = GetOutputDir(build_dir, build_debug)
                dest_dir = os.path.join(install_dir, version, 'android', 'debug')
            else:
                build_debug = False
                output_dir = GetOutputDir(build_dir, build_debug)
                dest_dir = os.path.join(install_dir, version, 'android', 'release')

            logging.info('Installing locally to: {}'.format(dest_dir))
            clean_dir(dest_dir)
            os.makedirs(os.path.dirname(dest_dir), exist_ok=True)
            shutil.copytree(output_dir, dest_dir)


def clean_dir(directory):
    logging.info('Removing: {}'.format(directory))
    shutil.rmtree(directory, ignore_errors=True)


def has_valid_signing_args(args):
    """Checks for the presence of the required signing environment variables.

    ORG_GRADLE_PROJECT_signingKeyId: GPG keyId for signing key (8 character short form).
    ORG_GRADLE_PROJECT_signingPassword: GPG passphrase for signing key.
    ORG_GRADLE_PROJECT_signingKey: Absolute path to the secret key ring file containing signing key.

    See https://docs.gradle.org/current/userguide/signing_plugin.html
    """

    return ('ORG_GRADLE_PROJECT_signingKeyId' in os.environ
            and 'ORG_GRADLE_PROJECT_signingPassword' in os.environ
            and 'ORG_GRADLE_PROJECT_signingKey' in os.environ)


def main():

    args = ParseArgs()

    if args.verbose is True:
        log_level = logging.DEBUG
    else:
        log_level = logging.INFO

    logging.basicConfig(level=log_level, format='%(levelname).1s:%(message)s')

    if args.quiet is True:
        logging.disable(logging.CRITICAL)

    build_dir = os.path.abspath(args.build_dir)
    logging.debug('Using build directory: {}'.format(build_dir))

    build_projects = Project.DEFAULT
    for disabled_project in (args.disabled_projects or []):
        build_projects -= disabled_project
    if args.archive_webrtc:
        build_projects |= Project.WEBRTC_ARCHIVE

    gradle_dir = os.path.abspath(args.gradle_dir)
    logging.debug('Using gradle directory: {}'.format(gradle_dir))

    if args.clean is True:
        for arch in ARCHS:
            clean_dir(GetArchBuildRoot(build_dir, arch))
        clean_dir(GetGradleBuildDir(build_dir))
        for dir in ('debug', 'release', 'javadoc', 'rustdoc', 'rust-lint'):
            clean_dir(os.path.join(build_dir, dir))
        return 0

    # The CLOUDSDK_AUTH_ACCESS_TOKEN environment variable needs to be set if publishing.
    publish_to_gcs = os.environ.get('CLOUDSDK_AUTH_ACCESS_TOKEN') is not None
    if publish_to_gcs:
        if args.debug is True:
            print('ERROR: Only the release build can be uploaded')
            return 1

        if not has_valid_signing_args(args):
            print('ERROR: If uploading to GCS, then all of the following '
                  'environment variables must be set: '
                  'ORG_GRADLE_PROJECT_signingKeyId, '
                  'ORG_GRADLE_PROJECT_signingPassword, and '
                  'ORG_GRADLE_PROJECT_signingKey.')
            return 1

    PerformBuild(args.publish_version, args.webrtc_version, args.gradle_dir,
                 publish_to_gcs, build_projects, args.install_local,
                 args.install_dir, args.webrtc_src_dir, build_dir, args.debug,
                 args.release)

    logging.info('''
Version           : {}
Architectures     : {}
Debug Build       : {}
Release Build     : {}
Build Directory   : {}
    '''.format(args.publish_version, ARCHS, args.debug, args.release, args.build_dir))

    return 0


# --------------------
#
# execution check
#
if __name__ == '__main__':
    exit(main())
