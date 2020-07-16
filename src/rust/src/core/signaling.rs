//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

/// The messages we send over the signaling channel to establish a call.
use std::fmt;
use std::time::Duration;

use crate::common::{CallMediaType, DeviceId, FeatureLevel};

/// An enum representing the different types of signaling messages that
/// can be sent and received.
#[derive(Clone)]
pub enum Message {
    Offer(Offer),
    Answer(Answer),
    Ice(Ice),
    Hangup(Hangup),
    LegacyHangup(Hangup),
    Busy,
}

impl Message {
    pub fn typ(&self) -> MessageType {
        match self {
            Self::Offer(_) => MessageType::Offer,
            Self::Answer(_) => MessageType::Answer,
            Self::Ice(_) => MessageType::Ice,
            Self::Hangup(_) => MessageType::Hangup,
            Self::LegacyHangup(_) => MessageType::Hangup,
            Self::Busy => MessageType::Busy,
        }
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display = match self {
            Self::Offer(offer) => format!("Offer({:?}, ...)", offer.call_media_type),
            Self::Answer(_) => "Answer(...)".to_string(),
            Self::Ice(_) => "Ice(...)".to_string(),
            Self::Hangup(hangup) => format!("Hangup({:?})", hangup),
            Self::LegacyHangup(hangup) => format!("LegacyHangup({:?})", hangup),
            Self::Busy => "Busy".to_string(),
        };
        write!(f, "({})", display)
    }
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

// It's conveneient to be able to now the type of a message without having
// an entire message, so we have the related MessageType enum
#[repr(i32)]
#[derive(Debug, PartialEq)]
pub enum MessageType {
    Offer,
    Answer,
    Ice,
    Hangup,
    Busy,
}

/// The caller sends this to several callees to initiate the call.
#[derive(Clone)]
pub struct Offer {
    pub call_media_type: CallMediaType,
    pub sdp:             String,
}

impl Offer {
    pub fn from_sdp(call_media_type: CallMediaType, sdp: String) -> Self {
        Self {
            call_media_type,
            sdp,
        }
    }
}

/// The callee sends this in response to an answer to setup
/// the call.
#[derive(Clone)]
pub struct Answer {
    pub sdp: String,
}

impl Answer {
    pub fn from_sdp(sdp: String) -> Self {
        Self { sdp }
    }
}

/// Each side can send these at any time after the offer and answer
/// are sent.
#[derive(Clone)]
pub struct Ice {
    pub candidates_added: Vec<IceCandidate>,
}

/// Each side sends these to setup an ICE connection
#[derive(Clone)]
pub struct IceCandidate {
    // We assume sdp_mline_index to be 0, in which case sdp_mid doesn't matter.
    pub sdp: String,
}

impl IceCandidate {
    pub fn from_sdp(sdp: String) -> Self {
        Self { sdp }
    }
}

impl fmt::Display for IceCandidate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let text = format!("sdp: {}", self.sdp);
        write!(f, "{}", crate::core::util::redact_string(&text))
    }
}

impl fmt::Debug for IceCandidate {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Hangup {
    Normal, // on this device
    AcceptedOnAnotherDevice(DeviceId),
    DeclinedOnAnotherDevice(DeviceId),
    BusyOnAnotherDevice(DeviceId),
    // If you want to express that you NeedPermission on your device,
    // You can either fill it in or with your own device_id.
    NeedPermission(Option<DeviceId>),
}

impl Hangup {
    pub fn to_type_and_device_id(&self) -> (HangupType, Option<DeviceId>) {
        match self {
            Self::Normal => (HangupType::Normal, None),
            Self::AcceptedOnAnotherDevice(other_device_id) => {
                (HangupType::AcceptedOnAnotherDevice, Some(*other_device_id))
            }
            Self::DeclinedOnAnotherDevice(other_device_id) => {
                (HangupType::DeclinedOnAnotherDevice, Some(*other_device_id))
            }
            Self::BusyOnAnotherDevice(other_device_id) => {
                (HangupType::BusyOnAnotherDevice, Some(*other_device_id))
            }
            Self::NeedPermission(other_device_id) => (HangupType::NeedPermission, *other_device_id),
        }
    }

    // For Normal, device_id is ignored
    // For NeedPermission, we can't express an unset DeviceId because the Android and iOS apps
    // give us DeviceIds of 0 rather than None when receiving, so we just assume it's set.
    // But since our receive logic doesn't care if it's 0 or None or anything else
    // for an outgoing call, that's fine.
    pub fn from_type_and_device_id(typ: HangupType, device_id: DeviceId) -> Self {
        match typ {
            HangupType::Normal => Self::Normal,
            HangupType::AcceptedOnAnotherDevice => Self::AcceptedOnAnotherDevice(device_id),
            HangupType::DeclinedOnAnotherDevice => Self::DeclinedOnAnotherDevice(device_id),
            HangupType::BusyOnAnotherDevice => Self::BusyOnAnotherDevice(device_id),
            HangupType::NeedPermission => Self::NeedPermission(Some(device_id)),
        }
    }
}

impl fmt::Display for Hangup {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (typ, device_id) = self.to_type_and_device_id();
        match device_id {
            Some(device_id) => write!(f, "{:?}/{}", typ, device_id),
            None => write!(f, "{:?}/None", typ),
        }
    }
}

// It's conveneient to be able to now the type of a hangup without having
// an entire message (such as with FFI), so we have the related HangupType.
// For convenience, we make this match the protobufs
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HangupType {
    // On this device
    Normal                  = 0,
    AcceptedOnAnotherDevice = 1,
    DeclinedOnAnotherDevice = 2,
    BusyOnAnotherDevice     = 3,
    // On either another device or this device
    NeedPermission          = 4,
}

impl HangupType {
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(HangupType::Normal),
            1 => Some(HangupType::AcceptedOnAnotherDevice),
            2 => Some(HangupType::DeclinedOnAnotherDevice),
            3 => Some(HangupType::BusyOnAnotherDevice),
            4 => Some(HangupType::NeedPermission),
            _ => None,
        }
    }
}

/// An Answer with extra info specific to sending
/// Answers are always sent to one device, never broadcast
pub struct SendAnswer {
    pub answer:             Answer,
    pub receiver_device_id: DeviceId,
}

/// An ICE message with extra info specific to sending
/// ICE messages can either target a particular device (callee only)
/// or brodacast (caller only).
pub struct SendIce {
    pub ice:                Ice,
    pub receiver_device_id: Option<DeviceId>,
}

/// A hangup message with extra info specific to sending
/// Hangup messages are always broadcast to all devices.
pub struct SendHangup {
    pub hangup:     Hangup,
    pub use_legacy: bool,
}

/// An Offer with extra info specific to receiving
pub struct ReceivedOffer {
    pub offer:                       Offer,
    /// The approximate age of the offer
    pub age:                         Duration,
    pub sender_device_id:            DeviceId,
    /// The feature level supported by the sender device
    pub sender_device_feature_level: FeatureLevel,
    pub receiver_device_id:          DeviceId,
    /// If true, the receiver (local) device is the primary device, otherwise a linked device
    pub receiver_device_is_primary:  bool,
}

/// An Answer with extra info specific to receiving
pub struct ReceivedAnswer {
    pub answer:                      Answer,
    pub sender_device_id:            DeviceId,
    /// The feature level supported by the sender device
    pub sender_device_feature_level: FeatureLevel,
}

/// An Ice message with extra info specific to receiving
pub struct ReceivedIce {
    pub ice:              Ice,
    pub sender_device_id: DeviceId,
}

/// A Hangup message with extra info specific to receiving
#[derive(Clone, Copy, Debug)]
pub struct ReceivedHangup {
    pub hangup:           Hangup,
    pub sender_device_id: DeviceId,
}

/// A Busy message with extra info specific to receiving
pub struct ReceivedBusy {
    pub sender_device_id: DeviceId,
}
