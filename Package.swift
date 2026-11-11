// swift-tools-version:6.0

//
// Copyright 2026 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import PackageDescription

var swiftSettings: [SwiftSetting] = []

// To capture video from input_video.mp4 in the app bundle instead of the camera, quit Xcode and
// reopen it with this set, for example: `RINGRTC_USE_FILE_BASED_CAMERA=1 xed .`
if Context.environment["RINGRTC_USE_FILE_BASED_CAMERA"] != nil {
    swiftSettings.append(.define("USE_FILE_BASED_CAMERA"))
}

var testDependencies: [Target.Dependency] = ["SignalRingRTC", "Nimble"]
var binaryTargets: [Target] = []
if let webRTCXCFrameworkPath = Context.environment["RINGRTC_WEBRTC_XCFRAMEWORK"] {
    binaryTargets.append(.binaryTarget(name: "WebRTC", path: webRTCXCFrameworkPath))
} else {
    binaryTargets.append(
        .binaryTarget(
            name: "WebRTC",
            url: "https://build-artifacts.signal.org/libraries/webrtc-8037e-ios-release.xcframework.zip",
            checksum: "84f36bd37860cdfc311db3f05f0f0378e37122bf0b221da45318b43f3e08e4cb"
        )
    )
}
if let ffiXCFrameworkPath = Context.environment["RINGRTC_FFI_XCFRAMEWORK"] {
    testDependencies.append("LibRingRTCFfi")
    binaryTargets.append(.binaryTarget(name: "LibRingRTCFfi", path: ffiXCFrameworkPath))
}

let package = Package(
    name: "SignalRingRTC",
    platforms: [
        .iOS(.v15)
    ],
    products: [
        .library(name: "SignalRingRTC", targets: ["SignalRingRTC"]),
        .library(name: "WebRTC", targets: ["WebRTC"]),
    ],
    dependencies: [
        .package(url: "https://github.com/Quick/Nimble", exact: "13.7.1")
    ],
    targets: [
        .systemLibrary(name: "RingRTCFfi", path: "src/ios/SignalRingRTC/RingRTCFfi"),
        .target(
            name: "SignalRingRTC",
            dependencies: ["RingRTCFfi", "WebRTC"],
            path: "src/ios/SignalRingRTC/SignalRingRTC",
            swiftSettings: swiftSettings
        ),
        .testTarget(
            name: "SignalRingRTCTests",
            dependencies: testDependencies,
            path: "src/ios/SignalRingRTC/SignalRingRTCTests"
        ),
    ] + binaryTargets,
    swiftLanguageModes: [.v5]
)
