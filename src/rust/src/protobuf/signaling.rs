/// A serialized one these goes in the "opaque" field of the CallingMessage::Offer in SignalService.proto
/// For future compatibility, we can add new slots (v2, v3, v4 ....)
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Offer {
    #[prost(message, optional, tag="2")]
    pub v2: ::std::option::Option<ConnectionParametersV2>,
}
/// A serialized one these goes in the "opaque" field of the CallingMessage::Offer in SignalService.proto
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Answer {
    #[prost(message, optional, tag="2")]
    pub v2: ::std::option::Option<ConnectionParametersV2>,
}
/// A serialized one these goes in the "opaque" field of the CallingMessage::Ice in SignalService.proto
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct IceCandidate {
    #[prost(message, optional, tag="2")]
    pub v2: ::std::option::Option<IceCandidateV2>,
}
/// The V2 protocol uses SDP, DTLS, but not SCTP.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ConnectionParametersV2 {
    #[prost(string, optional, tag="1")]
    pub sdp: ::std::option::Option<std::string::String>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct IceCandidateV2 {
    #[prost(string, optional, tag="1")]
    pub sdp: ::std::option::Option<std::string::String>,
}
