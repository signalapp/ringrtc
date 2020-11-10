#[derive(Clone, PartialEq, ::prost::Message)]
pub struct DeviceToDevice {
    #[prost(bytes, optional, tag="1")]
    pub group_id: ::std::option::Option<std::vec::Vec<u8>>,
    #[prost(message, optional, tag="2")]
    pub media_key: ::std::option::Option<device_to_device::MediaKey>,
    #[prost(message, optional, tag="3")]
    pub heartbeat: ::std::option::Option<device_to_device::Heartbeat>,
}
pub mod device_to_device {
    /// Sent over signaling
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct MediaKey {
        #[prost(uint32, optional, tag="1")]
        pub ratchet_counter: ::std::option::Option<u32>,
        #[prost(bytes, optional, tag="2")]
        pub secret: ::std::option::Option<std::vec::Vec<u8>>,
        #[prost(uint32, optional, tag="3")]
        pub demux_id: ::std::option::Option<u32>,
    }
    /// Sent over RTP data
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Heartbeat {
        #[prost(bool, optional, tag="1")]
        pub audio_muted: ::std::option::Option<bool>,
        #[prost(bool, optional, tag="2")]
        pub video_muted: ::std::option::Option<bool>,
    }
}
