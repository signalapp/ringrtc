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
    }
    /// Sent over RTP data channel *and* signaling
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Leaving {
        /// When sent over signaling, you must indicate which device is leaving.
        #[prost(uint32, optional, tag="1")]
        pub demux_id: ::std::option::Option<u32>,
    }
}
/// Called RtpDataChannelMessage in the SFU's RtpDataChannelMessages.proto
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct DeviceToSfu {
    /// Called resolutionRequest in the SFU's RtpDataChannelMessages.proto
    #[prost(message, optional, tag="1")]
    pub video_request: ::std::option::Option<device_to_sfu::VideoRequestMessage>,
    /// Called endpointMessage in the SFU's RtpDataChannelMessages.proto
    #[prost(message, optional, tag="7")]
    pub send_to_devices: ::std::option::Option<device_to_sfu::SendToDevices>,
}
pub mod device_to_sfu {
    /// Called ResolutionRequestMessage in the SFU's RtpDataChannelMessages.proto
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct VideoRequestMessage {
        #[prost(message, repeated, tag="1")]
        pub requests: ::std::vec::Vec<video_request_message::VideoRequest>,
        /// Don't send more than this many, even if they are in the list above
        /// or if they aren't in the list above.
        /// Called lastN in the SFU's RtpDataChannelMessages.proto
        #[prost(uint32, optional, tag="2")]
        pub max: ::std::option::Option<u32>,
    }
    pub mod video_request_message {
        /// Called Constraint in the SFU's RtpDataChannelMessages.proto
        #[derive(Clone, PartialEq, ::prost::Message)]
        pub struct VideoRequest {
            /// Functionally the same as a DemuxId, but oddly different.
            /// Called endpointSuffix in the SFU's RtpDataChannelMessages.proto
            #[prost(uint64, optional, tag="1")]
            pub short_device_id: ::std::option::Option<u64>,
            /// Called idealHeight in the SFU's RtpDataChannelMessages.proto
            /// This does not allocate bits eagerly.
            #[prost(uint32, optional, tag="2")]
            pub height: ::std::option::Option<u32>,
        }
    }
    /// Called EndpointToEndpointMessage in the SFU's RtpDataChannelMessages.proto
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct SendToDevices {
        /// Functionally the same as a DemuxId, but oddly different.
        /// If not set, it will broadcast
        #[prost(string, optional, tag="1")]
        pub long_device_id: ::std::option::Option<std::string::String>,
        #[prost(bytes, optional, tag="3")]
        pub payload: ::std::option::Option<std::vec::Vec<u8>>,
    }
}
/// Called RtpDataChannelMessage in the SFU's RtpDataChannelMessages.proto
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct SfuToDevice {
    /// Called senderVideoConstraint in the SFU's RtpDataChannelMessages.proto
    #[prost(message, optional, tag="2")]
    pub video_request: ::std::option::Option<sfu_to_device::VideoRequest>,
    /// Called endpointConnectionStatus in the SFU's RtpDataChannelMessages.proto
    #[prost(message, optional, tag="3")]
    pub device_connection_status: ::std::option::Option<sfu_to_device::DeviceConnectionStatus>,
    /// Called dominantSpeaker in the SFU's RtpDataChannelMessages.proto
    #[prost(message, optional, tag="4")]
    pub speaker: ::std::option::Option<sfu_to_device::Speaker>,
    /// Called forwardedEndpoints in the SFU's RtpDataChannelMessages.proto
    #[prost(message, optional, tag="5")]
    pub forwarding: ::std::option::Option<sfu_to_device::Forwarding>,
    /// Called endpointChanged in the SFU's RtpDataChannelMessages.proto
    #[prost(message, optional, tag="6")]
    pub device_joined_or_left: ::std::option::Option<sfu_to_device::DeviceJoinedOrLeft>,
    /// Called forwardedEndpoints in the SFU's RtpDataChannelMessages.proto
    #[prost(message, optional, tag="7")]
    pub received_from_device: ::std::option::Option<sfu_to_device::ReceivedFromDevice>,
}
pub mod sfu_to_device {
    /// Called EndpointChangedMessage in the SFU's RtpDataChannelMessages.proto
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct DeviceJoinedOrLeft {
        /// Functionally the same as a DemuxId, but oddly different.
        /// Called endpoint in the SFU's RtpDataChannelMessages.proto
        #[prost(string, optional, tag="1")]
        pub long_device_id: ::std::option::Option<std::string::String>,
        #[prost(bool, optional, tag="2")]
        pub joined: ::std::option::Option<bool>,
    }
    /// The current primary/active speaker as calculated by rather complex logic by the SFU.
    /// Called DominantSpeakerMessage in the SFU's RtpDataChannelMessages.proto
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Speaker {
        /// Functionally the same as a DemuxId, but oddly different.
        /// Called endpoint in the SFU's RtpDataChannelMessages.proto
        #[prost(string, optional, tag="1")]
        pub long_device_id: ::std::option::Option<std::string::String>,
    }
    /// The resolution the SFU wants you to send to it to satisfy the requests
    /// of all of the other devices.
    /// Called SenderVideoConstraintMessage in the SFU's RtpDataChannelMessages.proto
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct VideoRequest {
        /// Called idealHeight in the SFU's RtpDataChannelMessages.proto
        #[prost(uint32, optional, tag="1")]
        pub height: ::std::option::Option<u32>,
    }
    /// Called EndpointConnectionStatusMessage in the SFU's RtpDataChannelMessages.proto
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct DeviceConnectionStatus {
        /// Functionally the same as a DemuxId, but oddly different.
        #[prost(string, optional, tag="1")]
        pub long_device_id: ::std::option::Option<std::string::String>,
        #[prost(bool, optional, tag="2")]
        pub active: ::std::option::Option<bool>,
    }
    /// Called EndpointToEndpointMessage in the SFU's RtpDataChannelMessages.proto
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct ReceivedFromDevice {
        /// Functionally the same as a DemuxId, but oddly different.
        #[prost(string, optional, tag="1")]
        pub long_device_id: ::std::option::Option<std::string::String>,
        #[prost(bytes, optional, tag="3")]
        pub payload: ::std::option::Option<std::vec::Vec<u8>>,
    }
    /// Called ForwardedEndpointsMessage in the SFU's RtpDataChannelMessages.proto
    #[derive(Clone, PartialEq, ::prost::Message)]
    pub struct Forwarding {
        /// The remote devices from which video is being forwarded.
        /// Functionally the same as a DemuxId, but oddly different.
        /// Called suffixEndpointsBeingForwarded in the SFU's RtpDataChannelMessages.proto
        #[prost(uint64, repeated, packed="false", tag="1")]
        pub video_forwarded_short_device_ids: ::std::vec::Vec<u64>,
        /// The remote devices from which video is being forwarded, but from which
        /// video was not being forwarded in the last Forwarding message.
        /// Functionally the same as a DemuxId, but oddly different.
        /// Called suffixEndpointsEnteringLastN in the SFU's RtpDataChannelMessages.proto
        #[prost(uint64, repeated, packed="false", tag="2")]
        pub newly_forwarded_short_device_ids: ::std::vec::Vec<u64>,
        /// All the of devices.  The same as the "Devices" messages.
        /// Functionally the same as a DemuxId, but oddly different.
        /// Called suffixEndpointsInConference in the SFU's RtpDataChannelMessages.proto
        #[prost(uint64, repeated, packed="false", tag="3")]
        pub all_devices_short_device_ids: ::std::vec::Vec<u64>,
    }
}
