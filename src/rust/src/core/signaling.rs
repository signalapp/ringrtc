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
    // The V3 protocol does not use DTLS.  It uses a custom
    // Diffie-Helman exchange to derive SRTP keys.
    V3,
    // Same as V3 except without any SDP.
    V4,
}

impl Version {
    pub fn enable_dtls(self) -> bool {
        match self {
            Self::V1 => true,
            Self::V2 => true,
            // This disables DTLS
            Self::V3 => false,
            Self::V4 => false,
        }
    }

    pub fn enable_rtp_data_channel(self) -> bool {
        match self {
            Self::V1 => false,
            // This disables SCTP
            Self::V2 => true,
            Self::V3 => true,
            Self::V4 => true,
        }
    }
}

/// An enum representing the different types of signaling messages that
/// can be sent and received.
#[derive(Clone)]
#[allow(clippy::large_enum_variant)]
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
    // We cache a deserialized opaque value to avoid deserializing it repeatedly.
    // If opaque is set, this should be set as well, and vice-versa.
    // Once SDP is gone we can just remove all the Options here.
    proto:               Option<protobuf::signaling::Offer>,
}

impl Offer {
    pub fn from_opaque_or_sdp(
        call_media_type: CallMediaType,
        opaque: Option<Vec<u8>>,
        sdp: Option<String>,
    ) -> Result<Self> {
        let proto = Self::deserialize_opaque(opaque.as_ref())?;
        Ok(Self {
            call_media_type,
            sdp,
            opaque,
            proto,
        })
    }

    fn deserialize_opaque(opaque: Option<&Vec<u8>>) -> Result<Option<protobuf::signaling::Offer>> {
        match opaque {
            None => Ok(None),
            Some(opaque) => Ok(Some(protobuf::signaling::Offer::decode(Bytes::from(
                opaque.clone(),
            ))?)),
        }
    }

    pub fn latest_version(&self) -> Version {
        match self {
            Self {
                proto: Some(protobuf::signaling::Offer { v4: Some(_), .. }),
                ..
            } => Version::V4,
            Self {
                proto:
                    Some(protobuf::signaling::Offer {
                        v3_or_v2:
                            Some(protobuf::signaling::ConnectionParametersV3OrV2 {
                                public_key: Some(_),
                                ..
                            }),
                        ..
                    }),
                ..
            } => Version::V3,
            Self {
                proto:
                    Some(protobuf::signaling::Offer {
                        v3_or_v2:
                            Some(protobuf::signaling::ConnectionParametersV3OrV2 {
                                public_key: None,
                                ..
                            }),
                        ..
                    }),
                ..
            } => Version::V2,
            _ => Version::V1,
        }
    }

    // V4 == V3 w/o SDP; V3 == V2 + public key
    pub fn from_v4_and_v3_and_v2_and_v1(
        call_media_type: CallMediaType,
        public_key: Vec<u8>,
        v4: Option<protobuf::signaling::ConnectionParametersV4>,
        v3_or_v2_sdp: String,
        v1_sdp: String,
    ) -> Result<Self> {
        let mut offer_proto_v3_or_v2 = protobuf::signaling::ConnectionParametersV3OrV2::default();
        offer_proto_v3_or_v2.public_key = Some(public_key);
        offer_proto_v3_or_v2.sdp = Some(v3_or_v2_sdp);

        let mut offer_proto = protobuf::signaling::Offer::default();
        offer_proto.v3_or_v2 = Some(offer_proto_v3_or_v2);

        offer_proto.v4 = v4;

        let mut opaque = BytesMut::with_capacity(offer_proto.encoded_len());
        offer_proto.encode(&mut opaque)?;

        // Once SDP is gone, pass in the proto rather than deserializing it here.
        Self::from_opaque_or_sdp(call_media_type, Some(opaque.to_vec()), Some(v1_sdp))
    }

    // V4 == V3 + non-SDP
    pub fn to_v4(&self) -> Option<protobuf::signaling::ConnectionParametersV4> {
        match self {
            Self {
                proto: Some(protobuf::signaling::Offer { v4: Some(v4), .. }),
                ..
            } => Some(v4.clone()),
            _ => None,
        }
    }

    pub fn to_v3_or_v2_sdp(&self) -> Result<String> {
        match self {
            // Prefer opaque/proto over SDP
            Self {
                proto:
                    Some(protobuf::signaling::Offer {
                        v3_or_v2:
                            Some(protobuf::signaling::ConnectionParametersV3OrV2 {
                                sdp: Some(v3_or_v2_sdp),
                                ..
                            }),
                        ..
                    }),
                ..
            } => Ok(v3_or_v2_sdp.clone()),
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

    // First return value means "is_v3_or_v2"
    // V3 == V2 + public_key
    pub fn to_v3_or_v2_or_v1_sdp(&self) -> Result<(bool, String, Option<Vec<u8>>)> {
        match self {
            // Prefer opaque over SDP
            Self {
                proto:
                    Some(protobuf::signaling::Offer {
                        v3_or_v2:
                            Some(protobuf::signaling::ConnectionParametersV3OrV2 {
                                sdp: Some(v3_or_v2_sdp),
                                public_key,
                            }),
                        ..
                    }),
                ..
            } => Ok((true, v3_or_v2_sdp.clone(), public_key.clone())),
            Self {
                proto: None,
                sdp: Some(v1_sdp),
                ..
            } => Ok((false, v1_sdp.clone(), None)),
            _ => Err(RingRtcError::UnknownSignaledProtocolVersion.into()),
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
    // We cache a deserialized opaque value to avoid deserializing it repeatedly.
    // If opaque is set, this should be set as well, and vice-versa.
    // Once SDP is gone we can just remove all the Options here.
    proto:      Option<protobuf::signaling::Answer>,
}

impl Answer {
    pub fn from_opaque_or_sdp(opaque: Option<Vec<u8>>, sdp: Option<String>) -> Result<Self> {
        let proto = Self::deserialize_opaque(opaque.as_ref())?;
        Ok(Self { sdp, opaque, proto })
    }

    fn deserialize_opaque(opaque: Option<&Vec<u8>>) -> Result<Option<protobuf::signaling::Answer>> {
        match opaque {
            None => Ok(None),
            Some(opaque) => Ok(Some(protobuf::signaling::Answer::decode(Bytes::from(
                opaque.clone(),
            ))?)),
        }
    }

    pub fn to_info_string(&self) -> String {
        to_info_string(self.opaque.as_ref(), self.sdp.as_ref())
    }

    pub fn to_redacted_string(&self) -> String {
        redacted_string_from_opaque_or_sdp(self.opaque.as_ref(), self.sdp.as_ref())
    }

    pub fn latest_version(&self) -> Version {
        match self {
            Self {
                proto: Some(protobuf::signaling::Answer { v4: Some(_), .. }),
                ..
            } => Version::V4,
            Self {
                proto:
                    Some(protobuf::signaling::Answer {
                        v3_or_v2:
                            Some(protobuf::signaling::ConnectionParametersV3OrV2 {
                                public_key: Some(_),
                                ..
                            }),
                        ..
                    }),
                ..
            } => Version::V3,
            Self {
                proto:
                    Some(protobuf::signaling::Answer {
                        v3_or_v2:
                            Some(protobuf::signaling::ConnectionParametersV3OrV2 {
                                public_key: None,
                                ..
                            }),
                        ..
                    }),
                ..
            } => Version::V2,
            _ => Version::V1,
        }
    }

    // V4 == V3 + non-SDP; V3 == V2 + public key
    pub fn from_v4(v4: protobuf::signaling::ConnectionParametersV4) -> Result<Self> {
        let mut proto = protobuf::signaling::Answer::default();
        proto.v4 = Some(v4);

        let mut opaque = BytesMut::with_capacity(proto.encoded_len());
        proto.encode(&mut opaque)?;

        Self::from_opaque_or_sdp(Some(opaque.to_vec()), None)
    }

    // V3 == V2 + public key
    pub fn from_v3_and_v2_sdp(public_key: Vec<u8>, v3_and_v2_sdp: String) -> Result<Self> {
        let mut answer_proto_v3_or_v2 = protobuf::signaling::ConnectionParametersV3OrV2::default();
        answer_proto_v3_or_v2.public_key = Some(public_key);
        answer_proto_v3_or_v2.sdp = Some(v3_and_v2_sdp);

        let mut answer_proto = protobuf::signaling::Answer::default();
        answer_proto.v3_or_v2 = Some(answer_proto_v3_or_v2);

        let mut opaque = BytesMut::with_capacity(answer_proto.encoded_len());
        answer_proto.encode(&mut opaque)?;

        // Once SDP is gone, pass in the proto rather than deserializing it here.
        let v1_sdp = None;
        Self::from_opaque_or_sdp(Some(opaque.to_vec()), v1_sdp)
    }

    pub fn from_v1_sdp(v1_sdp: String) -> Result<Self> {
        Self::from_opaque_or_sdp(None, Some(v1_sdp))
    }

    // V4 == V3 + non-SDP; V3 == V2 + public key
    pub fn to_v4(&self) -> Option<protobuf::signaling::ConnectionParametersV4> {
        match self {
            // Prefer opaque over SDP
            Self {
                proto: Some(protobuf::signaling::Answer { v4: Some(v4), .. }),
                ..
            } => Some(v4.clone()),
            _ => None,
        }
    }

    // First return value means "is_v3_or_v2"
    // V3 == V2 + public_key
    pub fn to_v3_or_v2_or_v1_sdp(&self) -> Result<(bool, String, Option<Vec<u8>>)> {
        match self {
            // Prefer opaque over SDP
            Self {
                proto:
                    Some(protobuf::signaling::Answer {
                        v3_or_v2:
                            Some(protobuf::signaling::ConnectionParametersV3OrV2 {
                                sdp: Some(v3_or_v2_sdp),
                                public_key,
                            }),
                        ..
                    }),
                ..
            } => Ok((true, v3_or_v2_sdp.clone(), public_key.clone())),
            Self {
                proto: None,
                sdp: Some(v1_sdp),
                ..
            } => Ok((false, v1_sdp.clone(), None)),
            _ => Err(RingRtcError::UnknownSignaledProtocolVersion.into()),
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

    // ICE candidates are the same for V1 and V2 and V3.
    pub fn from_v3_and_v2_and_v1_sdp(sdp: String) -> Result<Self> {
        let mut ice_candidate_proto_v3_or_v2 = protobuf::signaling::IceCandidateV3OrV2::default();
        ice_candidate_proto_v3_or_v2.sdp = Some(sdp.clone());

        let mut ice_candidate_proto = protobuf::signaling::IceCandidate::default();
        ice_candidate_proto.v3_or_v2 = Some(ice_candidate_proto_v3_or_v2);

        let mut opaque = BytesMut::with_capacity(ice_candidate_proto.encoded_len());
        ice_candidate_proto.encode(&mut opaque)?;

        Ok(Self::from_opaque_or_sdp(Some(opaque.to_vec()), Some(sdp)))
    }

    // ICE candidates are the same for V1 and V2 and V3.
    pub fn to_v3_and_v2_and_v1_sdp(&self) -> Result<String> {
        match self {
            // Prefer opaque over SDP
            Self {
                opaque: Some(opaque),
                ..
            } => match protobuf::signaling::IceCandidate::decode(Bytes::from(opaque.clone()))? {
                protobuf::signaling::IceCandidate {
                    v3_or_v2:
                        Some(protobuf::signaling::IceCandidateV3OrV2 {
                            sdp: Some(v3_or_v2_sdp),
                        }),
                } => Ok(v3_or_v2_sdp),
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
    pub sender_identity_key:         Vec<u8>,
    pub receiver_identity_key:       Vec<u8>,
}

/// An Answer with extra info specific to receiving
pub struct ReceivedAnswer {
    pub answer:                      Answer,
    pub sender_device_id:            DeviceId,
    /// The feature level supported by the sender device
    pub sender_device_feature_level: FeatureLevel,
    pub sender_identity_key:         Vec<u8>,
    pub receiver_identity_key:       Vec<u8>,
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
