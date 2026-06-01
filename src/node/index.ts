//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import { RingRTCType } from './ringrtc/Service';

export type {
  AudioDevice,
  CallId,
  CallSettings,
  DeviceId,
  GroupCallObserver,
  HttpResult,
  PeekDeviceInfo,
  PeekInfo,
  Reaction,
  UserId,
  VideoFrameSender,
  VideoFrameSource,
} from './ringrtc/Service';
export {
  AnswerMessage,
  DataMode,
  BusyMessage,
  Call,
  CallEndReason,
  CallLogLevel,
  CallMessageUrgency,
  CallRejectReason,
  CallState,
  CallingMessage,
  ConnectionState,
  GroupCall,
  GroupCallKind,
  GroupMemberInfo,
  HangupMessage,
  HangupType,
  HttpMethod,
  IceCandidateMessage,
  JoinState,
  LocalDeviceState,
  OfferMessage,
  OfferType,
  OpaqueMessage,
  PeekStatusCodes,
  RemoteDeviceState,
  RingCancelReason,
  RingRTCType,
  RingUpdate,
  SpeechEvent,
  VideoPixelFormatEnum,
  videoPixelFormatToEnum,
  VideoRequest,
  callIdFromEra,
  callIdFromRingId,
} from './ringrtc/Service';

export {
  CallLinkRootKey,
  CallLinkRestrictions,
  CallLinkState,
} from './ringrtc/CallLinks';

export { CallSummary, QualityStats } from './ringrtc/CallSummary';

export const RingRTC = new RingRTCType();
