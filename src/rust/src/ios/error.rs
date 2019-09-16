//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! iOS Error Codes and Utilities.

/// iOS specific error codes.
#[allow(non_camel_case_types)]
#[derive(Fail, Debug)]
pub enum iOSError {

    // iOS error codes
    #[fail(display = "Couldn't allocate memory for logging object")]
    InitializeLogging,
    #[fail(display = "Creating RTCPeerConnection in App failed")]
    CreateAppPeerConnection,
    #[fail(display = "Creating RTCMediaStream in iOS failed")]
    CreateIOSMediaStream,

    // iOS Misc error codes
    #[fail(display = "Extracting native PeerConnectionInterface failed")]
    ExtractNativePeerConnectionInterface,

}
