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
    import logging
    import subprocess
    import sys
    import os
    import zipfile
    import shutil

except ImportError as e:
    raise ImportError(str(e) + "- required module not found")


DEFAULT_ARCHS = ['arm', 'arm64', 'x86', 'x64']
NINJA_TARGETS = ['ringrtc']
JAR_FILES     = [
    'lib.java/ringrtc/libringrtc.jar',
    'lib.java/sdk/android/libwebrtc.jar',
]
SO_LIBS       = [
    'libringrtc_rffi.so',
    'libringrtc.so',
]

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
    parser.add_argument('-o', '--output',
                        default = 'libringrtc.aar',
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
    parser.add_argument('-j', '--jobs',
                        default=32,
                        help='Number of parallel ninja jobs to run.')
    parser.add_argument('--gradle-dir',
                        required=True,
                        help='Android gradle directory')
    parser.add_argument('--publish-version',
                        required=True,
                        help='Library version to publish')
    parser.add_argument('--extra-gradle-args',
                        nargs='*', default=[],
                        help='Additional gradle arguments')
    parser.add_argument('--install-local',
                        action='store_true',
                        help='Install to local maven repo')
    parser.add_argument('--install-dir',
                        help='Install to local directory')
    parser.add_argument('--upload-sonatype-repo',
                        help='Upload to remote sonatype repo')
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
                        help='Store the unstripped libraries in the .aar. Default is false')
    parser.add_argument('-c', '--compile-only',
                        action='store_true',
                        help='Only compile the code, do not build the .aar. Default is false')
    parser.add_argument('--clean',
                        action='store_true',
                        help='Remove all the build products. Default is false')

    return parser.parse_args()

def RunSdkmanagerLicenses(dry_run):
    executable = os.path.join('third_party', 'android_sdk', 'public',
                              'cmdline-tools', 'latest', 'bin', 'sdkmanager')
    cmd = [ executable, '--licenses' ]
    logging.debug('Running: {}'.format(cmd))
    if dry_run is False:
        subprocess.check_call(cmd)

def RunGn(dry_run, args):
    cmd = [ 'gn' ] + args
    logging.debug('Running: {}'.format(cmd))
    if dry_run is False:
        subprocess.check_call(cmd)

def RunNinja(dry_run, args):
    cmd = [ 'ninja' ] + args
    logging.debug('Running: {}'.format(cmd))
    if dry_run is False:
        subprocess.check_call(cmd)

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

def BuildArch(dry_run, build_dir, arch, debug_build, extra_gn_args,
              extra_gn_flags, extra_ninja_flags, jobs):

    logging.info('Building: {} ...'.format(arch))

    output_dir = GetArchBuildDir(build_dir, arch, debug_build)
    gn_args = {
        'target_os'           : '"android"',
        'target_cpu'          : '"{}"'.format(arch),
        'is_debug'            : 'false',
        'rtc_include_tests'   : 'false',
        'rtc_build_examples'  : 'false',
        'rtc_build_tools'     : 'false',
        'rtc_enable_protobuf' : 'false',
        'rtc_enable_sctp'     : 'false',
        'rtc_libvpx_build_vp9': 'false',
        'rtc_include_ilbc'    : 'false',
    }
    if debug_build is True:
        gn_args['is_debug'] = 'true'
        gn_args['symbol_level'] = '2'

    gn_args_string = '--args=' + ' '.join(
        [k + '=' + v for k, v in gn_args.items()] + extra_gn_args)

    gn_total_args = [ 'gen', output_dir, gn_args_string ] + extra_gn_flags
    RunGn(dry_run, gn_total_args)

    ninja_args = [ '-C', output_dir ] + NINJA_TARGETS + [ '-j', jobs ] + extra_ninja_flags
    RunNinja(dry_run, ninja_args)

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

def CreateLibs(dry_run, build_dir, archs, output, debug_build, unstripped,
               extra_gn_args, extra_gn_flags, extra_ninja_flags, jobs,
               compile_only):

    for arch in archs:
        BuildArch(dry_run, build_dir, arch, debug_build, extra_gn_args,
                  extra_gn_flags, extra_ninja_flags, jobs)

    if compile_only is True:
        return

    output_dir = os.path.join(GetOutputDir(build_dir, debug_build),
                              'libs')
    output_file = os.path.join(output_dir, output)
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
                lib_file = os.path.join("lib.unstripped", lib)
            else:
                lib_file = lib
            target_dir = os.path.join(output_dir, GetABI(arch))
            logging.debug('  Adding lib: {}/{} to {}...'.format(GetABI(arch), lib_file, target_dir))
            os.makedirs(target_dir, exist_ok=True)
            shutil.copyfile(os.path.join(output_arch_dir, lib_file),
                            os.path.join(target_dir,
                                         os.path.basename(lib)))

def RunGradle(dry_run, args):
    cmd = [ './gradlew' ] + args
    logging.debug('Running: {}'.format(cmd))
    if dry_run is False:
        subprocess.check_call(cmd)

def CreateAar(dry_run, extra_gradle_args, version, gradle_dir,
              sonatype_repo, sonatype_user, sonatype_password,
              signing_keyid, signing_password, signing_secret_keyring,
              compile_only,
              install_local, install_dir, build_dir, archs,
              output, debug_build, release_build, unstripped,
              extra_gn_args, extra_gn_flags, extra_ninja_flags, jobs):

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
    gradle_args = [
        '-PringrtcVersion={}'.format(version),
        '-PbuildDir={}'.format(gradle_build_dir),
    ]

    if sonatype_repo is not None:
        sonatype_args = [
            '-PsonatypeRepo={}'.format(sonatype_repo),
            '-PsignalSonatypeUsername={}'.format(sonatype_user),
            '-PsignalSonatypePassword={}'.format(sonatype_password),
        ]
        gradle_args.extend(sonatype_args)

    if signing_keyid is not None:
        gradle_args.append(
            '-Psigning.keyId={}'.format(signing_keyid))

    if signing_password is not None:
        gradle_args.append(
            '-Psigning.password={}'.format(signing_password))

    if signing_secret_keyring is not None:
        gradle_args.append(
            '-Psigning.secretKeyRingFile={}'.format(signing_secret_keyring))

    for build_type in build_types:
        if build_type == 'debug':
            build_debug = True
            output_dir = GetOutputDir(build_dir, build_debug)
            lib_dir = os.path.join(output_dir, 'libs')
            gradle_args = gradle_args + [
                "-PdebugRingrtcLibDirs=['{}']".format(lib_dir),
            ]
        else:
            build_debug = False
            output_dir = GetOutputDir(build_dir, build_debug)
            lib_dir = os.path.join(output_dir, 'libs')
            gradle_args = gradle_args + [
                "-PreleaseRingrtcLibDirs=['{}']".format(lib_dir),
            ]
        CreateLibs(dry_run, build_dir, archs, output, build_debug, unstripped,
                   extra_gn_args, extra_gn_flags, extra_ninja_flags, jobs,
                   compile_only)

    if compile_only is True:
        return

    gradle_args.extend(('assembleDebug' if build_type == 'debug' else 'assembleRelease' for build_type in build_types))

    if install_local is True:
        if 'release' not in build_types:
            raise Exception('The `debug` build type is not supported with '
                    '--install-local. Remove --install-local and build again to '
                    'have a debug AAR created in the Gradle output directory.')

        gradle_args.append('publishToMavenLocal')

    if sonatype_repo is not None:
        gradle_args.append(':publishMavenJavaPublicationToMavenRepository')

    gradle_args.extend(extra_gradle_args)

    # Run gradle
    os.chdir(os.path.abspath(gradle_dir))
    RunGradle(dry_run, gradle_args)

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

    gradle_dir = os.path.abspath(args.gradle_dir)
    logging.debug('Using gradle directory: {}'.format(gradle_dir))

    if args.clean is True:
        for arch in DEFAULT_ARCHS:
            rm_dir = GetArchBuildRoot(build_dir, arch)
            clean_dir(GetArchBuildRoot(build_dir, arch), args.dry_run)
        clean_dir(GetGradleBuildDir(build_dir), args.dry_run)
        for dir in ('debug', 'release', 'javadoc', 'rustdoc', 'rust-lint'):
            clean_dir(os.path.join(build_dir, dir), args.dry_run)
        return 0

    os.chdir(os.path.abspath(args.webrtc_src_dir))
    RunSdkmanagerLicenses(args.dry_run)

    if args.upload_sonatype_repo is not None:
        if args.debug_build is True or args.release_build is True:
            print('ERROR: When uploading, must upload complete release and debug builds')
            print('ERROR: You cannot specify either --release or --debug while uploading')
            return 1

        if args.upload_sonatype_user is None or args.upload_sonatype_password is None:
            print('ERROR: If --upload-sonatype-repo argument set, then both --upload-sonatype-user and --upload-sonatype-password must also be set.')
            return 1

        if args.signing_keyid is None or \
           args.signing_password is None or \
           args.signing_secret_keyring is None:
            print('ERROR: If --upload-sonatype-repo argument set, then all of --signing-keyid, --signing-password, and --signing-secret-keyring must also be set.')
            return 1

    CreateAar(args.dry_run, args.extra_gradle_args, args.publish_version, args.gradle_dir,
              args.upload_sonatype_repo, args.upload_sonatype_user, args.upload_sonatype_password,
              args.signing_keyid, args.signing_password, args.signing_secret_keyring,
              args.compile_only,
              args.install_local, args.install_dir,
              build_dir, args.arch, args.output,
              args.debug_build, args.release_build, args.unstripped, args.extra_gn_args,
              args.extra_gn_flags, args.extra_ninja_flags, str(args.jobs))

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
