//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Simulation Error Codes and Utilities.

use thiserror::Error;

/// Simulation specific error codes.
#[derive(Error, Debug)]
pub enum SimError {
    #[error("Simulation: testing error code: {0}")]
    TestError(String),
    #[error("Simulation: Intentional: Send offer failed")]
    SendOfferError,
    #[error("Simulation: Intentional: Send answer failed")]
    SendAnswerError,
    #[error("Simulation: Intentional: Send ICE candidate failed")]
    SendIceCandidateError,
    #[error("Simulation: Intentional: Send hangup failed")]
    SendHangupError,
    #[error("Simulation: Intentional: Send busy failed")]
    SendBusyError,
    #[error("Simulation: Intentional: Send accepted failed")]
    SendAcceptedError,
    #[error("Simulation: Intentional: Add Media Stream failed")]
    MediaStreamError,
    #[error("Simulation: Intentional: Close Media failed")]
    CloseMediaError,
    #[error("Simulation: Intentional: Start Call failed")]
    StartCallError,
    #[error("Simulation: Intentional: Call Concluded failed")]
    CallConcludedError,
}
