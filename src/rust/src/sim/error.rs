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
    #[fail(display = "Simulation: Intentional: Send busy failed")]
    SendBusyError,
    #[fail(display = "Simulation: Intentional: Send accepted failed")]
    SendAcceptedError,
    #[fail(display = "Simulation: Intentional: Add Media Stream failed")]
    MediaStreamError,
    #[fail(display = "Simulation: Intentional: Close Media failed")]
    CloseMediaError,
    #[fail(display = "Simulation: Intentional: Start Call failed")]
    StartCallError,
    #[fail(display = "Simulation: Intentional: Call Concluded failed")]
    CallConcludedError,
}
