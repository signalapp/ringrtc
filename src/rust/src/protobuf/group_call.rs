#[derive(Clone, PartialEq, ::prost::Message)]
pub struct DeviceToDevice {
    #[prost(bytes, optional, tag="1")]
    pub group_id: ::std::option::Option<std::vec::Vec<u8>>,
    #[prost(message, optional, tag="2")]
    pub media_key: ::std::option::Option<device_to_device::MediaKey>,
    #[prost(message, optional, tag="3")]
    pub heartbeat: ::std::option::Option<device_to_device::Heartbeat>,
    #[prost(message, optional, tag="4")]
    pub leaving: ::std::option::Option<device_to_device::Leaving>,
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
        #[prost(bool, optional, tag="3")]
        pub presenting: ::std::option::Option<bool>,
        #[prost(bool, optional, tag="4")]
        pub sharing_screen: ::std::option::Option<bool>,
    }
    /// Sent over RTP data *and* signaling
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Leaving {
        /// When sent over signaling, you must indicate which device is leaving.
        #[prost(uint32, optional, tag="1")]
        pub demux_id: ::std::option::Option<u32>,
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct DeviceToSfu {
    #[prost(message, optional, tag="1")]
    pub video_request: ::std::option::Option<device_to_sfu::VideoRequestMessage>,
}
pub mod device_to_sfu {
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct VideoRequestMessage {
        #[prost(message, repeated, tag="1")]
        pub requests: ::std::vec::Vec<video_request_message::VideoRequest>,
        /// tag 2 was previously "max_count", which is no longer used
        #[prost(uint32, optional, tag="3")]
        pub max_kbps: ::std::option::Option<u32>,
    }
    pub mod video_request_message {
        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct VideoRequest {
            /// Functionally the same as a DemuxId, but oddly different.
            #[prost(uint64, optional, tag="1")]
            pub short_device_id: ::std::option::Option<u64>,
            #[prost(uint32, optional, tag="2")]
            pub height: ::std::option::Option<u32>,
        }
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SfuToDevice {
    #[prost(message, optional, tag="2")]
    pub video_request: ::std::option::Option<sfu_to_device::VideoRequest>,
    #[prost(message, optional, tag="4")]
    pub speaker: ::std::option::Option<sfu_to_device::Speaker>,
    #[prost(message, optional, tag="6")]
    pub device_joined_or_left: ::std::option::Option<sfu_to_device::DeviceJoinedOrLeft>,
    #[prost(message, optional, tag="7")]
    pub forwarding_video: ::std::option::Option<sfu_to_device::ForwardingVideo>,
}
pub mod sfu_to_device {
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct DeviceJoinedOrLeft {
    }
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Speaker {
        /// Functionally the same as a DemuxId, but oddly different.
        #[prost(string, optional, tag="1")]
        pub long_device_id: ::std::option::Option<std::string::String>,
    }
    /// The resolution the SFU wants you to send to it to satisfy the requests
    /// of all of the other devices.
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct VideoRequest {
        #[prost(uint32, optional, tag="1")]
        pub height: ::std::option::Option<u32>,
    }
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct ForwardingVideo {
        #[prost(uint32, repeated, packed="false", tag="1")]
        pub demux_ids: ::std::vec::Vec<u32>,
    }
}
