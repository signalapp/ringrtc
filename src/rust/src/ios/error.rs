//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! iOS Error Codes

use thiserror::Error;

/// iOS specific error codes.
#[allow(non_camel_case_types)]
#[derive(Error, Debug)]
pub enum IosError {
    // iOS error codes
    #[error("Creating RTCPeerConnection in App failed")]
    CreateAppPeerConnection,
    #[error("Creating MediaStream in App failed")]
    CreateAppMediaStream,
    #[error("Creating IosMediaStream failed")]
    CreateIosMediaStream,

    // iOS Misc error codes
    #[error("Extracting native PeerConnection failed")]
    ExtractNativePeerConnection,
}
