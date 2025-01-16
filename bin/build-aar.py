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


DEFAULT_ARCHS = ['arm', 'arm64', 'x86', 'x64']
NINJA_TARGETS = ['ringrtc']
JAR_FILES = [
    'lib.java/sdk/android/libwebrtc.jar',
]
WEBRTC_SO_LIBS = ['libringrtc_rffi.so']
SO_LIBS = WEBRTC_SO_LIBS + ['libringrtc.so']
# Android NDK used in webrtc/src/third_party/android_toolchain/README.chromium
NDK_REVISION = '27.0.12077973'


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
    parser.add_argument('--project-dir',
                        default=os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
                        help='Project root directory')
    parser.add_argument('-b', '--build-dir',
                        required=True,
                        help='Build directory')
    parser.add_argument('-w', '--webrtc-src-dir',
                        required=True,
                        help='WebRTC source root directory')
    parser.add_argument('-o', '--output',
                        default='libringrtc.aar',
                        help='Output AAR file name')
    parser.add_argument('-d', '--debug-build',
                        action='store_true',
                        help='Build a debug version of the AAR.  Default is both')
    parser.add_argument('-r', '--release-build',
                        action='store_true',
                        help='Build a release version of the AAR.  Default is both')
    parser.add_argument('-a', '--arch',
                        default=DEFAULT_ARCHS,
                        choices=DEFAULT_ARCHS,
                        nargs='*',
                        help='CPU architectures to build. Defaults: %(default)s.')
    parser.add_argument('-g', '--extra-gn-args',
                        nargs='*', default=[],
                        help='''Additional GN arguments, passed via `gn --args` switch.
                                These args override anything set internally by this script.''')
    parser.add_argument('-n', '--extra-ninja-flags',
                        nargs='*', default=[],
                        help='''Additional Ninja flags, overriding anything set internally
                                by this script.''')
    parser.add_argument('-f', '--extra-gn-flags',
                        nargs='*', default=[],
                        help='''Additional GN flags, overriding anything set internally
                                by this script.''')
    parser.add_argument('--extra-cargo-flags',
                        nargs='*', default=[],
                        help='Additional Cargo arguments')
    parser.add_argument('-j', '--jobs',
                        default=32,
                        help='Number of parallel ninja jobs to run.')
    parser.add_argument('--gradle-dir',
                        required=True,
                        help='Android gradle directory')
    parser.add_argument('--publish-version',
                        required=True,
                        help='Library version to publish')
    parser.add_argument('--webrtc-version',
                        required=True,
                        help='WebRTC version')
    parser.add_argument('--extra-gradle-args',
                        nargs='*', default=[],
                        help='Additional gradle arguments')
    parser.add_argument('--install-local',
                        action='store_true',
                        help='Install to local maven repo')
    parser.add_argument('--install-dir',
                        help='Install to local directory')
    parser.add_argument('--upload-sonatype-user',
                        help='Upload to remote sonatype repo as user')
    parser.add_argument('--upload-sonatype-password',
                        help='Upload to remote sonatype repo using password')
    parser.add_argument('--signing-keyid',
                        help='''GPG keyId for signing key (8 character short form).
                                See https://docs.gradle.org/current/userguide/signing_plugin.html''')
    parser.add_argument('--signing-password',
                        help='''GPG passphrase for signing key.
                                See https://docs.gradle.org/current/userguide/signing_plugin.html''')
    parser.add_argument('--signing-secret-keyring',
                        help='''Absolute path to the secret key ring file containing signing key.
                                See https://docs.gradle.org/current/userguide/signing_plugin.html''')
    parser.add_argument('--dry-run',
                        action='store_true',
                        help='Dry Run: print what would happen, but do not actually do anything')
    parser.add_argument('-u', '--unstripped',
                        action='store_true',
                        default=True,
                        help='Store the unstripped libraries in the .aar. Default is true')
    parser.add_argument('-c', '--compile-only', dest='disabled_projects',
                        action='append_const', const=Project.AAR,
                        help='Only compile the code, do not build the .aar. Default is false')
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


def RunCmd(dry_run, cmd, cwd=None, stdout=None):
    logging.debug('Running: {}'.format(cmd))
    if dry_run is False:
        subprocess.check_call(cmd, cwd=cwd, stdout=stdout)


def GetArchBuildRoot(build_dir, arch):
    return os.path.join(build_dir, 'android-{}'.format(arch))


def GetArchBuildDir(build_dir, arch, debug_build):
    if debug_build is True:
        build_type = 'debug'
    else:
        build_type = 'release'

    return os.path.join(GetArchBuildRoot(build_dir, arch), '{}'.format(build_type))


def GetOutputDir(build_dir, debug_build):
    if debug_build is True:
        build_type = 'debug'
    else:
        build_type = 'release'

    return os.path.join(build_dir, '{}'.format(build_type))


def GetGradleBuildDir(build_dir):
    return os.path.join(build_dir, 'gradle')


def GetAarAssetDir(build_dir):
    return os.path.join(build_dir, 'aar-assets')


def BuildArch(dry_run, project_dir, webrtc_src_dir, build_dir, arch, debug_build,
              extra_gn_args, extra_gn_flags, extra_ninja_flags, extra_cargo_flags,
              jobs, build_projects, publish_to_maven):

    logging.info('Building: {} ...'.format(arch))

    output_dir = GetArchBuildDir(build_dir, arch, debug_build)
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
            'rtc_libvpx_build_vp9': 'false',
            'rtc_disable_metrics': 'true',
            'rtc_disable_trace_events': 'true',
        }
        if debug_build is True:
            gn_args['is_debug'] = 'true'
            gn_args['symbol_level'] = '2'

        gn_args_string = '--args=' + ' '.join(
            [k + '=' + v for k, v in gn_args.items()] + extra_gn_args)

        gn_total_args = ['gn', 'gen', output_dir, gn_args_string] + extra_gn_flags
        RunCmd(dry_run, gn_total_args, cwd=webrtc_src_dir)

        ninja_args = ['ninja', '-C', output_dir] + NINJA_TARGETS + ['-j', jobs] + extra_ninja_flags
        RunCmd(dry_run, ninja_args, cwd=webrtc_src_dir)

    if Project.RINGRTC in build_projects:
        ndk_dir = os.environ['ANDROID_NDK_HOME']
        with open(os.path.join(ndk_dir, 'source.properties'), "r") as f:
            kvs = {}
            for line in f.readlines():
                key, value = line.split("=")
                kvs[key.strip()] = value.strip()
            if kvs['Pkg.Revision'] != NDK_REVISION and publish_to_maven:
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
            '--manifest-path', os.path.join(project_dir, 'src', 'rust', 'Cargo.toml'),
        ]
        if not debug_build:
            cargo_args += ['--release']
        cargo_args += extra_cargo_flags
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
        RunCmd(dry_run, cargo_args)

        if dry_run:
            return

        # Copy the built library alongside libringrtc_rffi.so.
        shutil.copyfile(
            os.path.join(output_dir, GetCargoTarget(arch), 'debug' if debug_build else 'release', 'libringrtc.so'),
            os.path.join(output_dir, 'lib.unstripped', 'libringrtc.so'))
        # And strip another copy.
        strip_args = [
            '{}/bin/llvm-strip'.format(ndk_toolchain_dir),
            '-s',
            os.path.join(output_dir, 'lib.unstripped', 'libringrtc.so'),
            '-o', os.path.join(output_dir, 'libringrtc.so'),
        ]
        RunCmd(dry_run, strip_args)


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


def CollectWebrtcLicenses(dry_run, project_dir, webrtc_src_dir, build_dir, debug_build, archs):
    assert len(NINJA_TARGETS) == 1, 'need to make this a loop'
    md_gen_args = [
        'vpython3',
        os.path.join('tools_webrtc', 'libs', 'generate_licenses.py'),
        '--target',
        NINJA_TARGETS[0],
        build_dir,
    ] + [GetArchBuildDir(build_dir, arch, debug_build) for arch in archs]
    RunCmd(dry_run, md_gen_args, cwd=webrtc_src_dir)


def ArchiveWebrtc(dry_run, build_dir, debug_build, archs, webrtc_version):
    build_mode = 'debug' if debug_build else 'release'
    archive_name = f'webrtc-{webrtc_version}-android-{build_mode}.tar.bz2'
    logging.info(f'Archiving to {archive_name} ...')
    with tarfile.open(os.path.join(build_dir, archive_name), 'w:bz2') as archive:
        def add(rel_path):
            archive.add(os.path.join(build_dir, rel_path), arcname=rel_path)

        for arch in archs:
            logging.debug('  For arch: {} ...'.format(arch))
            output_arch_rel_path = GetArchBuildDir('.', arch, debug_build)
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


def CreateLibs(dry_run, project_dir, webrtc_src_dir, build_dir, archs, output,
               debug_build, unstripped,
               extra_gn_args, extra_gn_flags, extra_ninja_flags,
               extra_cargo_flags, jobs, build_projects, webrtc_version,
               publish_to_maven):

    for arch in archs:
        BuildArch(dry_run, project_dir, webrtc_src_dir, build_dir, arch,
                  debug_build,
                  extra_gn_args, extra_gn_flags, extra_ninja_flags, extra_cargo_flags,
                  jobs, build_projects, publish_to_maven)

    if Project.WEBRTC in build_projects:
        CollectWebrtcLicenses(dry_run, project_dir, webrtc_src_dir, build_dir, debug_build, archs)

    if Project.WEBRTC_ARCHIVE in build_projects:
        ArchiveWebrtc(dry_run, build_dir, debug_build, archs, webrtc_version)

    # The rest is considered part of the AAR build rather than the WebRTC or
    # RingRTC Rust builds mostly by process of elimination: sometimes we want
    # to do a "compile-only" build that skips assembling the libs/ directory.
    if Project.AAR not in build_projects:
        return

    output_dir = os.path.join(GetOutputDir(build_dir, debug_build),
                              'libs')
    if dry_run is True:
        return

    shutil.rmtree(GetOutputDir(build_dir, debug_build), ignore_errors=True)
    os.makedirs(output_dir)

    for jar in JAR_FILES:
        logging.debug('  Adding jar: {} ...'.format(jar))
        output_arch_dir = GetArchBuildDir(build_dir, archs[0], debug_build)
        shutil.copyfile(os.path.join(output_arch_dir, jar),
                        os.path.join(output_dir, os.path.basename(jar)))

    for arch in archs:
        for lib in SO_LIBS:
            output_arch_dir = GetArchBuildDir(build_dir, arch, debug_build)
            if unstripped is True:
                # package the unstripped libraries
                lib_file = os.path.join('lib.unstripped', lib)
            else:
                lib_file = lib
            target_dir = os.path.join(output_dir, GetABI(arch))
            logging.debug('  Adding lib: {}/{} to {}...'.format(GetABI(arch), lib_file, target_dir))
            os.makedirs(target_dir, exist_ok=True)
            shutil.copyfile(os.path.join(output_arch_dir, lib_file),
                            os.path.join(target_dir,
                                         os.path.basename(lib)))


def CollectAarAssets(dry_run, project_dir, build_dir):
    # Assets in AARs get merged into one directory in the final app,
    # so we have to think about what files we're going to put in here.
    aar_asset_dir = GetAarAssetDir(build_dir)
    if not dry_run:
        shutil.rmtree(aar_asset_dir, ignore_errors=True)

    acknowledgments_dir = os.path.join(aar_asset_dir, 'acknowledgments')
    acknowledgments_file = os.path.join(acknowledgments_dir, 'ringrtc.md')
    logging.debug('Copying RingRTC acknowledgments to {}'.format(aar_asset_dir))
    if not dry_run:
        os.makedirs(acknowledgments_dir)
        shutil.copyfile(os.path.join(project_dir, 'acknowledgments', 'acknowledgments.md'),
                        acknowledgments_file)

    logging.debug('Appending WebRTC acknowledgments')
    acknowledgments_file_for_appending = open(acknowledgments_file, mode='ab') if not dry_run else None
    convert_exec = [
        sys.executable,
        os.path.join(project_dir, 'bin', 'convert_webrtc_acknowledgments.py'),
        '--format', 'md',
        os.path.join(build_dir, 'LICENSE.md'),
    ]
    RunCmd(dry_run, convert_exec, stdout=acknowledgments_file_for_appending)


def PerformBuild(dry_run, extra_gradle_args, version, webrtc_version,
                 gradle_dir, sonatype_user, sonatype_password, publish_to_maven,
                 signing_keyid, signing_password, signing_secret_keyring,
                 build_projects,
                 install_local, install_dir, project_dir, webrtc_src_dir, build_dir,
                 archs, output, debug_build, release_build, unstripped,
                 extra_gn_args, extra_gn_flags, extra_ninja_flags,
                 extra_cargo_flags, jobs):

    build_types = []
    if not (debug_build or release_build):
        # build both
        build_types = ['debug', 'release']
    else:
        if debug_build:
            build_types = ['debug']
        if release_build:
            build_types = build_types + ['release']

    gradle_build_dir = GetGradleBuildDir(build_dir)
    shutil.rmtree(gradle_build_dir, ignore_errors=True)
    gradle_exec = [
        './gradlew',
        '-PringrtcVersion={}'.format(version),
        '-PbuildDir={}'.format(gradle_build_dir),
        '-PassetDir={}'.format(GetAarAssetDir(build_dir)),
    ]

    if sonatype_user is not None:
        gradle_exec.append(
            '-PsignalSonatypeUsername={}'.format(sonatype_user))

    if sonatype_password is not None:
        gradle_exec.append(
            '-PsignalSonatypePassword={}'.format(sonatype_password))

    if signing_keyid is not None:
        gradle_exec.append(
            '-Psigning.keyId={}'.format(signing_keyid))

    if signing_password is not None:
        gradle_exec.append(
            '-Psigning.password={}'.format(signing_password))

    if signing_secret_keyring is not None:
        gradle_exec.append(
            '-Psigning.secretKeyRingFile={}'.format(signing_secret_keyring))

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
        CreateLibs(dry_run, project_dir, webrtc_src_dir, build_dir,
                   archs, output, build_debug, unstripped,
                   extra_gn_args, extra_gn_flags, extra_ninja_flags,
                   extra_cargo_flags, jobs, build_projects, webrtc_version,
                   publish_to_maven)

    if Project.AAR not in build_projects:
        return

    CollectAarAssets(dry_run, project_dir=project_dir, build_dir=build_dir)

    gradle_exec.extend(('assembleDebug' if build_type == 'debug' else 'assembleRelease' for build_type in build_types))

    if install_local is True:
        if 'release' not in build_types:
            raise Exception('The `debug` build type is not supported with '
                            '--install-local. Remove --install-local and build again to '
                            'have a debug AAR created in the Gradle output directory.')

        gradle_exec.append('publishToMavenLocal')

    if publish_to_maven:
        gradle_exec.extend(['publishToSonatype', 'closeAndReleaseSonatypeStagingRepository'])

    gradle_exec.extend(extra_gradle_args)

    # Run gradle
    RunCmd(dry_run, gradle_exec, cwd=gradle_dir)

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
            if dry_run is False:
                shutil.rmtree(dest_dir, ignore_errors=True)
                os.makedirs(os.path.dirname(dest_dir), exist_ok=True)
                shutil.copytree(output_dir, dest_dir)


def clean_dir(directory, dry_run):
    logging.info('Removing: {}'.format(directory))
    if dry_run is False:
        shutil.rmtree(directory, ignore_errors=True)


def has_valid_signing_args(args):
    cli_args = args.signing_keyid is not None and \
        args.signing_password is not None and \
        args.signing_secret_keyring is not None

    env_vars = 'ORG_GRADLE_PROJECT_signingKeyId' in os.environ and \
        'ORG_GRADLE_PROJECT_signingPassword' in os.environ and \
        'ORG_GRADLE_PROJECT_signingKey' in os.environ

    return cli_args or env_vars


def main():

    args = ParseArgs()

    if args.dry_run is True:
        args.verbose = True

    if args.verbose is True:
        log_level = logging.DEBUG
    else:
        log_level = logging.INFO

    logging.basicConfig(level=log_level, format='%(levelname).1s:%(message)s')

    if args.quiet is True:
        logging.disable(logging.CRITICAL)

    build_dir = os.path.abspath(args.build_dir)
    logging.debug('Using build directory: {}'.format(build_dir))

    if args.verbose is True:
        args.extra_ninja_flags = args.extra_ninja_flags + ['-v']
        args.extra_cargo_flags = args.extra_cargo_flags + ['-v']

    build_projects = Project.DEFAULT
    for disabled_project in (args.disabled_projects or []):
        build_projects -= disabled_project
    if args.archive_webrtc:
        build_projects |= Project.WEBRTC_ARCHIVE

    gradle_dir = os.path.abspath(args.gradle_dir)
    logging.debug('Using gradle directory: {}'.format(gradle_dir))

    if args.clean is True:
        for arch in DEFAULT_ARCHS:
            clean_dir(GetArchBuildRoot(build_dir, arch), args.dry_run)
        clean_dir(GetGradleBuildDir(build_dir), args.dry_run)
        for dir in ('debug', 'release', 'javadoc', 'rustdoc', 'rust-lint'):
            clean_dir(os.path.join(build_dir, dir), args.dry_run)
        return 0

    upload_sonatype_user = args.upload_sonatype_user or os.environ.get('ORG_GRADLE_PROJECT_signalSonatypeUsername')
    upload_sonatype_password = args.upload_sonatype_password or os.environ.get('ORG_GRADLE_PROJECT_signalSonatypePassword')
    if upload_sonatype_user is not None or upload_sonatype_password is not None:
        if args.debug_build is True:
            print('ERROR: Only the release build can be uploaded')
            return 1

        if upload_sonatype_user is None or upload_sonatype_password is None:
            print("ERROR: Can't set only one of sonatype username and password.")
            return 1

        if not has_valid_signing_args(args):
            print('ERROR: If uploading to Maven, then all of --signing-keyid, --signing-password, and --signing-secret-keyring must be set, or the following environment variables must be set: ORG_GRADLE_PROJECT_signingKeyId, ORG_GRADLE_PROJECT_signingPassword, and ORG_GRADLE_PROJECT_signingKey.')
            return 1

    publish_to_maven = upload_sonatype_user is not None or \
        upload_sonatype_password is not None

    PerformBuild(args.dry_run, args.extra_gradle_args, args.publish_version, args.webrtc_version,
                 args.gradle_dir,
                 args.upload_sonatype_user, args.upload_sonatype_password, publish_to_maven,
                 args.signing_keyid, args.signing_password, args.signing_secret_keyring,
                 build_projects,
                 args.install_local, args.install_dir,
                 args.project_dir, args.webrtc_src_dir, build_dir, args.arch, args.output,
                 args.debug_build, args.release_build, args.unstripped, args.extra_gn_args,
                 args.extra_gn_flags, args.extra_ninja_flags, args.extra_cargo_flags,
                 str(args.jobs))

    logging.info('''
Version           : {}
Architectures     : {}
Debug Build       : {}
Release Build     : {}
Build Directory   : {}
Stripped Libraries: {}
    '''.format(args.publish_version, args.arch, args.debug_build,
               args.release_build, args.build_dir, not args.unstripped))

    return 0


# --------------------
#
# execution check
#
if __name__ == '__main__':
    exit(main())
