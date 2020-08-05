//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

export { AudioDevice, Call, CallId, CallEndedReason, CallLogLevel, CallState, CallingMessage, CallSettings, DeviceId, RingRTCType, UserId, VideoCapturer, VideoRenderer } from './ringrtc/Service';
export { CanvasVideoRenderer, GumVideoCapturer } from './ringrtc/VideoSupport';

import { RingRTCType } from './ringrtc/Service';
export const RingRTC = new RingRTCType();
