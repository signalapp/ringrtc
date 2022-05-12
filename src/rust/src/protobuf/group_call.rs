#[derive(Clone, PartialEq, ::prost::Message)]
pub struct DeviceToDevice {
    #[prost(bytes="vec", optional, tag="1")]
    pub group_id: ::core::option::Option<::prost::alloc::vec::Vec<u8>>,
    #[prost(message, optional, tag="2")]
    pub media_key: ::core::option::Option<device_to_device::MediaKey>,
    #[prost(message, optional, tag="3")]
    pub heartbeat: ::core::option::Option<device_to_device::Heartbeat>,
    #[prost(message, optional, tag="4")]
    pub leaving: ::core::option::Option<device_to_device::Leaving>,
}
/// Nested message and enum types in `DeviceToDevice`.
pub mod device_to_device {
    /// Sent over signaling
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct MediaKey {
        #[prost(uint32, optional, tag="1")]
        pub ratchet_counter: ::core::option::Option<u32>,
        #[prost(bytes="vec", optional, tag="2")]
        pub secret: ::core::option::Option<::prost::alloc::vec::Vec<u8>>,
        #[prost(uint32, optional, tag="3")]
        pub demux_id: ::core::option::Option<u32>,
    }
    /// Sent over RTP data
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Heartbeat {
        #[prost(bool, optional, tag="1")]
        pub audio_muted: ::core::option::Option<bool>,
        #[prost(bool, optional, tag="2")]
        pub video_muted: ::core::option::Option<bool>,
        #[prost(bool, optional, tag="3")]
        pub presenting: ::core::option::Option<bool>,
        #[prost(bool, optional, tag="4")]
        pub sharing_screen: ::core::option::Option<bool>,
    }
    /// Sent over RTP data *and* signaling
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Leaving {
        /// When sent over signaling, you must indicate which device is leaving.
        #[prost(uint32, optional, tag="1")]
        pub demux_id: ::core::option::Option<u32>,
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct DeviceToSfu {
    #[prost(message, optional, tag="1")]
    pub video_request: ::core::option::Option<device_to_sfu::VideoRequestMessage>,
    #[prost(message, optional, tag="2")]
    pub leave: ::core::option::Option<device_to_sfu::LeaveMessage>,
}
/// Nested message and enum types in `DeviceToSfu`.
pub mod device_to_sfu {
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct VideoRequestMessage {
        #[prost(message, repeated, tag="1")]
        pub requests: ::prost::alloc::vec::Vec<video_request_message::VideoRequest>,
        #[prost(uint32, optional, tag="3")]
        pub max_kbps: ::core::option::Option<u32>,
    }
    /// Nested message and enum types in `VideoRequestMessage`.
    pub mod video_request_message {
        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct VideoRequest {
            #[prost(uint32, optional, tag="2")]
            pub height: ::core::option::Option<u32>,
            #[prost(fixed32, optional, tag="3")]
            pub demux_id: ::core::option::Option<u32>,
        }
    }
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct LeaveMessage {
    }
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SfuToDevice {
    #[prost(message, optional, tag="2")]
    pub video_request: ::core::option::Option<sfu_to_device::VideoRequest>,
    #[prost(message, optional, tag="4")]
    pub speaker: ::core::option::Option<sfu_to_device::Speaker>,
    #[prost(message, optional, tag="6")]
    pub device_joined_or_left: ::core::option::Option<sfu_to_device::DeviceJoinedOrLeft>,
    #[prost(message, optional, tag="7")]
    pub current_devices: ::core::option::Option<sfu_to_device::CurrentDevices>,
    #[prost(message, optional, tag="8")]
    pub stats: ::core::option::Option<sfu_to_device::Stats>,
}
/// Nested message and enum types in `SfuToDevice`.
pub mod sfu_to_device {
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct DeviceJoinedOrLeft {
    }
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Speaker {
        #[prost(fixed32, optional, tag="2")]
        pub demux_id: ::core::option::Option<u32>,
    }
    /// The resolution the SFU wants you to send to it to satisfy the requests
    /// of all of the other devices.
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct VideoRequest {
        #[prost(uint32, optional, tag="1")]
        pub height: ::core::option::Option<u32>,
    }
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct CurrentDevices {
        #[prost(uint32, repeated, packed="false", tag="1")]
        pub demux_ids_with_video: ::prost::alloc::vec::Vec<u32>,
        #[prost(fixed32, repeated, packed="false", tag="2")]
        pub all_demux_ids: ::prost::alloc::vec::Vec<u32>,
    }
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Stats {
        /// server => client rate given by congestion control
        #[prost(uint32, optional, tag="1")]
        pub target_send_rate_kbps: ::core::option::Option<u32>,
        /// server => client ideal rate
        #[prost(uint32, optional, tag="2")]
        pub ideal_send_rate_kbps: ::core::option::Option<u32>,
        /// server => client rate allocated (likely less than target_send_rate_kbps)
        #[prost(uint32, optional, tag="3")]
        pub allocated_send_rate_kbps: ::core::option::Option<u32>,
    }
}
