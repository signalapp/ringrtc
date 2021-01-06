//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

export {
    AudioDevice,
    BandwidthMode,
    Call,
    CallId,
    CallEndedReason,
    CallLogLevel,
    CallState,
    CallingMessage,
    CallSettings,
    ConnectionState,
    DeviceId,
    GroupCall,
    GroupCallEndReason,
    GroupCallObserver,
    GroupMemberInfo,
    HangupMessage,
    HangupType,
    HttpMethod,
    JoinState,
    LocalDeviceState,
    OfferType,
    OpaqueMessage,
    PeekInfo,
    RemoteDeviceState,
    RingRTCType,
    UserId,
    VideoCapturer,
    VideoRenderer,
    VideoRequest
} from './ringrtc/Service';

export {
    CanvasVideoRenderer,
    GumVideoCapturer,
    VideoFrameSource,
} from './ringrtc/VideoSupport';

import { RingRTCType } from './ringrtc/Service';
export const RingRTC = new RingRTCType();
