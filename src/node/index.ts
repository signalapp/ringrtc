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
  CallEndReason,
  CallId,
  CallLogLevel,
  CallMessageUrgency,
  CallRejectReason,
  CallSettings,
  CallState,
  CallingMessage,
  ConnectionState,
  DeviceId,
  GroupCall,
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
  SpeechEvent,
  UserId,
  VideoFrameSender,
  VideoFrameSource,
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
  CallLinkEpoch,
} from './ringrtc/CallLinks';

export { CallSummary, QualityStats } from './ringrtc/CallSummary';

export const RingRTC = new RingRTCType();
