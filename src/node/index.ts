//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import { RingRTCType } from './ringrtc/Service';

export {
  AnswerMessage,
  AudioDevice,
  DataMode,
  BusyMessage,
  Call,
  CallEndedReason,
  CallId,
  CallLogLevel,
  CallMessageUrgency,
  CallSettings,
  CallState,
  CallingMessage,
  ConnectionState,
  DeviceId,
  GroupCall,
  GroupCallEndReason,
  GroupCallKind,
  GroupCallObserver,
  GroupMemberInfo,
  HangupMessage,
  HangupType,
  HttpMethod,
  HttpResult,
  IceCandidateMessage,
  JoinState,
  LocalDeviceState,
  OfferMessage,
  OfferType,
  OpaqueMessage,
  PeekDeviceInfo,
  PeekInfo,
  PeekStatusCodes,
  Reaction,
  RemoteDeviceState,
  RingCancelReason,
  RingRTCType,
  RingUpdate,
  UserId,
  VideoCapturer,
  VideoRenderer,
  VideoRequest,
  callIdFromEra,
  callIdFromRingId,
} from './ringrtc/Service';

export {
  CanvasVideoRenderer,
  GumVideoCapturer,
  VideoFrameSource,
  MAX_VIDEO_CAPTURE_AREA,
  MAX_VIDEO_CAPTURE_BUFFER_SIZE,
  MAX_VIDEO_CAPTURE_HEIGHT,
  MAX_VIDEO_CAPTURE_WIDTH,
} from './ringrtc/VideoSupport';

export {
  CallLinkRootKey,
  CallLinkRestrictions,
  CallLinkState,
} from './ringrtc/CallLinks';

export const RingRTC = new RingRTCType();
