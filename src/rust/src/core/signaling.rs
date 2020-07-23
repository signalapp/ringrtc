//
// Copyright (C) 2019, 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use bytes::{Bytes, BytesMut};
use prost::Message as _;
/// The messages we send over the signaling channel to establish a call.
use std::fmt;
use std::time::Duration;

use crate::common::{CallMediaType, DeviceId, FeatureLevel, Result};
use crate::core::util::redact_string;
use crate::error::RingRtcError;
use crate::protobuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    // The V1 protocol uses SDP, DTLS, and SCTP.
    V1,
    // The V2 protocol does not use SCTP.  It uses RTP data channels.
    // It uses SDP, but embedded in a protobuf.
    V2,
}

impl Version {
    pub fn enable_dtls(self) -> bool {
        // We expect V3 will not use DTLS
        true
    }

    pub fn enable_rtp_data_channel(self) -> bool {
        match self {
            Self::V1 => false,
            // This disables SCTP
            Self::V2 => true,
        }
    }
}

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
    // While we are transitioning, we should send *both* of these
    // and receive *either* of these.  Eventually, we'll drop to
    // only sending an receving the opaque value.
    pub opaque:          Option<Vec<u8>>,
    pub sdp:             Option<String>,
}

impl Offer {
    pub fn from_opaque_or_sdp(
        call_media_type: CallMediaType,
        opaque: Option<Vec<u8>>,
        sdp: Option<String>,
    ) -> Self {
        Self {
            call_media_type,
            sdp,
            opaque,
        }
    }

    pub fn latest_version(&self) -> Version {
        if self.opaque.is_some() {
            // Assume it's V2.  Don't bother decoding it.
            Version::V2
        } else {
            Version::V1
        }
    }

    pub fn from_v2_and_v1_sdp(
        call_media_type: CallMediaType,
        v2_sdp: String,
        v1_sdp: String,
    ) -> Result<Self> {
        let mut offer_proto_v2 = protobuf::signaling::ConnectionParametersV2::default();
        offer_proto_v2.sdp = Some(v2_sdp);

        let mut offer_proto = protobuf::signaling::Offer::default();
        offer_proto.v2 = Some(offer_proto_v2);

        let mut opaque = BytesMut::with_capacity(offer_proto.encoded_len());
        offer_proto.encode(&mut opaque)?;

        Ok(Self::from_opaque_or_sdp(
            call_media_type,
            Some(opaque.to_vec()),
            Some(v1_sdp),
        ))
    }

    pub fn to_v2_sdp(&self) -> Result<String> {
        match self {
            // Prefer opaque over SDP
            Self {
                opaque: Some(opaque),
                ..
            } => match protobuf::signaling::Offer::decode(Bytes::from(opaque.clone()))? {
                protobuf::signaling::Offer {
                    v2: Some(protobuf::signaling::ConnectionParametersV2 { sdp: Some(v2_sdp) }),
                } => Ok(v2_sdp),
                _ => Err(RingRtcError::UnknownSignaledProtocolVersion.into()),
            },
            _ => Err(RingRtcError::UnknownSignaledProtocolVersion.into()),
        }
    }

    pub fn to_v1_sdp(&self) -> Result<String> {
        match self {
            Self {
                sdp: Some(v1_sdp), ..
            } => Ok(v1_sdp.clone()),
            _ => Err(RingRtcError::UnknownSignaledProtocolVersion.into()),
        }
    }

    // First return value means "is_v2"
    pub fn to_v2_or_v1_sdp(&self) -> Result<(bool, String)> {
        match self {
            // Prefer opaque over SDP
            Self {
                opaque: Some(opaque),
                ..
            } => match protobuf::signaling::Offer::decode(Bytes::from(opaque.clone()))? {
                protobuf::signaling::Offer {
                    v2: Some(protobuf::signaling::ConnectionParametersV2 { sdp: Some(v2_sdp) }),
                } => Ok((true, v2_sdp)),
                _ => Err(RingRtcError::UnknownSignaledProtocolVersion.into()),
            },
            Self {
                opaque: None,
                sdp: Some(v1_sdp),
                ..
            } => Ok((false, v1_sdp.clone())),
            Self {
                opaque: None,
                sdp: None,
                ..
            } => Err(RingRtcError::UnknownSignaledProtocolVersion.into()),
        }
    }

    pub fn to_info_string(&self) -> String {
        to_info_string(self.opaque.as_ref(), self.sdp.as_ref())
    }

    pub fn to_redacted_string(&self) -> String {
        redacted_string_from_opaque_or_sdp(self.opaque.as_ref(), self.sdp.as_ref())
    }
}

impl fmt::Display for Offer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_redacted_string())
    }
}

impl fmt::Debug for Offer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_redacted_string())
    }
}

/// The callee sends this in response to an answer to setup
/// the call.
#[derive(Clone)]
pub struct Answer {
    // While we are transitioning, we send and receive *either* one of these
    // but not both.  It might be cleaner to use an enum here, but eventually
    // we will do away with sdp and just have opaque and go back to not
    // being an enum.  And it's more consistent with ICE and Offer messages
    // that can have both the SDP and opaque values.
    pub opaque: Option<Vec<u8>>,
    pub sdp:    Option<String>,
}

impl Answer {
    pub fn from_opaque_or_sdp(opaque: Option<Vec<u8>>, sdp: Option<String>) -> Self {
        Self { sdp, opaque }
    }

    pub fn to_info_string(&self) -> String {
        to_info_string(self.opaque.as_ref(), self.sdp.as_ref())
    }

    pub fn to_redacted_string(&self) -> String {
        redacted_string_from_opaque_or_sdp(self.opaque.as_ref(), self.sdp.as_ref())
    }

    pub fn latest_version(&self) -> Version {
        if self.opaque.is_some() {
            // Assume it's V2.  Don't bother decoding it.
            Version::V2
        } else {
            Version::V1
        }
    }

    pub fn from_v2_sdp(v2_sdp: String) -> Result<Self> {
        let mut answer_proto_v2 = protobuf::signaling::ConnectionParametersV2::default();
        answer_proto_v2.sdp = Some(v2_sdp);

        let mut answer_proto = protobuf::signaling::Answer::default();
        answer_proto.v2 = Some(answer_proto_v2);

        let mut opaque = BytesMut::with_capacity(answer_proto.encoded_len());
        answer_proto.encode(&mut opaque)?;

        let v1_sdp = None;
        Ok(Self::from_opaque_or_sdp(Some(opaque.to_vec()), v1_sdp))
    }

    pub fn from_v1_sdp(v1_sdp: String) -> Self {
        Self::from_opaque_or_sdp(None, Some(v1_sdp))
    }

    // First return value means "is_v2"
    pub fn to_v2_or_v1_sdp(&self) -> Result<(bool, String)> {
        match self {
            // Prefer opaque over SDP
            Self {
                opaque: Some(opaque),
                ..
            } => match protobuf::signaling::Answer::decode(Bytes::from(opaque.clone()))? {
                protobuf::signaling::Answer {
                    v2: Some(protobuf::signaling::ConnectionParametersV2 { sdp: Some(v2_sdp) }),
                } => Ok((true, v2_sdp)),
                _ => Err(RingRtcError::UnknownSignaledProtocolVersion.into()),
            },
            Self {
                opaque: None,
                sdp: Some(v1_sdp),
            } => Ok((false, v1_sdp.clone())),
            Self {
                opaque: None,
                sdp: None,
            } => Err(RingRtcError::UnknownSignaledProtocolVersion.into()),
        }
    }
}

impl fmt::Display for Answer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_redacted_string())
    }
}

impl fmt::Debug for Answer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.to_redacted_string())
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
    // While we are transitioning, we should send *both* of these
    // and receive *either* of these.  Eventually, we'll drop to
    // only sending an receving the opaque value.
    pub opaque: Option<Vec<u8>>,
    pub sdp:    Option<String>,
}

impl IceCandidate {
    pub fn from_opaque_or_sdp(opaque: Option<Vec<u8>>, sdp: Option<String>) -> Self {
        Self { sdp, opaque }
    }

    pub fn to_info_string(&self) -> String {
        to_info_string(self.opaque.as_ref(), self.sdp.as_ref())
    }

    pub fn to_redacted_string(&self) -> String {
        redacted_string_from_opaque_or_sdp(self.opaque.as_ref(), self.sdp.as_ref())
    }

    // ICE candiates are the same for V1 and V2, so this works for V1 as well.
    pub fn from_v2_sdp(sdp: String) -> Result<Self> {
        let mut ice_candidate_proto_v2 = protobuf::signaling::IceCandidateV2::default();
        ice_candidate_proto_v2.sdp = Some(sdp.clone());

        let mut ice_candidate_proto = protobuf::signaling::IceCandidate::default();
        ice_candidate_proto.v2 = Some(ice_candidate_proto_v2);

        let mut opaque = BytesMut::with_capacity(ice_candidate_proto.encoded_len());
        ice_candidate_proto.encode(&mut opaque)?;

        Ok(Self::from_opaque_or_sdp(Some(opaque.to_vec()), Some(sdp)))
    }

    // ICE candiates are the same for V1 and V2, so this works for V1 as well.
    pub fn to_v2_sdp(&self) -> Result<String> {
        match self {
            // Prefer opaque over SDP
            Self {
                opaque: Some(opaque),
                ..
            } => match protobuf::signaling::IceCandidate::decode(Bytes::from(opaque.clone()))? {
                protobuf::signaling::IceCandidate {
                    v2: Some(protobuf::signaling::IceCandidateV2 { sdp: Some(sdp) }),
                } => Ok(sdp),
                _ => Err(RingRtcError::UnknownSignaledProtocolVersion.into()),
            },
            Self {
                opaque: None,
                sdp: Some(sdp),
            } => Ok(sdp.clone()),
            Self {
                opaque: None,
                sdp: None,
            } => Err(RingRtcError::UnknownSignaledProtocolVersion.into()),
        }
    }
}

fn to_info_string(opaque: Option<&Vec<u8>>, sdp: Option<&String>) -> String {
    match (opaque, sdp) {
        (Some(opaque), Some(sdp)) => {
            format!("opaque=true/{}; sdp=true/{}", opaque.len(), sdp.len())
        }
        (Some(opaque), None) => format!("opaque=true/{}; sdp=false", opaque.len()),
        (None, Some(sdp)) => format!("opaque=false; sdp=true/{}", sdp.len()),
        (None, None) => "opaque=false; sdp=false".to_string(),
    }
}

fn redacted_string_from_opaque_or_sdp(opaque: Option<&Vec<u8>>, sdp: Option<&String>) -> String {
    match (opaque, sdp) {
        (Some(_), Some(sdp)) => format!("opaque: ...; sdp: {}", redact_string(sdp)),
        (Some(_), None) => "opaque: ...".to_string(),
        (None, Some(sdp)) => format!("sdp: {}", redact_string(sdp)),
        (None, None) => "Neither opaque nor SDP!  This shouldn't happen".to_string(),
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
