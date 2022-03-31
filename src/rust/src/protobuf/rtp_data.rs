#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Accepted {
    #[prost(uint64, optional, tag="1")]
    pub id: ::core::option::Option<u64>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Hangup {
    #[prost(uint64, optional, tag="1")]
    pub id: ::core::option::Option<u64>,
    #[prost(enumeration="hangup::Type", optional, tag="2")]
    pub r#type: ::core::option::Option<i32>,
    #[prost(uint32, optional, tag="3")]
    pub device_id: ::core::option::Option<u32>,
}
/// Nested message and enum types in `Hangup`.
pub mod hangup {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, ::prost::Enumeration)]
    #[repr(i32)]
    pub enum Type {
        HangupNormal = 0,
        HangupAccepted = 1,
        HangupDeclined = 2,
        HangupBusy = 3,
        HangupNeedPermission = 4,
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SenderStatus {
    #[prost(uint64, optional, tag="1")]
    pub id: ::core::option::Option<u64>,
    #[prost(bool, optional, tag="2")]
    pub video_enabled: ::core::option::Option<bool>,
    #[prost(bool, optional, tag="3")]
    pub sharing_screen: ::core::option::Option<bool>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct ReceiverStatus {
    #[prost(uint64, optional, tag="1")]
    pub id: ::core::option::Option<u64>,
    /// Used during the call to convey the bitrate that should be used for sending.
    #[prost(uint64, optional, tag="2")]
    pub max_bitrate_bps: ::core::option::Option<u64>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Message {
    #[prost(message, optional, tag="1")]
    pub accepted: ::core::option::Option<Accepted>,
    #[prost(message, optional, tag="2")]
    pub hangup: ::core::option::Option<Hangup>,
    #[prost(message, optional, tag="3")]
    pub sender_status: ::core::option::Option<SenderStatus>,
    /// If set, a larger value means a later message than a smaller value.
    /// Can be used to detect that messages are out of order.
    /// Useful when sending over transports that don't have ordering
    /// (or when sending over more than one transport)
    #[prost(uint64, optional, tag="4")]
    pub seqnum: ::core::option::Option<u64>,
    #[prost(message, optional, tag="5")]
    pub receiver_status: ::core::option::Option<ReceiverStatus>,
}
