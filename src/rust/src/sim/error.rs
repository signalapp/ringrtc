//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

//! Simulation Error Codes and Utilities.

/// Simulation specific error codes.
#[derive(Fail, Debug)]
pub enum SimError {

    #[fail(display = "Simulation: testing error code: {}", _0)]
    TestError(String),
    #[fail(display = "Simulation: Intentional: Send offer failed")]
    SendOfferError,
    #[fail(display = "Simulation: Intentional: Send answer failed")]
    SendAnswerError,
    #[fail(display = "Simulation: Intentional: Send ICE candidate failed")]
    SendIceCandidateError,
    #[fail(display = "Simulation: Intentional: Send hangup failed")]
    SendHangupError,

}
