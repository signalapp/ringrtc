//
// Copyright 2026 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

internal import RingRTCFfi

// Called from setUpRingRTCLogging, which apps call before using anything else in RingRTC, so dead
// stripping can't remove this reference. A mismatched libringrtc causes linking to fail.
//
// To check that a release build still references the version symbol, run from the repo root:
//
//     xcodebuild build -scheme SignalRingRTC -configuration Release \
//         -destination 'generic/platform=iOS' -derivedDataPath .build/xcode
//     find .build/xcode -name Logging.o -exec nm {} + | grep ringrtcVersion
//
// The output should include an undefined symbol, "U _ringrtcVersion_X_Y_Z". In release builds
// this function gets inlined into setUpRingRTCLogging, so the reference shows up in Logging.o, not
// FFIVersion.o.
func checkFFIVersion() {
    ringrtcVersion_2_75_0()
}
