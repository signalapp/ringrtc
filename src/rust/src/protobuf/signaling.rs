/// A serialized one these goes in the "opaque" field of the CallingMessage::Offer in SignalService.proto
/// For future compatibility, we can add new slots (v2, v3, v4 ....)
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Offer {
    #[prost(message, optional, tag="2")]
    pub v3_or_v2: ::std::option::Option<ConnectionParametersV3OrV2>,
    #[prost(message, optional, tag="4")]
    pub v4: ::std::option::Option<ConnectionParametersV4>,
}
/// A serialized one these goes in the "opaque" field of the CallingMessage::Offer in SignalService.proto
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Answer {
    #[prost(message, optional, tag="2")]
    pub v3_or_v2: ::std::option::Option<ConnectionParametersV3OrV2>,
    #[prost(message, optional, tag="4")]
    pub v4: ::std::option::Option<ConnectionParametersV4>,
}
/// A serialized one these goes in the "opaque" field of the CallingMessage::Ice in SignalService.proto
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct IceCandidate {
    #[prost(message, optional, tag="2")]
    pub v3_or_v2: ::std::option::Option<IceCandidateV3OrV2>,
}
/// The V2 protocol uses SDP, DTLS, but not SCTP.
/// The V3 protocol uses SDP, but not DTLS, but not SCTP.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConnectionParametersV3OrV2 {
    #[prost(string, optional, tag="1")]
    pub sdp: ::std::option::Option<std::string::String>,
    /// V2 has this unset.
    /// V3 has this set
    #[prost(bytes, optional, tag="2")]
    pub public_key: ::std::option::Option<std::vec::Vec<u8>>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct IceCandidateV3OrV2 {
    #[prost(string, optional, tag="1")]
    pub sdp: ::std::option::Option<std::string::String>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct VideoCodec {
    #[prost(enumeration="VideoCodecType", optional, tag="1")]
    pub r#type: ::std::option::Option<i32>,
    /// Used for H264; Not used for VP8
    #[prost(uint32, optional, tag="2")]
    pub level: ::std::option::Option<u32>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConnectionParametersV4 {
    #[prost(bytes, optional, tag="1")]
    pub public_key: ::std::option::Option<std::vec::Vec<u8>>,
    #[prost(string, optional, tag="2")]
    pub ice_ufrag: ::std::option::Option<std::string::String>,
    #[prost(string, optional, tag="3")]
    pub ice_pwd: ::std::option::Option<std::string::String>,
    /// In other words, the video codecs the sender can receive.
    #[prost(message, repeated, tag="4")]
    pub receive_video_codecs: ::std::vec::Vec<VideoCodec>,
    /// Used at call establishment to convey the bitrate that should be used for sending.
    #[prost(uint64, optional, tag="5")]
    pub max_bitrate_bps: ::std::option::Option<u64>,
}
/// A generic calling message that is opaque to the application but interpreted by RingRTC.
/// A serialized one of these goes into the "Opaque" field in the CallingMessage variant
/// in Signal protocol messages.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct CallMessage {
    #[prost(message, optional, tag="1")]
    pub group_call_message: ::std::option::Option<super::group_call::DeviceToDevice>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
#[repr(i32)]
pub enum VideoCodecType {
    Vp8 = 8,
    H264ConstrainedBaseline = 40,
    H264ConstrainedHigh = 46,
}
