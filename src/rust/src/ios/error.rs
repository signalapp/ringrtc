//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! iOS Error Codes

/// iOS specific error codes.
#[allow(non_camel_case_types)]
#[derive(Fail, Debug)]
pub enum IOSError {
    // iOS error codes
    #[fail(display = "Couldn't allocate memory for logging object")]
    InitializeLogging,
    #[fail(display = "Creating RTCPeerConnection in App failed")]
    CreateAppPeerConnection,
    #[fail(display = "Creating MediaStream in App failed")]
    CreateAppMediaStream,
    #[fail(display = "Creating IOSMediaStream failed")]
    CreateIOSMediaStream,

    // iOS Misc error codes
    #[fail(display = "Extracting native PeerConnection failed")]
    ExtractNativePeerConnection,
}
