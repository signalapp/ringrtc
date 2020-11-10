//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
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
    RemoteDeviceState,
    RenderedResolution,
    RingRTCType,
    UserId,
    VideoCapturer,
    VideoRenderer
} from './ringrtc/Service';

export {
    CanvasVideoRenderer,
    GumVideoCapturer,
    VideoFrameSource,
} from './ringrtc/VideoSupport';

import { RingRTCType } from './ringrtc/Service';
export const RingRTC = new RingRTCType();
