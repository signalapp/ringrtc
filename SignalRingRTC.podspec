#
# Be sure to run `pod lib lint SignalRingRTC.podspec' to ensure this is a
# valid spec before submitting.
#
# Any lines starting with a # are optional, but their use is encouraged
# To learn more about a Podspec see http://guides.cocoapods.org/syntax/podspec.html
#

Pod::Spec.new do |s|
  s.name             = "SignalRingRTC"
  s.version          = "2.48.1"
  s.summary          = "A Swift & Objective-C library used by the Signal iOS app for WebRTC interactions."

  s.description      = <<-DESC
    A Swift & Objective-C library used by the Signal iOS app for WebRTC interactions."
  DESC

  s.license          = 'AGPLv3'
  s.homepage         = 'https://github.com/signalapp/ringrtc'
  s.source           = { git: 'https://github.com/signalapp/ringrtc.git', tag: "v#{s.version.to_s}" }
  s.author           = { 'Calling Team': 'callingteam@signal.org' }
  s.social_media_url = 'https://twitter.com/signalapp'

  # Newer versions of Xcode don't correctly handle command-line testing on older simulators.
  s.platform      = :ios, ENV.include?('RINGRTC_POD_TESTING') ? '14' : '13'
  s.requires_arc  = true
  s.swift_version = '5'

  s.source_files = 'src/ios/SignalRingRTC/SignalRingRTC/**/*.{h,m,swift}', 'out/release/libringrtc/*.h'
  s.public_header_files = 'src/ios/SignalRingRTC/SignalRingRTC/**/*.h'
  s.private_header_files = 'out/release/libringrtc/*.h'

  s.module_map = 'src/ios/SignalRingRTC/SignalRingRTC/SignalRingRTC.modulemap'

  s.preserve_paths = [
    'acknowledgments/acknowledgments.plist',
    'bin/set-up-for-cocoapods',
    'bin/fetch-artifact.py', # env.sh has extra dependencies, so we go directly to the Python script
    'config/version.sh',
    'config/version.properties',
    'prebuild-checksum',

    # controlled by bin/set-up-for-cocoapods
    'out/release/libringrtc',
    'out/release/acknowledgments-webrtc-ios.plist',
  ]

  s.pod_target_xcconfig = {
    # Make sure we link the static library, not a dynamic one.
    # Use an extra level of indirection because CocoaPods messes with OTHER_LDFLAGS too.
    'LIBRINGRTC_IF_NEEDED' => '$(PODS_TARGET_SRCROOT)/out/release/libringrtc/$(CARGO_BUILD_TARGET)/libringrtc.a',
    'OTHER_LDFLAGS' => '$(LIBRINGRTC_IF_NEEDED)',

    'RINGRTC_PREBUILD_DIR' => "$(USER_LIBRARY_DIR)/Caches/org.signal.ringrtc/prebuild-#{s.version.to_s}",

    'CARGO_BUILD_TARGET[sdk=iphonesimulator*][arch=arm64]' => 'aarch64-apple-ios-sim',
    'CARGO_BUILD_TARGET[sdk=iphonesimulator*][arch=*]' => 'x86_64-apple-ios',
    'CARGO_BUILD_TARGET[sdk=iphoneos*]' => 'aarch64-apple-ios',
  }

  s.script_phases = [
    { name: 'Check prebuild',
      execution_position: :before_compile,
      input_files: ['$(PODS_TARGET_SRCROOT)/prebuild-checksum', '$(RINGRTC_PREBUILD_DIR)/prebuild-checksum'],
      output_files: ['$(DERIVED_FILE_DIR)/prebuild-checksum'],
      script: %q(
        set -euo pipefail
        if [[ ! -f "${SCRIPT_INPUT_FILE_0}" ]]; then
          # Local development, ignore
          exit
        elif [[ ! -f "${SCRIPT_INPUT_FILE_1}" ]]; then
          echo 'error: Cannot find prebuild directory' "${RINGRTC_PREBUILD_DIR}" >&2
          echo 'note: If you are trying to use a local checkout of RingRTC, delete' "${SCRIPT_INPUT_FILE_0}" 'and try again' >&2
          echo 'note: Otherwise, please run Pods/SignalRingRTC/bin/set-up-for-cocoapods' >&2
          echo 'note: If you are in the Signal iOS repo, you can use `make dependencies`' >&2
          exit 1
        elif ! diff -q "${SCRIPT_INPUT_FILE_0}" "${SCRIPT_INPUT_FILE_1}"; then
          # Why not run it now? Because Xcode may have already processed some of the files.
          echo 'error: Please run Pods/SignalRingRTC/bin/set-up-for-cocoapods' >&2
          echo 'note: If you are in the Signal iOS repo, you can use `make dependencies`' >&2
          exit 1
        fi
        cp "${SCRIPT_INPUT_FILE_0}" "${SCRIPT_OUTPUT_FILE_0}"
      ),
    },
  ]

  s.test_spec 'Tests' do |test_spec|
    test_spec.source_files = 'src/ios/SignalRingRTC/SignalRingRTCTests/**/*.{h,m,swift}'
    test_spec.dependency 'Nimble'
  end

  s.subspec 'WebRTC' do |webrtc|
    webrtc.vendored_frameworks = 'out/release/WebRTC.xcframework'
  end

  s.prepare_command = 'bin/set-up-for-cocoapods'
end
