//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

/// The messages we send over the signaling channel to establish a call.
use std::{
    convert::TryInto,
    fmt,
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use bytes::{Bytes, BytesMut};
use prost::Message as _;

use crate::common::{CallMediaType, DeviceId, Result};
use crate::protobuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    // V1 used SDP, DTLS, and SCTP. Not supported.
    // V2 replaced SCTP with RTP data and embedded SDP in a protobuf. Not supported.
    // V3 replaced DTLS with a custom Diffie-Hellman exchange to derive SRTP keys.
    // V3 is used for ICE candidates but not supported for offers/answers.
    V3,
    // V4 is the same as V3 but replaces SDP with discrete protobuf fields.
    V4,
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let display = match self {
            Self::V3 => "V3".to_string(),
            Self::V4 => "V4".to_string(),
        };
        write!(f, "{}", display)
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
    Busy,
}

impl Message {
    pub fn typ(&self) -> MessageType {
        match self {
            Self::Offer(_) => MessageType::Offer,
            Self::Answer(_) => MessageType::Answer,
            Self::Ice(_) => MessageType::Ice,
            Self::Hangup(_) => MessageType::Hangup,
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

// It's convenient to be able to now the type of a message without having
// an entire message, so we have the related MessageType enum.
#[repr(i32)]
#[derive(Debug, PartialEq, Eq)]
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
    pub opaque: Vec<u8>,
    // We cache a deserialized opaque value to avoid deserializing it repeatedly.
    proto: protobuf::signaling::Offer,
}

impl Offer {
    pub fn new(call_media_type: CallMediaType, opaque: Vec<u8>) -> Result<Self> {
        let proto = Self::deserialize_opaque(&opaque)?;
        Ok(Self {
            call_media_type,
            opaque,
            proto,
        })
    }

    fn deserialize_opaque(opaque: &[u8]) -> Result<protobuf::signaling::Offer> {
        Ok(protobuf::signaling::Offer::decode(Bytes::from(
            opaque.to_owned(),
        ))?)
    }

    pub fn latest_version(&self) -> Version {
        Version::V4
    }

    pub fn from_v4(
        call_media_type: CallMediaType,
        v4: protobuf::signaling::ConnectionParametersV4,
    ) -> Result<Self> {
        let proto = protobuf::signaling::Offer { v4: Some(v4) };

        let mut opaque = BytesMut::with_capacity(proto.encoded_len());
        proto.encode(&mut opaque)?;

        Self::new(call_media_type, opaque.to_vec())
    }

    pub fn to_v4(&self) -> Option<protobuf::signaling::ConnectionParametersV4> {
        match self {
            Self {
                proto: protobuf::signaling::Offer { v4: Some(v4), .. },
                ..
            } => Some(v4.clone()),
            _ => None,
        }
    }

    pub fn to_info_string(&self) -> String {
        format!(
            "opaque.len={}\tproto.version={}\ttype={}",
            self.opaque.len(),
            self.latest_version(),
            self.call_media_type
        )
    }
}

/// The callee sends this in response to an answer to setup the call.
#[derive(Clone)]
pub struct Answer {
    pub opaque: Vec<u8>,
    // We cache a deserialized opaque value to avoid deserializing it repeatedly.
    proto: protobuf::signaling::Answer,
}

impl Answer {
    pub fn new(opaque: Vec<u8>) -> Result<Self> {
        let proto = Self::deserialize_opaque(&opaque)?;
        Ok(Self { opaque, proto })
    }

    fn deserialize_opaque(opaque: &[u8]) -> Result<protobuf::signaling::Answer> {
        Ok(protobuf::signaling::Answer::decode(Bytes::from(
            opaque.to_owned(),
        ))?)
    }

    pub fn latest_version(&self) -> Version {
        Version::V4
    }

    pub fn from_v4(v4: protobuf::signaling::ConnectionParametersV4) -> Result<Self> {
        let proto = protobuf::signaling::Answer { v4: Some(v4) };

        let mut opaque = BytesMut::with_capacity(proto.encoded_len());
        proto.encode(&mut opaque)?;

        Self::new(opaque.to_vec())
    }

    pub fn to_v4(&self) -> Option<protobuf::signaling::ConnectionParametersV4> {
        match self {
            // Prefer opaque over SDP
            Self {
                proto: protobuf::signaling::Answer { v4: Some(v4), .. },
                ..
            } => Some(v4.clone()),
            _ => None,
        }
    }

    pub fn to_info_string(&self) -> String {
        format!(
            "opaque.len={}\tproto.version={}",
            self.opaque.len(),
            self.latest_version()
        )
    }
}

/// Each side can send these at any time after the offer and answer are sent.
#[derive(Clone)]
pub struct Ice {
    pub candidates: Vec<IceCandidate>,
}

/// Each side sends these to setup an ICE connection
/// and throughout the call ("continual gathering").
/// This can represent either an ICE candidate being added
/// or one being removed.
#[derive(Clone)]
pub struct IceCandidate {
    pub opaque: Vec<u8>,
}

impl From<SocketAddr> for protobuf::signaling::SocketAddr {
    fn from(addr: SocketAddr) -> Self {
        Self {
            ip: Some(match addr.ip() {
                IpAddr::V4(v4) => v4.octets().to_vec(),
                IpAddr::V6(v6) => v6.octets().to_vec(),
            }),
            port: Some(addr.port() as u32),
        }
    }
}

impl protobuf::signaling::SocketAddr {
    fn to_std(&self) -> Option<SocketAddr> {
        let octets = &self.ip.as_ref()?[..];
        let ip = if octets.len() == 4 {
            let octets: [u8; 4] = octets.try_into().unwrap();
            IpAddr::V4(octets.into())
        } else if octets.len() == 16 {
            let octets: [u8; 16] = octets.try_into().unwrap();
            IpAddr::V6(octets.into())
        } else {
            return None;
        };

        let port = self.port?;
        if port > (u16::MAX as u32) {
            return None;
        }
        let port = port as u16;

        Some(SocketAddr::new(ip, port))
    }
}

impl IceCandidate {
    pub fn new(opaque: Vec<u8>) -> Self {
        Self { opaque }
    }

    // The plan is to switch ICE candidates to V4, but they currently still use SDP (V3).
    pub fn from_v3_sdp(sdp: String) -> Result<Self> {
        let ice_candidate_proto_v3 = protobuf::signaling::IceCandidateV3 { sdp: Some(sdp) };
        let ice_candidate_proto = protobuf::signaling::IceCandidate {
            added_v3: Some(ice_candidate_proto_v3),
            removed: None,
        };

        let mut opaque = Vec::with_capacity(ice_candidate_proto.encoded_len());
        ice_candidate_proto.encode(&mut opaque)?;

        Ok(Self::new(opaque))
    }

    pub fn from_removed_address(removed_address: SocketAddr) -> Result<Self> {
        let ice_candidate_proto = protobuf::signaling::IceCandidate {
            removed: Some(removed_address.into()),
            // Old clients blow up if they don't find an added candidate,
            // so we need to put something here.
            // It must pass WebRTC's ParseCandidate, VerifyCandidate,
            // JsepTransport::AddRemoteCandidates,
            // and P2PTransportChannel::AddRemoteCandidate.
            // ParseCandidate requires all of the following:
            // - the format (with an optional "a=" prefix):
            //   "candidate:$foundation $component $protocol $priority $ip $port typ %type
            // - component must be an int
            // - protocol be "udp", "tcp", "ssltcp", or "tls"
            // - priority must be an uint32
            // - port must be a uint16
            // - type must be "local", "stun", "prflx", or "relay"
            // VerifyCandidate requires all of the following:
            // - (a non-zero port) or (a non-zero IP)
            // - (TCP with port 0) or (port > 1024) or ... who cares ...
            // JsepTransport::AddRemoteCandidates requires component = 1.
            // P2PTransportChannel::AddRemoteCandidate requires
            // - An unset ufrag (or you might get a warning or worse)
            // - An IP instead of a hostname (or you might trigger a DNS query)
            // - A protocol (UDP/TCP) that doesn't pair with anything (or you might create new pairs)
            // - Either an unset generation (for no warnings) or a set generation (for warnings, but no memory of the candidate)
            // So it's not paired, the foundation, IP, port, and type don't matter except to pass parsing
            added_v3: Some(protobuf::signaling::IceCandidateV3 {
                sdp: Some("candidate:FAKE 1 tcp 0 127.0.0.1 0 typ host".to_owned()),
            }),
        };

        let mut opaque = Vec::with_capacity(ice_candidate_proto.encoded_len());
        ice_candidate_proto.encode(&mut opaque)?;

        Ok(Self::new(opaque))
    }

    // ICE candidates are the same for V2 and V3 and V4.
    pub fn v3_sdp(&self) -> Option<String> {
        match protobuf::signaling::IceCandidate::decode(Bytes::from(self.opaque.clone())).ok()? {
            protobuf::signaling::IceCandidate {
                added_v3: Some(protobuf::signaling::IceCandidateV3 { sdp: Some(v3_sdp) }),
                ..
            } => Some(v3_sdp),
            _ => None,
        }
    }

    pub fn removed_address(&self) -> Option<SocketAddr> {
        match protobuf::signaling::IceCandidate::decode(Bytes::from(self.opaque.clone())).ok()? {
            protobuf::signaling::IceCandidate {
                removed: Some(removed_address),
                ..
            } => removed_address.to_std(),
            _ => None,
        }
    }

    pub fn to_info_string(&self) -> String {
        format!("opaque.len={}", self.opaque.len())
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

// It's convenient to be able to now the type of a hangup without having
// an entire message (such as with FFI), so we have the related HangupType.
// For convenience, we make this match the protobufs
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HangupType {
    // On this device
    Normal = 0,
    AcceptedOnAnotherDevice = 1,
    DeclinedOnAnotherDevice = 2,
    BusyOnAnotherDevice = 3,
    // On either another device or this device
    NeedPermission = 4,
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
    pub answer: Answer,
    pub receiver_device_id: DeviceId,
}

/// An ICE message with extra info specific to sending
/// ICE messages can either target a particular device (callee only)
/// or broadcast (caller only).
#[derive(Clone)]
pub struct SendIce {
    pub ice: Ice,
    pub receiver_device_id: Option<DeviceId>,
}

/// A hangup message with extra info specific to sending
/// Hangup messages are always broadcast to all devices.
pub struct SendHangup {
    pub hangup: Hangup,
}

/// An Offer with extra info specific to receiving
pub struct ReceivedOffer {
    pub offer: Offer,
    /// The approximate age of the offer
    pub age: Duration,
    pub sender_device_id: DeviceId,
    pub receiver_device_id: DeviceId,
    /// If true, the receiver (local) device is the primary device, otherwise a linked device
    pub receiver_device_is_primary: bool,
    pub sender_identity_key: Vec<u8>,
    pub receiver_identity_key: Vec<u8>,
}

/// An Answer with extra info specific to receiving
pub struct ReceivedAnswer {
    pub answer: Answer,
    pub sender_device_id: DeviceId,
    pub sender_identity_key: Vec<u8>,
    pub receiver_identity_key: Vec<u8>,
}

/// An Ice message with extra info specific to receiving
pub struct ReceivedIce {
    pub ice: Ice,
    pub sender_device_id: DeviceId,
}

/// A Hangup message with extra info specific to receiving
#[derive(Clone, Copy, Debug)]
pub struct ReceivedHangup {
    pub hangup: Hangup,
    pub sender_device_id: DeviceId,
}

/// A Busy message with extra info specific to receiving
pub struct ReceivedBusy {
    pub sender_device_id: DeviceId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct SenderStatus {
    pub video_enabled: Option<bool>,
    pub sharing_screen: Option<bool>,
}
