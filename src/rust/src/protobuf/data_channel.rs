//
// Copyright (C) 2019 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Connected {
    #[prost(uint64, optional, tag="1")]
    pub id: ::std::option::Option<u64>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Hangup {
    #[prost(uint64, optional, tag="1")]
    pub id: ::std::option::Option<u64>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct VideoStreamingStatus {
    #[prost(uint64, optional, tag="1")]
    pub id: ::std::option::Option<u64>,
    #[prost(bool, optional, tag="2")]
    pub enabled: ::std::option::Option<bool>,
}
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Data {
    #[prost(message, optional, tag="1")]
    pub connected: ::std::option::Option<Connected>,
    #[prost(message, optional, tag="2")]
    pub hangup: ::std::option::Option<Hangup>,
    #[prost(message, optional, tag="3")]
    pub video_streaming_status: ::std::option::Option<VideoStreamingStatus>,
}
