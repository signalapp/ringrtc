/// A serialized one these goes in the "opaque" field of the CallingMessage::Offer in SignalService.proto
/// For future compatibility, we can add new slots (v2, v3, v4 ....)
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Offer {
    #[prost(message, optional, tag="2")]
    pub v3_or_v2: ::std::option::Option<ConnectionParametersV3OrV2>,
}
/// A serialized one these goes in the "opaque" field of the CallingMessage::Offer in SignalService.proto
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Answer {
    #[prost(message, optional, tag="2")]
    pub v3_or_v2: ::std::option::Option<ConnectionParametersV3OrV2>,
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
