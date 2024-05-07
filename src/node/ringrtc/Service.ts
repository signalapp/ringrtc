//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

/* eslint-disable max-classes-per-file */

import { GumVideoCaptureOptions, VideoPixelFormatEnum } from './VideoSupport';
import {
  CallLinkState,
  CallLinkRestrictions,
  CallLinkRootKey,
} from './CallLinks';
import Native from './Native';

export const callIdFromEra: (era: string) => CallId = Native.callIdFromEra;

export function callIdFromRingId(ringId: bigint): CallId {
  return {
    low: Number(BigInt.asIntN(32, ringId)),
    high: Number(BigInt.asIntN(32, ringId >> BigInt(32))),
    unsigned: true,
  };
}

class Config {
  field_trials: Record<string, string> | undefined;
}

class NativeCallManager {
  // Read by Rust
  private readonly observer: CallManagerCallbacks;

  constructor(observer: CallManagerCallbacks) {
    this.observer = observer;
    this.createCallEndpoint(new Config());
  }

  setConfig(config: Config) {
    this.createCallEndpoint(config);
  }

  private createCallEndpoint(config: Config) {
    const fieldTrials = Object.assign(
      {
        'RingRTC-AnyAddressPortsKillSwitch': 'Enabled',
        'WebRTC-Audio-OpusSetSignalVoiceWithDtx': 'Enabled',
        'RingRTC-PruneTurnPorts': 'Enabled',
        'WebRTC-Bwe-ProbingConfiguration':
          'skip_if_est_larger_than_fraction_of_max:0.99',
      },
      config.field_trials
    );

    /* eslint-disable prefer-template */
    const fieldTrialsString =
      Object.entries(fieldTrials)
        .map(([k, v]) => `${k}/${v}`)
        .join('/') + '/';
    /* eslint-enable prefer-template */
    Object.defineProperty(this, Native.callEndpointPropertyKey, {
      configurable: true, // allows it to be changed
      get() {
        const callEndpoint = Native.createCallEndpoint(this, fieldTrialsString);

        Object.defineProperty(this, Native.callEndpointPropertyKey, {
          configurable: true, // allows it to be changed
          value: callEndpoint,
        });
        // eslint-disable-next-line @typescript-eslint/no-unsafe-return
        return callEndpoint;
      },
    });
  }
}

// Mirror methods onto NativeCallManager.
// This is done through direct assignment rather than wrapper methods to avoid indirection.
(NativeCallManager.prototype as any).setSelfUuid = Native.cm_setSelfUuid;
(NativeCallManager.prototype as any).createOutgoingCall =
  Native.cm_createOutgoingCall;
(NativeCallManager.prototype as any).proceed = Native.cm_proceed;
(NativeCallManager.prototype as any).accept = Native.cm_accept;
(NativeCallManager.prototype as any).ignore = Native.cm_ignore;
(NativeCallManager.prototype as any).hangup = Native.cm_hangup;
(NativeCallManager.prototype as any).cancelGroupRing =
  Native.cm_cancelGroupRing;
(NativeCallManager.prototype as any).signalingMessageSent =
  Native.cm_signalingMessageSent;
(NativeCallManager.prototype as any).signalingMessageSendFailed =
  Native.cm_signalingMessageSendFailed;
(NativeCallManager.prototype as any).updateDataMode = Native.cm_updateDataMode;
(NativeCallManager.prototype as any).receivedOffer = Native.cm_receivedOffer;
(NativeCallManager.prototype as any).receivedAnswer = Native.cm_receivedAnswer;
(NativeCallManager.prototype as any).receivedIceCandidates =
  Native.cm_receivedIceCandidates;
(NativeCallManager.prototype as any).receivedHangup = Native.cm_receivedHangup;
(NativeCallManager.prototype as any).receivedBusy = Native.cm_receivedBusy;
(NativeCallManager.prototype as any).receivedCallMessage =
  Native.cm_receivedCallMessage;
(NativeCallManager.prototype as any).receivedHttpResponse =
  Native.cm_receivedHttpResponse;
(NativeCallManager.prototype as any).httpRequestFailed =
  Native.cm_httpRequestFailed;
(NativeCallManager.prototype as any).setOutgoingAudioEnabled =
  Native.cm_setOutgoingAudioEnabled;
(NativeCallManager.prototype as any).setOutgoingVideoEnabled =
  Native.cm_setOutgoingVideoEnabled;
(NativeCallManager.prototype as any).setOutgoingVideoIsScreenShare =
  Native.cm_setOutgoingVideoIsScreenShare;
(NativeCallManager.prototype as any).sendVideoFrame = Native.cm_sendVideoFrame;
(NativeCallManager.prototype as any).receiveVideoFrame =
  Native.cm_receiveVideoFrame;
(NativeCallManager.prototype as any).receiveGroupCallVideoFrame =
  Native.cm_receiveGroupCallVideoFrame;
(NativeCallManager.prototype as any).createGroupCallClient =
  Native.cm_createGroupCallClient;
(NativeCallManager.prototype as any).createCallLinkCallClient =
  Native.cm_createCallLinkCallClient;
(NativeCallManager.prototype as any).deleteGroupCallClient =
  Native.cm_deleteGroupCallClient;
(NativeCallManager.prototype as any).connect = Native.cm_connect;
(NativeCallManager.prototype as any).join = Native.cm_join;
(NativeCallManager.prototype as any).leave = Native.cm_leave;
(NativeCallManager.prototype as any).disconnect = Native.cm_disconnect;
(NativeCallManager.prototype as any).groupRing = Native.cm_groupRing;
(NativeCallManager.prototype as any).groupReact = Native.cm_groupReact;
(NativeCallManager.prototype as any).groupRaiseHand = Native.cm_groupRaiseHand;
(NativeCallManager.prototype as any).setOutgoingAudioMuted =
  Native.cm_setOutgoingAudioMuted;
(NativeCallManager.prototype as any).setOutgoingVideoMuted =
  Native.cm_setOutgoingVideoMuted;
(NativeCallManager.prototype as any).setOutgoingGroupCallVideoIsScreenShare =
  Native.cm_setOutgoingGroupCallVideoIsScreenShare;
(NativeCallManager.prototype as any).setPresenting = Native.cm_setPresenting;
(NativeCallManager.prototype as any).resendMediaKeys =
  Native.cm_resendMediaKeys;
(NativeCallManager.prototype as any).setDataMode = Native.cm_setDataMode;
(NativeCallManager.prototype as any).requestVideo = Native.cm_requestVideo;
(NativeCallManager.prototype as any).approveUser = Native.cm_approveUser;
(NativeCallManager.prototype as any).denyUser = Native.cm_denyUser;
(NativeCallManager.prototype as any).removeClient = Native.cm_removeClient;
(NativeCallManager.prototype as any).blockClient = Native.cm_blockClient;
(NativeCallManager.prototype as any).setGroupMembers =
  Native.cm_setGroupMembers;
(NativeCallManager.prototype as any).setMembershipProof =
  Native.cm_setMembershipProof;
(NativeCallManager.prototype as any).readCallLink = Native.cm_readCallLink;
(NativeCallManager.prototype as any).createCallLink = Native.cm_createCallLink;
(NativeCallManager.prototype as any).updateCallLink = Native.cm_updateCallLink;
(NativeCallManager.prototype as any).deleteCallLink = Native.cm_deleteCallLink;
(NativeCallManager.prototype as any).peekGroupCall = Native.cm_peekGroupCall;
(NativeCallManager.prototype as any).peekCallLinkCall =
  Native.cm_peekCallLinkCall;
(NativeCallManager.prototype as any).getAudioInputs = Native.cm_getAudioInputs;
(NativeCallManager.prototype as any).setAudioInput = Native.cm_setAudioInput;
(NativeCallManager.prototype as any).getAudioOutputs =
  Native.cm_getAudioOutputs;
(NativeCallManager.prototype as any).setAudioOutput = Native.cm_setAudioOutput;
(NativeCallManager.prototype as any).processEvents = Native.cm_processEvents;
(NativeCallManager.prototype as any).setRtcStatsInterval =
  Native.cm_setRtcStatsInterval;

type GroupId = Buffer;
type GroupCallUserId = Buffer;

export interface PeekDeviceInfo {
  demuxId: number;
  userId?: GroupCallUserId;
}

export interface Reaction {
  demuxId: number;
  value: string;
}

/** type returned by Rust */
export interface RawPeekInfo {
  devices: Array<PeekDeviceInfo>;
  creator?: GroupCallUserId;
  eraId?: string;
  maxDevices?: number;
  /** @deprecated Use {@link #deviceCountIncludingPendingDevices} and {@link #deviceCountExcludingPendingDevices} as appropriate */
  deviceCount: number;
  deviceCountIncludingPendingDevices: number;
  deviceCountExcludingPendingDevices: number;
  pendingUsers: Array<GroupCallUserId>;
  callLinkState?: RawCallLinkState;
}

/** type derived from RawPeekInfo */
export interface PeekInfo {
  devices: Array<PeekDeviceInfo>;
  creator?: GroupCallUserId;
  eraId?: string;
  maxDevices?: number;
  /** @deprecated Use {@link #deviceCountIncludingPendingDevices} and {@link #deviceCountExcludingPendingDevices} as appropriate */
  deviceCount: number;
  deviceCountIncludingPendingDevices: number;
  deviceCountExcludingPendingDevices: number;
  pendingUsers: Array<GroupCallUserId>;
  callLinkState?: CallLinkState;
}

export enum PeekStatusCodes {
  EXPIRED_CALL_LINK = 703,
  INVALID_CALL_LINK = 704,
}

// In sync with WebRTC's PeerConnection.AdapterType.
// Despite how it looks, this is not an option set.
// A network adapter type can only be one of the listed values.
// And there are a few oddities to note:
// - Cellular means we don't know if it's 2G, 3G, 4G, 5G, ...
//   If we know, it will be one of those corresponding enum values.
//   This means to know if something is cellular or not, you must
//   check all of those values.
// - Default means we don't know the adapter type (like Unknown)
//   but it's because we bound to the default IP address (0.0.0.0)
//   so it's probably the default adapter (wifi if available, for example)
//   This is unlikely to happen in practice.
enum NetworkAdapterType {
  Unknown = 0,
  Ethernet = 1 << 0,
  Wifi = 1 << 1,
  Cellular = 1 << 2,
  Vpn = 1 << 3,
  Loopback = 1 << 4,
  Default = 1 << 5,
  Cellular2G = 1 << 6,
  Cellular3G = 1 << 7,
  Cellular4G = 1 << 8,
  Cellular5G = 1 << 9,
}

// Information about the network route being used for sending audio/video/data
export class NetworkRoute {
  localAdapterType: NetworkAdapterType;

  constructor() {
    this.localAdapterType = NetworkAdapterType.Unknown;
  }
}

// Range of 0-32767 where 0 is silence.
export type RawAudioLevel = number;
// Range of 0-1 where 0 is silence.
export type NormalizedAudioLevel = number;

export class ReceivedAudioLevel {
  demuxId: number; // UInt32
  level: RawAudioLevel;

  constructor(demuxId: number, level: RawAudioLevel) {
    this.demuxId = demuxId;
    this.level = level;
  }
}

interface RawCallLinkState {
  name: string;
  rawRestrictions: number;
  revoked: boolean;
  expiration: Date;
}

function normalizeAudioLevel(raw: RawAudioLevel): NormalizedAudioLevel {
  return raw / 32767;
}

function rawCallLinkStateToCallLinkState(
  raw: RawCallLinkState | undefined
): CallLinkState | undefined {
  if (raw) {
    let restrictions: CallLinkRestrictions;
    switch (raw.rawRestrictions) {
      case 0:
        restrictions = CallLinkRestrictions.None;
        break;
      case 1:
        restrictions = CallLinkRestrictions.AdminApproval;
        break;
      default:
        restrictions = CallLinkRestrictions.Unknown;
        break;
    }
    return new CallLinkState(
      raw.name,
      restrictions,
      raw.revoked,
      raw.expiration
    );
  } else {
    return undefined;
  }
}

function rawPeekInfoToPeekInfo(raw: RawPeekInfo): PeekInfo {
  return {
    ...raw,
    callLinkState: rawCallLinkStateToCallLinkState(raw.callLinkState),
  };
}

class Requests<T> {
  private _resolveById: Map<number, (response: T) => void> = new Map();
  private _nextId = 1;

  add(): [number, Promise<T>] {
    const id = this._nextId++;
    const promise = new Promise<T>((resolve, _reject) => {
      this._resolveById.set(id, resolve);
    });
    return [id, promise];
  }

  resolve(id: number, response: T): boolean {
    const resolve = this._resolveById.get(id);
    if (!resolve) {
      return false;
    }
    resolve(response);
    this._resolveById.delete(id);
    return true;
  }
}

class CallInfo {
  isVideoCall: boolean;
  receivedAtCounter: number;

  constructor(isVideoCall: boolean, receivedAtCounter: number) {
    this.isVideoCall = isVideoCall;
    this.receivedAtCounter = receivedAtCounter;
  }
}

export type EmptyObj = Record<PropertyKey, never>;

export type HttpResult<T> =
  | { success: true; value: T }
  | { success: false; errorStatusCode: number };

export class RingRTCType {
  private readonly callManager: CallManager;
  private _call: Call | null;
  private _groupCallByClientId: Map<GroupCallClientId, GroupCall>;
  private _peekRequests: Requests<HttpResult<PeekInfo>>;
  private _callLinkRequests: Requests<HttpResult<CallLinkState>>;
  private _emptyRequests: Requests<HttpResult<undefined>>;

  // A map to hold call information not maintained in RingRTC.
  private _callInfoByCallId: Map<string, CallInfo>;

  private getCallInfoKey(callId: CallId): string {
    // CallId is u64 so use a string key instead.
    // Note that the representation is not padded, so we include a separator.
    // Otherwise {1, 123} and {11, 23} would have the same key.
    // (We could use Long.toString as well, but it doesn't matter what the key is.)
    return `${callId.high} ${callId.low}`;
  }

  // Set by UX
  handleOutgoingSignaling:
    | ((remoteUserId: UserId, message: CallingMessage) => Promise<boolean>)
    | null = null;

  handleIncomingCall: ((call: Call) => Promise<boolean>) | null = null;

  handleStartCall: ((call: Call) => Promise<boolean>) | null = null;

  handleAutoEndedIncomingCallRequest:
    | ((
        callId: CallId,
        remoteUserId: UserId,
        reason: CallEndedReason,
        ageSec: number,
        wasVideoCall: boolean,
        receivedAtCounter: number | undefined
      ) => void)
    | null = null;

  handleLogMessage:
    | ((
        level: CallLogLevel,
        fileName: string,
        line: number,
        message: string
      ) => void)
    | null = null;

  handleSendHttpRequest:
    | ((
        requestId: number,
        url: string,
        method: HttpMethod,
        headers: { [name: string]: string },
        body: Buffer | undefined
      ) => void)
    | null = null;

  handleSendCallMessage:
    | ((
        recipientUuid: Buffer,
        message: Buffer,
        urgency: CallMessageUrgency
      ) => void)
    | null = null;

  handleSendCallMessageToGroup:
    | ((
        groupId: Buffer,
        message: Buffer,
        urgency: CallMessageUrgency,
        overrideRecipients: Array<Buffer>
      ) => void)
    | null = null;

  handleGroupCallRingUpdate:
    | ((
        groupId: Buffer,
        ringId: bigint,
        sender: Buffer,
        update: RingUpdate
      ) => void)
    | null = null;

  handleRtcStatsReport: ((reportJson: string) => void) | null = null;

  constructor() {
    this.callManager = new NativeCallManager(this) as unknown as CallManager;
    this._call = null;
    this._groupCallByClientId = new Map();
    this._peekRequests = new Requests();
    this._callLinkRequests = new Requests();
    this._emptyRequests = new Requests();
    this._callInfoByCallId = new Map();
  }

  setConfig(config: Config): void {
    this.callManager.setConfig(config);
  }

  // Called by UX
  setSelfUuid(uuid: Buffer): void {
    this.callManager.setSelfUuid(uuid);
  }

  // Called by UX
  startOutgoingCall(
    remoteUserId: UserId,
    isVideoCall: boolean,
    localDeviceId: DeviceId
  ): Call {
    const callId = this.callManager.createOutgoingCall(
      remoteUserId,
      isVideoCall,
      localDeviceId
    );
    const isIncoming = false;
    const call = new Call(
      this.callManager,
      remoteUserId,
      callId,
      isIncoming,
      isVideoCall,
      CallState.Prering
    );
    this._call = call;
    // We won't actually send anything until the remote side accepts.
    call.outgoingAudioEnabled = true;
    call.outgoingVideoEnabled = isVideoCall;
    return call;
  }

  // Called by UX
  cancelGroupRing(
    groupId: GroupId,
    ringId: bigint,
    reason: RingCancelReason | null
  ): void {
    sillyDeadlockProtection(() => {
      this.callManager.cancelGroupRing(groupId, ringId.toString(), reason);
    });
  }

  // Called by Rust
  onStartOutgoingCall(remoteUserId: UserId, callId: CallId): void {
    const call = this._call;
    if (!call || call.remoteUserId !== remoteUserId) {
      return;
    }

    call.callId = callId;

    const handleStartCall = this.handleStartCall;
    if (!handleStartCall) {
      call.ignore();
      return;
    }

    handleStartCall(call)
      .then(result => {
        if (!result) {
          this.logWarn(
            'RingRTC.handleStartCall failed for outgoing call. Call ignored.'
          );
          call.ignore();
        }
      })
      .catch(e => {
        this.logError(`RingRTC.handleStartCall exception: ${e}`);
        call.ignore();
      });
  }

  // Called by Rust
  onStartIncomingCall(
    remoteUserId: UserId,
    callId: CallId,
    isVideoCall: boolean
  ): void {
    // Temporary: Force hangup in all glare scenarios until handled gracefully.
    // In case of a glare loser, an incoming call will be generated right
    // after the outgoing call is ended. In that case, ignore it once.
    if (
      this._call &&
      (this._call.endedReason === CallEndedReason.Glare ||
        this._call.endedReason === CallEndedReason.ReCall)
    ) {
      this._call.endedReason = undefined;
      // EVIL HACK: We are the "loser" of a glare collision and have ended the outgoing call
      // and are now receiving the incoming call from the remote side (the "winner").
      // However, the Desktop client has a bug where it re-orders the events so that
      // instead of seeing ("outgoing call ended", "incoming call"), it sees
      // ("incoming call", "call ended") and it gets messed up.
      // The solution?  Delay processing the incoming call.
      setTimeout(() => {
        this.onStartIncomingCall(remoteUserId, callId, isVideoCall);
      }, 500);
      return;
    }

    const isIncoming = true;
    const call = new Call(
      this.callManager,
      remoteUserId,
      callId,
      isIncoming,
      isVideoCall,
      CallState.Prering
    );
    const { handleIncomingCall, handleStartCall } = this;
    if (!handleIncomingCall || !handleStartCall) {
      call.ignore();
      return;
    }
    this._call = call;

    handleIncomingCall(call)
      .then(success => {
        if (!success) {
          this.logWarn(
            'RingRTC.handleIncomingCall failed for incoming call. Call ignored.'
          );
          call.ignore();
        } else {
          handleStartCall(call)
            .then(innerSuccess => {
              if (!innerSuccess) {
                this.logWarn(
                  'RingRTC.handleStartCall failed for incoming call. Call ignored.'
                );
                call.ignore();
              }
            })
            .catch(e => {
              this.logError(`RingRTC.handleStartCall exception: ${e}`);
              call.ignore();
            });
        }
      })
      .catch(e => {
        this.logError(`RingRTC.handleIncomingCall exception: ${e}`);
        call.ignore();
      });
  }

  proceed(callId: CallId, settings: CallSettings): void {
    sillyDeadlockProtection(() => {
      this.callManager.proceed(
        callId,
        settings.iceServers,
        settings.hideIp,
        settings.dataMode,
        settings.audioLevelsIntervalMillis || 0
      );
    });
  }

  // Called by Rust
  onCallState(remoteUserId: UserId, state: CallState): void {
    const call = this._call;
    if (!call || call.remoteUserId !== remoteUserId) {
      return;
    }
    call.state = state;
  }

  // Called by Rust
  onCallEnded(
    remoteUserId: UserId,
    callId: CallId,
    reason: CallEndedReason,
    ageSec: number
  ): void {
    const callInfo = this._callInfoByCallId.get(this.getCallInfoKey(callId));
    const { isVideoCall, receivedAtCounter } = callInfo || {
      isVideoCall: false,
      receivedAtCounter: undefined,
    };
    this._callInfoByCallId.delete(this.getCallInfoKey(callId));

    const call = this._call;
    if (call && reason == CallEndedReason.ReceivedOfferWithGlare) {
      // The current call is the outgoing call.
      // The ended call is the incoming call.
      // We're the "winner", so ignore the incoming call and keep going with the outgoing call.
      return;
    }

    if (
      call &&
      (reason === CallEndedReason.Glare || reason === CallEndedReason.ReCall)
    ) {
      // The current call is the outgoing call.
      // The ended call is the outgoing call.
      // We're the "loser", so end the outgoing/current call and wait for a new incoming call.
      // (proceeded down to the code below)
    }

    // If there is no call or the remoteUserId doesn't match that of the current
    // call, or if one of the "receive offer while already in a call or because
    // it expired" reasons are provided, don't end the current call, because
    // there isn't one for this Ended notification, just update the call history.
    // If the incoming call ends while in the prering state, also immediately
    // update the call history because it is just a replay of messages.
    if (
      !call ||
      call.remoteUserId !== remoteUserId ||
      reason === CallEndedReason.ReceivedOfferWhileActive ||
      reason === CallEndedReason.ReceivedOfferExpired ||
      (call.state === CallState.Prering && call.isIncoming)
    ) {
      if (this.handleAutoEndedIncomingCallRequest) {
        this.handleAutoEndedIncomingCallRequest(
          callId,
          remoteUserId,
          reason,
          ageSec,
          isVideoCall,
          receivedAtCounter
        );
      }

      if (call && call.state === CallState.Prering && call.isIncoming) {
        // Set the state to Ended without triggering a state update since we
        // already notified the client.
        call.endedReason = reason;
        call.setCallEnded();
      }

      return;
    }

    // Send the end reason first because setting the state triggers
    // call.handleStateChanged, which may look at call.endedReason.
    call.endedReason = reason;
    call.state = CallState.Ended;
  }

  onRemoteVideoEnabled(remoteUserId: UserId, enabled: boolean): void {
    const call = this._call;
    if (!call || call.remoteUserId !== remoteUserId) {
      return;
    }

    call.remoteVideoEnabled = enabled;
    if (call.handleRemoteVideoEnabled) {
      call.handleRemoteVideoEnabled();
    }
  }

  onRemoteSharingScreen(remoteUserId: UserId, enabled: boolean): void {
    const call = this._call;
    if (!call || call.remoteUserId !== remoteUserId) {
      return;
    }

    call.remoteSharingScreen = enabled;
    if (call.handleRemoteSharingScreen) {
      call.handleRemoteSharingScreen();
    }
  }

  onNetworkRouteChanged(
    remoteUserId: UserId,
    localNetworkAdapterType: NetworkAdapterType
  ): void {
    const call = this._call;
    if (!call || call.remoteUserId !== remoteUserId) {
      return;
    }

    call.networkRoute.localAdapterType = localNetworkAdapterType;
    if (call.handleNetworkRouteChanged) {
      call.handleNetworkRouteChanged();
    }
  }

  onAudioLevels(
    remoteUserId: UserId,
    capturedLevel: RawAudioLevel,
    receivedLevel: RawAudioLevel
  ): void {
    const call = this._call;
    if (!call || call.remoteUserId !== remoteUserId) {
      return;
    }

    call.outgoingAudioLevel = normalizeAudioLevel(capturedLevel);
    call.remoteAudioLevel = normalizeAudioLevel(receivedLevel);
    if (call.handleAudioLevels) {
      call.handleAudioLevels();
    }
  }

  onLowBandwidthForVideo(remoteUserId: UserId, recovered: boolean): void {
    const call = this._call;
    if (!call || call.remoteUserId !== remoteUserId) {
      return;
    }

    if (call.handleLowBandwidthForVideo) {
      call.handleLowBandwidthForVideo(recovered);
    }
  }

  renderVideoFrame(width: number, height: number, buffer: Buffer): void {
    const call = this._call;
    if (!call) {
      return;
    }

    if (this._call?.renderVideoFrame) {
      this._call?.renderVideoFrame(width, height, buffer);
    }
  }

  // Called by Rust
  onSendOffer(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    offerType: OfferType,
    opaque: Buffer
  ): void {
    const message = new CallingMessage();
    message.offer = new OfferMessage(callId, offerType, opaque);
    this.sendSignaling(
      remoteUserId,
      remoteDeviceId,
      callId,
      broadcast,
      message
    );
  }

  // Called by Rust
  onSendAnswer(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    opaque: Buffer
  ): void {
    const message = new CallingMessage();
    message.answer = new AnswerMessage(callId, opaque);
    this.sendSignaling(
      remoteUserId,
      remoteDeviceId,
      callId,
      broadcast,
      message
    );
  }

  // Called by Rust
  onSendIceCandidates(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    candidates: Array<Buffer>
  ): void {
    const message = new CallingMessage();
    message.iceCandidates = [];
    for (const candidate of candidates) {
      const copy = new IceCandidateMessage(callId, candidate);
      message.iceCandidates.push(copy);
    }
    this.sendSignaling(
      remoteUserId,
      remoteDeviceId,
      callId,
      broadcast,
      message
    );
  }

  // Called by Rust
  onSendHangup(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    hangupType: HangupType,
    deviceId: DeviceId | null
  ): void {
    const message = new CallingMessage();
    message.hangup = new HangupMessage(callId, hangupType, deviceId || 0);
    this.sendSignaling(
      remoteUserId,
      remoteDeviceId,
      callId,
      broadcast,
      message
    );
  }

  // Called by Rust
  onSendBusy(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean
  ): void {
    const message = new CallingMessage();
    message.busy = new BusyMessage(callId);
    this.sendSignaling(
      remoteUserId,
      remoteDeviceId,
      callId,
      broadcast,
      message
    );
  }

  private sendSignaling(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    message: CallingMessage
  ): void {
    if (!broadcast) {
      message.destinationDeviceId = remoteDeviceId;
    }

    (async () => {
      if (this.handleOutgoingSignaling) {
        const signalingResult = await this.handleOutgoingSignaling(
          remoteUserId,
          message
        );
        if (signalingResult) {
          this.callManager.signalingMessageSent(callId);
        } else {
          this.callManager.signalingMessageSendFailed(callId);
        }
      } else {
        this.callManager.signalingMessageSendFailed(callId);
      }
    })().catch(e => this.logError(e.toString()));
  }

  // Call Links

  /**
   * Asynchronous request to get information about a call link.
   *
   * @param sfuUrl - the URL to use when accessing the SFU
   * @param authCredentialPresentation - a serialized CallLinkAuthCredentialPresentation
   * @param linkRootKey - the root key for the call link
   *
   * Expected failure codes include:
   * - 404: the room does not exist (or expired so long ago that it has been removed from the server)
   */
  readCallLink(
    sfuUrl: string,
    authCredentialPresentation: Buffer,
    linkRootKey: CallLinkRootKey
  ): Promise<HttpResult<CallLinkState>> {
    const [requestId, promise] = this._callLinkRequests.add();
    // Response comes back via handleCallLinkResponse
    sillyDeadlockProtection(() => {
      this.callManager.readCallLink(
        requestId,
        sfuUrl,
        authCredentialPresentation,
        linkRootKey.bytes
      );
    });
    return promise;
  }

  /**
   * Asynchronous request to create a new call link.
   *
   * This request is idempotent; if it fails due to a network issue, it is safe to retry.
   *
   * @example
   * const linkKey = CallLinkRootKey.generate();
   * const adminPasskey = CallLinkRootKey.generateAdminPasskey();
   * const roomId = linkKey.deriveRoomId();
   * const credential = requestCreateCredentialFromChatServer(roomId); // using libsignal
   * const secretParams = CallLinkSecretParams.deriveFromRootKey(linkKey.bytes);
   * const credentialPresentation = credential.present(roomId, secretParams).serialize();
   * const serializedPublicParams = secretParams.getPublicParams().serialize();
   * const result = await RingRTC.createCallLink(sfuUrl, credentialPresentation, linkKey, adminPasskey, serializedPublicParams);
   * if (result.success) {
   *   const state = result.value;
   *   // In actuality you may not want to do this until the user clicks Done.
   *   saveToDatabase(linkKey.bytes, adminPasskey, state);
   *   syncToOtherDevices(linkKey.bytes, adminPasskey);
   * } else {
   *   switch (result.errorStatusCode) {
   *   case 409:
   *     // The room already exists (and isn't yours), i.e. you've hit a 1-in-a-billion conflict.
   *     // Fall through to kicking the user out to try again later.
   *   default:
   *     // Unexpected error, kick the user out for now.
   *   }
   * }
   *
   * @param sfuUrl - the URL to use when accessing the SFU
   * @param createCredentialPresentation - a serialized CreateCallLinkCredentialPresentation
   * @param linkRootKey - the root key for the call link
   * @param adminPasskey - the arbitrary passkey to use for the new room
   * @param callLinkPublicParams - the serialized CallLinkPublicParams for the new room
   */
  createCallLink(
    sfuUrl: string,
    createCredentialPresentation: Buffer,
    linkRootKey: CallLinkRootKey,
    adminPasskey: Buffer,
    callLinkPublicParams: Buffer
  ): Promise<HttpResult<CallLinkState>> {
    const [requestId, promise] = this._callLinkRequests.add();
    // Response comes back via handleCallLinkResponse
    sillyDeadlockProtection(() => {
      this.callManager.createCallLink(
        requestId,
        sfuUrl,
        createCredentialPresentation,
        linkRootKey.bytes,
        adminPasskey,
        callLinkPublicParams
      );
    });
    return promise;
  }

  /**
   * Asynchronous request to update a call link's name.
   *
   * Possible failure codes include:
   * - 401: the room does not exist (and this is the wrong API to create a new room)
   * - 403: the admin passkey is incorrect
   *
   * This request is idempotent; if it fails due to a network issue, it is safe to retry.
   *
   * @param sfuUrl - the URL to use when accessing the SFU
   * @param authCredentialPresentation - a serialized CallLinkAuthCredentialPresentation
   * @param linkRootKey - the root key for the call link
   * @param adminPasskey - the passkey specified when the link was created
   * @param newName - the new name to use
   */
  updateCallLinkName(
    sfuUrl: string,
    authCredentialPresentation: Buffer,
    linkRootKey: CallLinkRootKey,
    adminPasskey: Buffer,
    newName: string
  ): Promise<HttpResult<CallLinkState>> {
    const [requestId, promise] = this._callLinkRequests.add();
    // Response comes back via handleCallLinkResponse
    sillyDeadlockProtection(() => {
      this.callManager.updateCallLink(
        requestId,
        sfuUrl,
        authCredentialPresentation,
        linkRootKey.bytes,
        adminPasskey,
        newName,
        undefined,
        undefined
      );
    });
    return promise;
  }

  /**
   * Asynchronous request to update a call link's restrictions.
   *
   * Possible failure codes include:
   * - 401: the room does not exist (and this is the wrong API to create a new room)
   * - 403: the admin passkey is incorrect
   * - 409: the room is currently in use, so restrictions cannot be changed at the moment
   *
   * This request is idempotent; if it fails due to a network issue, it is safe to retry.
   *
   * @param sfuUrl - the URL to use when accessing the SFU
   * @param authCredentialPresentation - a serialized CallLinkAuthCredentialPresentation
   * @param linkRootKey - the root key for the call link
   * @param adminPasskey - the passkey specified when the link was created
   * @param restrictions - the new restrictions to use
   */
  updateCallLinkRestrictions(
    sfuUrl: string,
    authCredentialPresentation: Buffer,
    linkRootKey: CallLinkRootKey,
    adminPasskey: Buffer,
    restrictions: Exclude<CallLinkRestrictions, CallLinkRestrictions.Unknown>
  ): Promise<HttpResult<CallLinkState>> {
    const [requestId, promise] = this._callLinkRequests.add();
    // Response comes back via handleCallLinkResponse
    sillyDeadlockProtection(() => {
      this.callManager.updateCallLink(
        requestId,
        sfuUrl,
        authCredentialPresentation,
        linkRootKey.bytes,
        adminPasskey,
        undefined,
        restrictions,
        undefined
      );
    });
    return promise;
  }

  /**
   * Asynchronous request to delete a call link.
   *
   * Possible failure codes include:
   * - 403: the admin passkey is incorrect
   * - 409: conflict - there is an ongoing call for this room.
   *
   * This request is idempotent; if it fails due to a network issue, it is safe to retry.
   *
   * @param sfuUrl - the URL to use when accessing the SFU
   * @param authCredentialPresentation - a serialized CallLinkAuthCredentialPresentation
   * @param linkRootKey - the root key for the call link
   * @param adminPasskey - the passkey specified when the link was created
   */
  deleteCallLink(
    sfuUrl: string,
    authCredentialPresentation: Buffer,
    linkRootKey: CallLinkRootKey,
    adminPasskey: Buffer
  ): Promise<HttpResult<undefined>> {
    const [requestId, promise] = this._emptyRequests.add();
    // Response comes back via handleCallLinkResponse
    sillyDeadlockProtection(() => {
      this.callManager.deleteCallLink(
        requestId,
        sfuUrl,
        authCredentialPresentation,
        linkRootKey.bytes,
        adminPasskey
      );
    });
    return promise;
  }

  // HTTP callbacks

  receivedHttpResponse(requestId: number, status: number, body: Buffer): void {
    sillyDeadlockProtection(() => {
      try {
        this.callManager.receivedHttpResponse(requestId, status, body);
      } catch {
        // We may not have an active connection any more.
        // In which case it doesn't matter
      }
    });
  }

  httpRequestFailed(requestId: number, debugInfo: string | undefined): void {
    sillyDeadlockProtection(() => {
      try {
        this.callManager.httpRequestFailed(requestId, debugInfo);
      } catch {
        // We may not have an active connection any more.
        // In which case it doesn't matter
      }
    });
  }

  // Group Calls

  // Called by UX
  getGroupCall(
    groupId: Buffer,
    sfuUrl: string,
    hkdfExtraInfo: Buffer,
    audioLevelsIntervalMillis: number | undefined,
    observer: GroupCallObserver
  ): GroupCall | undefined {
    const clientId = this.callManager.createGroupCallClient(
      groupId,
      sfuUrl,
      hkdfExtraInfo,
      audioLevelsIntervalMillis || 0
    );
    const groupCall = new GroupCall(
      GroupCallKind.SignalGroup,
      this.callManager,
      observer,
      clientId
    );

    this._groupCallByClientId.set(groupCall.clientId, groupCall);

    return groupCall;
  }

  // Called by UX
  getCallLinkCall(
    sfuUrl: string,
    authCredentialPresentation: Buffer,
    rootKey: CallLinkRootKey,
    adminPasskey: Buffer | undefined,
    hkdfExtraInfo: Buffer,
    audioLevelsIntervalMillis: number | undefined,
    observer: GroupCallObserver
  ): GroupCall | undefined {
    const clientId = this.callManager.createCallLinkCallClient(
      sfuUrl,
      authCredentialPresentation,
      rootKey.bytes,
      adminPasskey,
      hkdfExtraInfo,
      audioLevelsIntervalMillis || 0
    );
    const groupCall = new GroupCall(
      GroupCallKind.CallLink,
      this.callManager,
      observer,
      clientId
    );

    this._groupCallByClientId.set(groupCall.clientId, groupCall);

    return groupCall;
  }

  // Called by UX
  // Returns a list of user IDs
  peekGroupCall(
    sfuUrl: string,
    membershipProof: Buffer,
    groupMembers: Array<GroupMemberInfo>
  ): Promise<PeekInfo> {
    const [requestId, promise] = this._peekRequests.add();
    // Response comes back via handlePeekResponse
    sillyDeadlockProtection(() => {
      this.callManager.peekGroupCall(
        requestId,
        sfuUrl,
        membershipProof,
        groupMembers
      );
    });
    return promise.then(result => {
      if (result.success) {
        return result.value;
      } else {
        return {
          devices: [],
          deviceCount: 0,
          deviceCountIncludingPendingDevices: 0,
          deviceCountExcludingPendingDevices: 0,
          pendingUsers: [],
        };
      }
    });
  }

  // Called by UX
  peekCallLinkCall(
    sfuUrl: string,
    authCredentialPresentation: Buffer,
    rootKey: CallLinkRootKey
  ): Promise<HttpResult<PeekInfo>> {
    const [requestId, promise] = this._peekRequests.add();
    // Response comes back via handlePeekResponse
    sillyDeadlockProtection(() => {
      this.callManager.peekCallLinkCall(
        requestId,
        sfuUrl,
        authCredentialPresentation,
        rootKey.bytes
      );
    });
    return promise;
  }

  // Called by Rust
  requestMembershipProof(clientId: GroupCallClientId): void {
    sillyDeadlockProtection(() => {
      const groupCall = this._groupCallByClientId.get(clientId);
      if (!groupCall) {
        this.logError('requestMembershipProof(): GroupCall not found in map!');
        return;
      }

      groupCall.requestMembershipProof();
    });
  }

  // Called by Rust
  requestGroupMembers(clientId: GroupCallClientId): void {
    sillyDeadlockProtection(() => {
      const groupCall = this._groupCallByClientId.get(clientId);
      if (!groupCall) {
        this.logError('requestGroupMembers(): GroupCall not found in map!');
        return;
      }

      groupCall.requestGroupMembers();
    });
  }

  // Called by Rust
  handleConnectionStateChanged(
    clientId: GroupCallClientId,
    connectionState: ConnectionState
  ): void {
    sillyDeadlockProtection(() => {
      const groupCall = this._groupCallByClientId.get(clientId);
      if (!groupCall) {
        this.logError(
          'handleConnectionStateChanged(): GroupCall not found in map!'
        );
        return;
      }

      groupCall.handleConnectionStateChanged(connectionState);
    });
  }

  // Called by Rust
  handleJoinStateChanged(
    clientId: GroupCallClientId,
    joinState: JoinState,
    demuxId: number | undefined
  ): void {
    sillyDeadlockProtection(() => {
      const groupCall = this._groupCallByClientId.get(clientId);
      if (!groupCall) {
        this.logError('handleJoinStateChanged(): GroupCall not found in map!');
        return;
      }

      groupCall.handleJoinStateChanged(joinState, demuxId);
    });
  }

  // Called by Rust
  handleNetworkRouteChanged(
    clientId: GroupCallClientId,
    localNetworkAdapterType: NetworkAdapterType
  ): void {
    sillyDeadlockProtection(() => {
      const groupCall = this._groupCallByClientId.get(clientId);
      if (!groupCall) {
        this.logError(
          'handleNetworkRouteChanged(): GroupCall not found in map!'
        );
        return;
      }

      groupCall.handleNetworkRouteChanged(localNetworkAdapterType);
    });
  }

  // Called by Rust
  handleAudioLevels(
    clientId: GroupCallClientId,
    capturedLevel: RawAudioLevel,
    receivedLevels: Array<ReceivedAudioLevel>
  ): void {
    sillyDeadlockProtection(() => {
      const groupCall = this._groupCallByClientId.get(clientId);
      if (groupCall) {
        groupCall.handleAudioLevels(capturedLevel, receivedLevels);
      }
    });
  }

  // Called by Rust
  handleLowBandwidthForVideo(
    clientId: GroupCallClientId,
    recovered: boolean
  ): void {
    sillyDeadlockProtection(() => {
      const groupCall = this._groupCallByClientId.get(clientId);
      if (groupCall) {
        groupCall.handleLowBandwidthForVideo(recovered);
      }
    });
  }

  // Called by Rust
  handleReactions(
    clientId: GroupCallClientId,
    reactions: Array<Reaction>
  ): void {
    sillyDeadlockProtection(() => {
      const groupCall = this._groupCallByClientId.get(clientId);
      if (groupCall) {
        groupCall.handleReactions(reactions);
      }
    });
  }

  // Called by Rust
  handleRaisedHands(
    clientId: GroupCallClientId,
    raisedHands: Array<number>
  ): void {
    sillyDeadlockProtection(() => {
      const groupCall = this._groupCallByClientId.get(clientId);
      if (groupCall) {
        groupCall.handleRaisedHands(raisedHands);
      }
    });
  }

  // Called by Rust
  handleRemoteDevicesChanged(
    clientId: GroupCallClientId,
    remoteDeviceStates: Array<RemoteDeviceState>
  ): void {
    sillyDeadlockProtection(() => {
      const groupCall = this._groupCallByClientId.get(clientId);
      if (!groupCall) {
        this.logError(
          'handleRemoteDevicesChanged(): GroupCall not found in map!'
        );
        return;
      }

      groupCall.handleRemoteDevicesChanged(remoteDeviceStates);
    });
  }

  // Called by Rust
  handlePeekChanged(clientId: GroupCallClientId, rawInfo: RawPeekInfo): void {
    sillyDeadlockProtection(() => {
      const groupCall = this._groupCallByClientId.get(clientId);
      if (!groupCall) {
        this.logError('handlePeekChanged(): GroupCall not found in map!');
        return;
      }

      const info = rawPeekInfoToPeekInfo(rawInfo);
      groupCall.handlePeekChanged(info);
    });
  }

  // Called by Rust
  handlePeekResponse(
    requestId: number,
    statusCode: number,
    rawInfo: RawPeekInfo | undefined
  ): void {
    sillyDeadlockProtection(() => {
      let result: HttpResult<PeekInfo>;
      if (rawInfo) {
        const info = rawPeekInfoToPeekInfo(rawInfo);
        result = { success: true, value: info };
      } else {
        result = { success: false, errorStatusCode: statusCode };
      }
      if (!this._peekRequests.resolve(requestId, result)) {
        this.logWarn(`Invalid request ID for handlePeekResponse: ${requestId}`);
      }
    });
  }

  // Called by Rust
  handleCallLinkResponse(
    requestId: number,
    statusCode: number,
    state: RawCallLinkState | undefined
  ): void {
    sillyDeadlockProtection(() => {
      // Recreate the state so that we have the correct prototype, in case we add more methods to CallLinkState.
      let result: HttpResult<CallLinkState>;
      const s = rawCallLinkStateToCallLinkState(state);
      if (s) {
        result = {
          success: true,
          value: s,
        };
      } else {
        result = { success: false, errorStatusCode: statusCode };
      }
      if (!this._callLinkRequests.resolve(requestId, result)) {
        this.logWarn(
          `Invalid request ID for handleCallLinkResponse: ${requestId}`
        );
      }
    });
  }

  // Called by Rust
  handleEmptyResponse(
    requestId: number,
    statusCode: number,
    state: EmptyObj | undefined
  ): void {
    sillyDeadlockProtection(() => {
      let result: HttpResult<undefined>;
      if (state !== undefined) {
        result = { success: true, value: undefined };
      } else {
        result = { success: false, errorStatusCode: statusCode };
      }
      if (!this._emptyRequests.resolve(requestId, result)) {
        this.logWarn(
          `Invalid request ID for handleEmptyResponse: ${requestId}`
        );
      }
    });
  }

  // Called by Rust
  handleEnded(clientId: GroupCallClientId, reason: GroupCallEndReason): void {
    sillyDeadlockProtection(() => {
      const groupCall = this._groupCallByClientId.get(clientId);
      if (!groupCall) {
        this.logError('handleEnded(): GroupCall not found in map!');
        return;
      }

      this._groupCallByClientId.delete(clientId);

      groupCall.handleEnded(reason);
    });
  }

  // Called by Rust
  groupCallRingUpdate(
    groupId: GroupId,
    ringId: bigint,
    sender: GroupCallUserId,
    state: RingUpdate
  ): void {
    sillyDeadlockProtection(() => {
      if (this.handleGroupCallRingUpdate) {
        this.handleGroupCallRingUpdate(groupId, ringId, sender, state);
      } else {
        this.logError('RingRTC.handleGroupCallRingUpdate is not set!');
      }
    });
  }

  // Called by Rust
  handleRtcStatsReportComplete(reportJson: string): void {
    if (this.handleRtcStatsReport) {
      this.handleRtcStatsReport(reportJson);
    }
  }

  // Called by Rust
  onLogMessage(
    level: number,
    fileName: string,
    line: number,
    message: string
  ): void {
    if (this.handleLogMessage) {
      this.handleLogMessage(level, fileName, line, message);
    }
  }

  // Called from here
  logError(message: string): void {
    this.onLogMessage(CallLogLevel.Error, 'Service.ts', 0, message);
  }

  // Called from here
  logWarn(message: string): void {
    this.onLogMessage(CallLogLevel.Warn, 'Service.ts', 0, message);
  }

  // Called from here
  logInfo(message: string): void {
    this.onLogMessage(CallLogLevel.Info, 'Service.ts', 0, message);
  }

  // Called by MessageReceiver
  handleCallingMessage(
    remoteUserId: UserId,
    remoteUuid: Buffer | null,
    remoteDeviceId: DeviceId,
    localDeviceId: DeviceId,
    messageAgeSec: number,
    messageReceivedAtCounter: number,
    message: CallingMessage,
    senderIdentityKey: Buffer,
    receiverIdentityKey: Buffer
  ): void {
    if (
      message.destinationDeviceId &&
      message.destinationDeviceId !== localDeviceId
    ) {
      // Drop the message as it isn't for this device, handleIgnoredCall() is not needed.
      return;
    }

    if (message.offer?.callId) {
      const callId = message.offer.callId;
      const opaque = toBuffer(message.offer.opaque);

      // opaque is required. sdp is obsolete, but it might still come with opaque.
      if (!opaque) {
        // TODO: Remove once the proto is updated to only support opaque and require it.
        this.logError(
          'handleCallingMessage(): opaque not received for offer, remote should update'
        );
        return;
      }

      const offerType = message.offer.type || OfferType.AudioCall;

      // Save the call details for later when the call is ended.
      const callInfo = new CallInfo(
        offerType === OfferType.VideoCall,
        messageReceivedAtCounter
      );
      this._callInfoByCallId.set(this.getCallInfoKey(callId), callInfo);

      this.callManager.receivedOffer(
        remoteUserId,
        remoteDeviceId,
        localDeviceId,
        messageAgeSec,
        callId,
        offerType,
        opaque,
        senderIdentityKey,
        receiverIdentityKey
      );
    }
    if (message.answer?.callId) {
      const callId = message.answer.callId;
      const opaque = toBuffer(message.answer.opaque);

      // opaque is required. sdp is obsolete, but it might still come with opaque.
      if (!opaque) {
        // TODO: Remove once the proto is updated to only support opaque and require it.
        this.logError(
          'handleCallingMessage(): opaque not received for answer, remote should update'
        );
        return;
      }

      this.callManager.receivedAnswer(
        remoteUserId,
        remoteDeviceId,
        callId,
        opaque,
        senderIdentityKey,
        receiverIdentityKey
      );
    }
    if (message.iceCandidates && message.iceCandidates.length > 0) {
      // We assume they all have the same .callId
      const callId = message.iceCandidates[0].callId;
      // We have to copy them to do the .toArrayBuffer() thing.
      const candidates: Array<Buffer> = [];
      for (const candidate of message.iceCandidates) {
        const copy = toBuffer(candidate.opaque);
        if (copy) {
          candidates.push(copy);
        } else {
          // TODO: Remove once the proto is updated to only support opaque and require it.
          this.logError(
            'handleCallingMessage(): opaque not received for ice candidate, remote should update'
          );
          continue;
        }
      }

      if (candidates.length == 0) {
        this.logWarn(
          'handleCallingMessage(): No ice candidates in ice message, remote should update'
        );
        return;
      }

      if (!callId) {
        this.logWarn('handleCallingMessage(): No call ID in ice message');
        return;
      }

      this.callManager.receivedIceCandidates(
        remoteUserId,
        remoteDeviceId,
        callId,
        candidates
      );
    }
    if (message.hangup?.callId) {
      const callId = message.hangup.callId;
      const hangupType = message.hangup.type || HangupType.Normal;
      const hangupDeviceId = message.hangup.deviceId || null;
      this.callManager.receivedHangup(
        remoteUserId,
        remoteDeviceId,
        callId,
        hangupType,
        hangupDeviceId
      );
    }
    if (message.busy?.callId) {
      const callId = message.busy.callId;
      this.callManager.receivedBusy(remoteUserId, remoteDeviceId, callId);
    }
    if (message.opaque) {
      if (remoteUuid == null) {
        this.logError(
          'handleCallingMessage(): opaque message received without UUID!'
        );
        return;
      }
      const data = toBuffer(message.opaque.data);
      if (data == undefined) {
        this.logError(
          'handleCallingMessage(): opaque message received without data!'
        );
        return;
      }
      this.callManager.receivedCallMessage(
        remoteUuid,
        remoteDeviceId,
        localDeviceId,
        data,
        messageAgeSec
      );
    }
  }

  // Called by Rust
  sendHttpRequest(
    requestId: number,
    url: string,
    method: HttpMethod,
    headers: { [name: string]: string },
    body: Buffer | undefined
  ): void {
    if (this.handleSendHttpRequest) {
      this.handleSendHttpRequest(requestId, url, method, headers, body);
    } else {
      this.logError('RingRTC.handleSendHttpRequest is not set!');
    }
  }

  // Called by Rust
  sendCallMessage(
    recipientUuid: Buffer,
    message: Buffer,
    urgency: CallMessageUrgency
  ): void {
    if (this.handleSendCallMessage) {
      this.handleSendCallMessage(recipientUuid, message, urgency);
    } else {
      this.logError('RingRTC.handleSendCallMessage is not set!');
    }
  }

  // Called by Rust
  sendCallMessageToGroup(
    groupId: Buffer,
    message: Buffer,
    urgency: CallMessageUrgency,
    overrideRecipients: Array<Buffer>
  ): void {
    if (this.handleSendCallMessageToGroup) {
      this.handleSendCallMessageToGroup(
        groupId,
        message,
        urgency,
        overrideRecipients
      );
    } else {
      this.logError('RingRTC.handleSendCallMessageToGroup is not set!');
    }
  }

  // These are convenience methods.  One could use the Call class instead.
  get call(): Call | null {
    return this._call;
  }

  getCall(callId: CallId): Call | null {
    const call = this.call;

    if (
      call &&
      call.callId.high === callId.high &&
      call.callId.low === callId.low
    ) {
      return call;
    }
    return null;
  }

  accept(callId: CallId, asVideoCall: boolean): void {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.accept();
    call.outgoingAudioEnabled = true;
    call.outgoingVideoEnabled = asVideoCall;
  }

  decline(callId: CallId): void {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.decline();
  }

  ignore(callId: CallId): void {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.ignore();
  }

  hangup(callId: CallId): void {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.hangup();
  }

  setOutgoingAudio(callId: CallId, enabled: boolean): void {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.outgoingAudioEnabled = enabled;
  }

  setOutgoingVideo(callId: CallId, enabled: boolean): void {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.outgoingVideoEnabled = enabled;
  }

  setOutgoingVideoIsScreenShare(callId: CallId, isScreenShare: boolean): void {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.outgoingVideoIsScreenShare = isScreenShare;
  }

  setVideoCapturer(callId: CallId, capturer: VideoCapturer | null): void {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.videoCapturer = capturer;
  }

  setVideoRenderer(callId: CallId, renderer: VideoRenderer | null): void {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.videoRenderer = renderer;
  }

  getAudioInputs(): Array<AudioDevice> {
    return this.callManager.getAudioInputs();
  }

  setAudioInput(index: number): void {
    this.callManager.setAudioInput(index);
  }

  getAudioOutputs(): Array<AudioDevice> {
    return this.callManager.getAudioOutputs();
  }

  setAudioOutput(index: number): void {
    this.callManager.setAudioOutput(index);
  }
}

export interface CallSettings {
  iceServers: Array<IceServer>;
  hideIp: boolean;
  dataMode: DataMode;
  audioLevelsIntervalMillis?: number;
}

interface IceServer {
  username?: string;
  password?: string;
  /** Provide hostname when urls contain IP addresses instead of hostname */
  hostname?: string;
  urls: Array<string>;
}

// Describes an audio input or output device.
export interface AudioDevice {
  // Device name.
  name: string;
  // Index of this device, starting from 0.
  index: number;
  // A unique and somewhat stable identifier of this device.
  uniqueId: string;
  // If present, the identifier of a localized string to substitute for the device name.
  i18nKey?: string;
}

export interface VideoCapturer {
  enableCapture(): void;
  enableCaptureAndSend(
    call: Call,
    captureOptions?: GumVideoCaptureOptions
  ): void;
  disable(): void;
}

export interface VideoRenderer {
  enable(call: Call): void;
  disable(): void;
}
export class Call {
  // The calls' info and state.
  private readonly _callManager: CallManager;
  private readonly _remoteUserId: UserId;
  // We can have a null CallId while we're waiting for RingRTC to give us one.
  callId: CallId;
  private readonly _isIncoming: boolean;
  private readonly _isVideoCall: boolean;
  private _state: CallState;
  private _outgoingAudioEnabled = false;
  private _outgoingVideoEnabled = false;
  private _outgoingVideoIsScreenShare = false;
  private _remoteVideoEnabled = false;
  outgoingAudioLevel: NormalizedAudioLevel = 0;
  remoteAudioLevel: NormalizedAudioLevel = 0;
  remoteSharingScreen = false;
  networkRoute: NetworkRoute = new NetworkRoute();
  private _videoCapturer: VideoCapturer | null = null;
  private _videoRenderer: VideoRenderer | null = null;
  endedReason?: CallEndedReason;

  // These callbacks should be set by the UX code.
  handleStateChanged?: () => void;
  handleRemoteVideoEnabled?: () => void;
  handleRemoteSharingScreen?: () => void;
  handleNetworkRouteChanged?: () => void;
  handleAudioLevels?: () => void;

  /**
   * Notification of low upload bandwidth for sending video.
   *
   * When this is first called, recovered will be false. The second call (if
   * any) will have recovered set to true and will be called when the upload
   * bandwidth is high enough to send video.
   *
   * @param recovered - whether there is enough bandwidth to send video reliably
   */
  handleLowBandwidthForVideo?: (recovered: boolean) => void;

  // This callback should be set by the VideoCapturer,
  // But could also be set by the UX.
  renderVideoFrame?: (width: number, height: number, buffer: Buffer) => void;

  constructor(
    callManager: CallManager,
    remoteUserId: UserId,
    callId: CallId,
    isIncoming: boolean,
    isVideoCall: boolean,
    state: CallState
  ) {
    this._callManager = callManager;
    this._remoteUserId = remoteUserId;
    this.callId = callId;
    this._isIncoming = isIncoming;
    this._isVideoCall = isVideoCall;
    this._state = state;
  }

  get remoteUserId(): UserId {
    return this._remoteUserId;
  }

  get isIncoming(): boolean {
    return this._isIncoming;
  }

  get isVideoCall(): boolean {
    return this._isVideoCall;
  }

  get state(): CallState {
    return this._state;
  }

  set state(state: CallState) {
    if (state == this._state) {
      return;
    }
    this._state = state;
    this.enableOrDisableCapturer();
    this.enableOrDisableRenderer();
    if (this.handleStateChanged) {
      this.handleStateChanged();
    }
  }

  setCallEnded(): void {
    this._state = CallState.Ended;
  }

  set videoCapturer(capturer: VideoCapturer | null) {
    this._videoCapturer = capturer;
    this.enableOrDisableCapturer();
  }

  set videoRenderer(renderer: VideoRenderer | null) {
    this._videoRenderer = renderer;
    this.enableOrDisableRenderer();
  }

  accept(): void {
    this._callManager.accept(this.callId);
  }

  decline(): void {
    this.hangup();
  }

  ignore(): void {
    this._callManager.ignore(this.callId);
  }

  hangup(): void {
    // This is a little faster than waiting for the
    // change in call state to come back.
    if (this._videoCapturer) {
      this._videoCapturer.disable();
    }
    if (this._videoRenderer) {
      this._videoRenderer.disable();
    }
    // This assumes we only have one active call.
    sillyDeadlockProtection(() => {
      this._callManager.hangup();
    });
  }

  get outgoingAudioEnabled(): boolean {
    return this._outgoingAudioEnabled;
  }

  set outgoingAudioEnabled(enabled: boolean) {
    this._outgoingAudioEnabled = enabled;
    // This assumes we only have one active call.
    sillyDeadlockProtection(() => {
      this._callManager.setOutgoingAudioEnabled(enabled);
    });
  }

  get outgoingVideoEnabled(): boolean {
    return this._outgoingVideoEnabled;
  }

  set outgoingVideoEnabled(enabled: boolean) {
    this._outgoingVideoEnabled = enabled;
    this.enableOrDisableCapturer();
  }

  set outgoingVideoIsScreenShare(isScreenShare: boolean) {
    // This assumes we only have one active call.
    this._outgoingVideoIsScreenShare = isScreenShare;
    sillyDeadlockProtection(() => {
      this._callManager.setOutgoingVideoIsScreenShare(isScreenShare);
    });
  }

  get remoteVideoEnabled(): boolean {
    return this._remoteVideoEnabled;
  }

  set remoteVideoEnabled(enabled: boolean) {
    this._remoteVideoEnabled = enabled;
    this.enableOrDisableRenderer();
  }

  // With this method, a Call is a VideoFrameSender
  sendVideoFrame(
    width: number,
    height: number,
    format: VideoPixelFormatEnum,
    buffer: Buffer
  ): void {
    // This assumes we only have one active call.
    this._callManager.sendVideoFrame(width, height, format, buffer);
  }

  // With this method, a Call is a VideoFrameSource
  receiveVideoFrame(
    buffer: Buffer,
    maxWidth: number,
    maxHeight: number
  ): [number, number] | undefined {
    // This assumes we only have one active call.
    return this._callManager.receiveVideoFrame(buffer, maxWidth, maxHeight);
  }

  private enableOrDisableCapturer(): void {
    if (!this._videoCapturer) {
      return;
    }
    if (!this.outgoingVideoEnabled) {
      this._videoCapturer.disable();
      if (this.state === CallState.Accepted) {
        this.setOutgoingVideoEnabled(false);
      }
      return;
    }
    switch (this.state) {
      case CallState.Prering:
      case CallState.Ringing:
        this._videoCapturer.enableCapture();
        break;
      case CallState.Accepted:
        this._videoCapturer.enableCaptureAndSend(this);
        this.setOutgoingVideoEnabled(true);
        if (this._outgoingVideoIsScreenShare) {
          // Make sure the status gets sent.
          this.outgoingVideoIsScreenShare = true;
        }
        break;
      case CallState.Reconnecting:
        this._videoCapturer.enableCaptureAndSend(this);
        // Don't send status until we're reconnected.
        break;
      case CallState.Ended:
        this._videoCapturer.disable();
        break;
      default:
    }
  }

  private setOutgoingVideoEnabled(enabled: boolean) {
    sillyDeadlockProtection(() => {
      try {
        this._callManager.setOutgoingVideoEnabled(enabled);
      } catch {
        // We may not have an active connection any more.
        // In which case it doesn't matter
      }
    });
  }

  updateDataMode(dataMode: DataMode): void {
    sillyDeadlockProtection(() => {
      try {
        this._callManager.updateDataMode(dataMode);
      } catch {
        // We may not have an active connection any more.
        // In which case it doesn't matter
      }
    });
  }

  private enableOrDisableRenderer(): void {
    if (!this._videoRenderer) {
      return;
    }
    if (!this.remoteVideoEnabled) {
      this._videoRenderer.disable();
      return;
    }
    switch (this.state) {
      case CallState.Prering:
      case CallState.Ringing:
        this._videoRenderer.disable();
        break;
      case CallState.Accepted:
      case CallState.Reconnecting:
        this._videoRenderer.enable(this);
        break;
      case CallState.Ended:
        this._videoRenderer.disable();
        break;
      default:
    }
  }
}

// Group Calls

export type GroupCallClientId = number;

// Represents the connection state to a media server for a group call.
export enum ConnectionState {
  NotConnected = 0,
  Connecting,
  Connected,
  Reconnecting,
}

// Represents whether or not a user is joined to a group call and can exchange media.
export enum JoinState {
  NotJoined = 0,
  Joining,
  Pending,
  Joined,
}

// If not ended purposely by the user, gives the reason why a group call ended.
export enum GroupCallEndReason {
  // Normal events
  DeviceExplicitlyDisconnected = 0,
  ServerExplicitlyDisconnected,
  DeniedRequestToJoinCall,
  RemovedFromCall,

  // Things that can go wrong
  CallManagerIsBusy,
  SfuClientFailedToJoin,
  FailedToCreatePeerConnectionFactory,
  FailedToNegotiateSrtpKeys,
  FailedToCreatePeerConnection,
  FailedToStartPeerConnection,
  FailedToUpdatePeerConnection,
  FailedToSetMaxSendBitrate,
  IceFailedWhileConnecting,
  IceFailedAfterConnected,
  ServerChangedDemuxId,
  HasMaxDevices,
}

export enum CallMessageUrgency {
  Droppable = 0,
  HandleImmediately,
}

export enum RingUpdate {
  /// The sender is trying to ring this user.
  Requested = 0,
  /// The sender tried to ring this user, but it's been too long.
  ExpiredRequest,
  /// Call was accepted elsewhere by a different device.
  AcceptedOnAnotherDevice,
  /// Call was declined elsewhere by a different device.
  DeclinedOnAnotherDevice,
  /// This device is currently on a different call.
  BusyLocally,
  /// A different device is currently on a different call.
  BusyOnAnotherDevice,
  /// The sender cancelled the ring request.
  CancelledByRinger,
}

// HTTP request methods.
export enum HttpMethod {
  Get = 0,
  Put = 1,
  Post = 2,
  Delete = 3,
}

// The local device state for a group call.
export class LocalDeviceState {
  connectionState: ConnectionState;
  joinState: JoinState;
  // Set after joined
  demuxId?: number;
  audioMuted: boolean;
  videoMuted: boolean;
  audioLevel: NormalizedAudioLevel;
  presenting: boolean;
  sharingScreen: boolean;
  networkRoute: NetworkRoute;

  constructor() {
    this.connectionState = ConnectionState.NotConnected;
    this.joinState = JoinState.NotJoined;
    // By default audio and video are muted.
    this.audioMuted = true;
    this.videoMuted = true;
    this.audioLevel = 0;
    this.presenting = false;
    this.sharingScreen = false;
    this.networkRoute = new NetworkRoute();
  }
}

// All remote devices in a group call and their associated state.
export class RemoteDeviceState {
  demuxId: number; // UInt32
  userId: Buffer;
  mediaKeysReceived: boolean;
  audioMuted: boolean | undefined;
  videoMuted: boolean | undefined;
  audioLevel: NormalizedAudioLevel;
  presenting: boolean | undefined;
  sharingScreen: boolean | undefined;
  videoAspectRatio: number | undefined; // Float
  addedTime: string; // unix millis (to be converted to a numeric type)
  speakerTime: string; // unix millis; 0 if they've never spoken (to be converted to a numeric type)
  forwardingVideo: boolean | undefined;
  isHigherResolutionPending: boolean;

  constructor(
    demuxId: number,
    userId: Buffer,
    addedTime: string,
    speakerTime: string,
    mediaKeysReceived: boolean
  ) {
    this.demuxId = demuxId;
    this.userId = userId;
    this.mediaKeysReceived = mediaKeysReceived;
    this.audioLevel = 0;
    this.addedTime = addedTime;
    this.speakerTime = speakerTime;
    this.isHigherResolutionPending = false;
  }
}

// Used to communicate the group membership to RingRTC for a group call.
export class GroupMemberInfo {
  userId: Buffer;
  userIdCipherText: Buffer;

  constructor(userId: Buffer, userIdCipherText: Buffer) {
    this.userId = userId;
    this.userIdCipherText = userIdCipherText;
  }
}

// Used for the application to communicate the actual resolutions of
// each device in a group call to RingRTC and the SFU.
export class VideoRequest {
  demuxId: number; // UInt32
  width: number; // UInt16
  height: number; // UInt16
  framerate: number | undefined; // UInt16

  constructor(
    demuxId: number,
    width: number,
    height: number,
    framerate: number | undefined
  ) {
    this.demuxId = demuxId;
    this.width = width;
    this.height = height;
    this.framerate = framerate;
  }
}

export enum GroupCallKind {
  SignalGroup,
  CallLink,
}

export interface GroupCallObserver {
  requestMembershipProof(groupCall: GroupCall): void;
  requestGroupMembers(groupCall: GroupCall): void;
  onLocalDeviceStateChanged(groupCall: GroupCall): void;
  onRemoteDeviceStatesChanged(groupCall: GroupCall): void;
  onAudioLevels(groupCall: GroupCall): void;
  onLowBandwidthForVideo(groupCall: GroupCall, recovered: boolean): void;
  onReactions(groupCall: GroupCall, reactions: Array<Reaction>): void;
  onRaisedHands(groupCall: GroupCall, raisedHands: Array<number>): void;
  onPeekChanged(groupCall: GroupCall): void;
  onEnded(groupCall: GroupCall, reason: GroupCallEndReason): void;
}

export class GroupCall {
  private readonly _kind: GroupCallKind;
  private readonly _callManager: CallManager;
  private readonly _observer: GroupCallObserver;

  private readonly _clientId: GroupCallClientId;

  public get clientId(): GroupCallClientId {
    return this._clientId;
  }

  private _localDeviceState: LocalDeviceState;
  private _remoteDeviceStates: Array<RemoteDeviceState> | undefined;

  private _peekInfo: PeekInfo | undefined;

  // Called by UI via RingRTC object
  constructor(
    kind: GroupCallKind,
    callManager: CallManager,
    observer: GroupCallObserver,
    clientId: GroupCallClientId
  ) {
    this._kind = kind;
    this._callManager = callManager;
    this._observer = observer;
    this._clientId = clientId;
    this._localDeviceState = new LocalDeviceState();
  }

  getKind(): GroupCallKind {
    return this._kind;
  }

  // Called by UI
  connect(): void {
    this._callManager.connect(this._clientId);
  }

  // Called by UI
  join(): void {
    this._callManager.join(this._clientId);
  }

  // Called by UI
  leave(): void {
    this._callManager.leave(this._clientId);
  }

  // Called by UI
  disconnect(): void {
    this._callManager.disconnect(this._clientId);
  }

  // Called by UI
  getLocalDeviceState(): LocalDeviceState {
    return this._localDeviceState;
  }

  // Called by UI
  getRemoteDeviceStates(): Array<RemoteDeviceState> | undefined {
    return this._remoteDeviceStates;
  }

  // Called by UI
  getPeekInfo(): PeekInfo | undefined {
    return this._peekInfo;
  }

  // Called by UI
  setOutgoingAudioMuted(muted: boolean): void {
    this._localDeviceState.audioMuted = muted;
    this._callManager.setOutgoingAudioMuted(this._clientId, muted);
    this._observer.onLocalDeviceStateChanged(this);
  }

  // Called by UI
  react(value: string): void {
    this._callManager.groupReact(this._clientId, value);
  }

  // Called by UI
  raiseHand(raise: boolean): void {
    this._callManager.groupRaiseHand(this._clientId, raise);
  }

  // Called by UI
  setOutgoingVideoMuted(muted: boolean): void {
    this._localDeviceState.videoMuted = muted;
    this._callManager.setOutgoingVideoMuted(this._clientId, muted);
    this._observer.onLocalDeviceStateChanged(this);
  }

  // Called by UI
  setPresenting(presenting: boolean): void {
    this._localDeviceState.presenting = presenting;
    this._callManager.setPresenting(this._clientId, presenting);
    this._observer.onLocalDeviceStateChanged(this);
  }

  // Called by UI
  setOutgoingVideoIsScreenShare(isScreenShare: boolean): void {
    this._localDeviceState.sharingScreen = isScreenShare;
    this._callManager.setOutgoingGroupCallVideoIsScreenShare(
      this._clientId,
      isScreenShare
    );
    this._observer.onLocalDeviceStateChanged(this);
  }

  // Called by UI
  ringAll(): void {
    this._callManager.groupRing(this._clientId, undefined);
  }

  // Called by UI
  resendMediaKeys(): void {
    this._callManager.resendMediaKeys(this._clientId);
  }

  // Called by UI
  setDataMode(dataMode: DataMode): void {
    this._callManager.setDataMode(this._clientId, dataMode);
  }

  // Called by UI
  requestVideo(
    resolutions: Array<VideoRequest>,
    activeSpeakerHeight: number
  ): void {
    this._callManager.requestVideo(
      this._clientId,
      resolutions,
      activeSpeakerHeight
    );
  }

  // Called by UI
  approveUser(otherUserId: Buffer): void {
    this._callManager.approveUser(this._clientId, otherUserId);
  }

  // Called by UI
  denyUser(otherUserId: Buffer): void {
    this._callManager.denyUser(this._clientId, otherUserId);
  }

  // Called by UI
  removeClient(otherClientDemuxId: number): void {
    this._callManager.removeClient(this._clientId, otherClientDemuxId);
  }

  // Called by UI
  blockClient(otherClientDemuxId: number): void {
    this._callManager.blockClient(this._clientId, otherClientDemuxId);
  }

  // Called by UI
  setGroupMembers(members: Array<GroupMemberInfo>): void {
    this._callManager.setGroupMembers(this._clientId, members);
  }

  // Called by UI
  setMembershipProof(proof: Buffer): void {
    this._callManager.setMembershipProof(this._clientId, proof);
  }

  // Called by Rust via RingRTC object
  requestMembershipProof(): void {
    this._observer.requestMembershipProof(this);
  }

  // Called by Rust via RingRTC object
  requestGroupMembers(): void {
    this._observer.requestGroupMembers(this);
  }

  // Called by Rust via RingRTC object
  handleConnectionStateChanged(connectionState: ConnectionState): void {
    this._localDeviceState.connectionState = connectionState;

    this._observer.onLocalDeviceStateChanged(this);
  }

  // Called by Rust via RingRTC object
  handleJoinStateChanged(
    joinState: JoinState,
    demuxId: number | undefined
  ): void {
    this._localDeviceState.joinState = joinState;

    // Don't set to undefined after we leave so we can still know the demuxId after we leave.
    if (demuxId != undefined) {
      this._localDeviceState.demuxId = demuxId;
    }

    this._observer.onLocalDeviceStateChanged(this);
  }

  // Called by Rust via RingRTC object
  handleNetworkRouteChanged(localNetworkAdapterType: NetworkAdapterType): void {
    this._localDeviceState.networkRoute.localAdapterType =
      localNetworkAdapterType;

    this._observer.onLocalDeviceStateChanged(this);
  }

  handleAudioLevels(
    capturedLevel: RawAudioLevel,
    receivedLevels: Array<ReceivedAudioLevel>
  ): void {
    this._localDeviceState.audioLevel = normalizeAudioLevel(capturedLevel);
    if (this._remoteDeviceStates != undefined) {
      for (const received of receivedLevels) {
        for (const remoteDeviceState of this._remoteDeviceStates) {
          if (remoteDeviceState.demuxId == received.demuxId) {
            remoteDeviceState.audioLevel = normalizeAudioLevel(received.level);
          }
        }
      }
    }

    this._observer.onAudioLevels(this);
  }

  handleLowBandwidthForVideo(recovered: boolean): void {
    this._observer.onLowBandwidthForVideo(this, recovered);
  }

  handleReactions(reactions: Array<Reaction>): void {
    this._observer.onReactions(this, reactions);
  }

  handleRaisedHands(raisedHands: Array<number>): void {
    this._observer.onRaisedHands(this, raisedHands);
  }

  // Called by Rust via RingRTC object
  handleRemoteDevicesChanged(
    remoteDeviceStates: Array<RemoteDeviceState>
  ): void {
    // We don't get aspect ratios from RingRTC, so make sure to copy them over.
    for (const noo of remoteDeviceStates) {
      const old = this._remoteDeviceStates?.find(
        old => old.demuxId == noo.demuxId
      );
      noo.videoAspectRatio = old?.videoAspectRatio;
    }

    this._remoteDeviceStates = remoteDeviceStates;

    this._observer.onRemoteDeviceStatesChanged(this);
  }

  // Called by Rust via RingRTC object
  handlePeekChanged(info: PeekInfo): void {
    this._peekInfo = info;

    this._observer.onPeekChanged(this);
  }

  // Called by Rust via RingRTC object
  handleEnded(reason: GroupCallEndReason): void {
    this._observer.onEnded(this, reason);

    this._callManager.deleteGroupCallClient(this._clientId);
  }

  // With this, a GroupCall is a VideoFrameSender
  sendVideoFrame(
    width: number,
    height: number,
    format: VideoPixelFormatEnum,
    buffer: Buffer
  ): void {
    // This assumes we only have one active call.
    this._callManager.sendVideoFrame(width, height, format, buffer);
  }

  // With this, a GroupCall can provide a VideoFrameSource for each remote device.
  getVideoSource(remoteDemuxId: number): GroupCallVideoFrameSource {
    return new GroupCallVideoFrameSource(
      this._callManager,
      this,
      remoteDemuxId
    );
  }

  // Called by the GroupCallVideoFrameSource when it receives a video frame.
  setRemoteAspectRatio(remoteDemuxId: number, aspectRatio: number): void {
    const remoteDevice = this._remoteDeviceStates?.find(
      device => device.demuxId == remoteDemuxId
    );
    if (!!remoteDevice && remoteDevice.videoAspectRatio != aspectRatio) {
      remoteDevice.videoAspectRatio = aspectRatio;
      this._observer.onRemoteDeviceStatesChanged(this);
    }
  }

  setRtcStatsInterval(intervalMillis: number): void {
    this._callManager.setRtcStatsInterval(this._clientId, intervalMillis);
  }
}

// Implements VideoSource for use in CanvasVideoRenderer
class GroupCallVideoFrameSource {
  private readonly _callManager: CallManager;
  private readonly _groupCall: GroupCall;
  private readonly _remoteDemuxId: number; // Uint32

  constructor(
    callManager: CallManager,
    groupCall: GroupCall,
    remoteDemuxId: number // Uint32
  ) {
    this._callManager = callManager;
    this._groupCall = groupCall;
    this._remoteDemuxId = remoteDemuxId;
  }

  receiveVideoFrame(
    buffer: Buffer,
    maxWidth: number,
    maxHeight: number
  ): [number, number] | undefined {
    // This assumes we only have one active call.
    const frame = this._callManager.receiveGroupCallVideoFrame(
      this._groupCall.clientId,
      this._remoteDemuxId,
      buffer,
      maxWidth,
      maxHeight
    );
    if (frame) {
      const [width, height] = frame;
      this._groupCall.setRemoteAspectRatio(this._remoteDemuxId, width / height);
    }
    return frame;
  }
}

// When sending, we just set an Buffer.
// When receiving, we call .toArrayBuffer().
type ProtobufBuffer = Buffer | { toArrayBuffer: () => ArrayBuffer };

function toBuffer(pbab: ProtobufBuffer | undefined): Buffer | undefined {
  if (!pbab) {
    return pbab;
  }
  if (pbab instanceof Buffer) {
    return pbab;
  }
  return Buffer.from(pbab.toArrayBuffer());
}

export type UserId = string;

export type DeviceId = number;

// A stripped-down version of Long.
export type CallId = {
  high: number;
  low: number;
  // RingRTC always treats call IDs as unsigned internally regardless of what this is set to.
  // Call IDs produced by RingRTC will always set this to `true`.
  unsigned: boolean;
};

export class CallingMessage {
  offer?: OfferMessage;
  answer?: AnswerMessage;
  iceCandidates?: Array<IceCandidateMessage>;
  busy?: BusyMessage;
  hangup?: HangupMessage;
  opaque?: OpaqueMessage;
  destinationDeviceId?: DeviceId;
}

export class OfferMessage {
  callId: CallId;
  type: OfferType;
  opaque: ProtobufBuffer;

  constructor(callId: CallId, type: OfferType, opaque: ProtobufBuffer) {
    this.callId = callId;
    this.type = type;
    this.opaque = opaque;
  }
}

export enum OfferType {
  AudioCall = 0,
  VideoCall = 1,
}

export class AnswerMessage {
  callId: CallId;
  opaque: ProtobufBuffer;

  constructor(callId: CallId, opaque: ProtobufBuffer) {
    this.callId = callId;
    this.opaque = opaque;
  }
}

export class IceCandidateMessage {
  callId: CallId;
  opaque: ProtobufBuffer;

  constructor(callId: CallId, opaque: ProtobufBuffer) {
    this.callId = callId;
    this.opaque = opaque;
  }
}

export class BusyMessage {
  callId: CallId;

  constructor(callId: CallId) {
    this.callId = callId;
  }
}

export class HangupMessage {
  callId: CallId;
  type: HangupType;
  deviceId: DeviceId;

  constructor(callId: CallId, type: HangupType, deviceId: DeviceId) {
    this.callId = callId;
    this.type = type;
    this.deviceId = deviceId;
  }
}

export class OpaqueMessage {
  data?: ProtobufBuffer;
}

export enum HangupType {
  Normal = 0,
  Accepted = 1,
  Declined = 2,
  Busy = 3,
  NeedPermission = 4,
}

export enum DataMode {
  Low = 0,
  Normal = 1,
}

/// Describes why a ring was cancelled.
export enum RingCancelReason {
  /// The user explicitly clicked "Decline".
  DeclinedByUser = 0,
  /// The device is busy with another call.
  Busy,
}

export interface CallManager {
  setConfig(config: Config): void;
  setSelfUuid(uuid: Buffer): void;
  createOutgoingCall(
    remoteUserId: UserId,
    isVideoCall: boolean,
    localDeviceId: DeviceId
  ): CallId;
  proceed(
    callId: CallId,
    iceServers: Array<IceServer>,
    hideIp: boolean,
    dataMode: DataMode,
    audioLevelsIntervalMillis: number
  ): void;
  accept(callId: CallId): void;
  ignore(callId: CallId): void;
  hangup(): void;
  cancelGroupRing(
    groupId: GroupId,
    ringId: string,
    reason: RingCancelReason | null
  ): void;
  signalingMessageSent(callId: CallId): void;
  signalingMessageSendFailed(callId: CallId): void;
  setOutgoingAudioEnabled(enabled: boolean): void;
  setOutgoingVideoEnabled(enabled: boolean): void;
  setOutgoingVideoIsScreenShare(enabled: boolean): void;
  updateDataMode(dataMode: DataMode): void;
  sendVideoFrame(
    width: number,
    height: number,
    format: VideoPixelFormatEnum,
    buffer: Buffer
  ): void;
  receiveVideoFrame(
    buffer: Buffer,
    maxWidth: number,
    maxHeight: number
  ): [number, number] | undefined;
  receivedOffer(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    localDeviceId: DeviceId,
    messageAgeSec: number,
    callId: CallId,
    offerType: OfferType,
    opaque: Buffer,
    senderIdentityKey: Buffer,
    receiverIdentityKey: Buffer
  ): void;
  receivedAnswer(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    opaque: Buffer,
    senderIdentityKey: Buffer,
    receiverIdentityKey: Buffer
  ): void;
  receivedIceCandidates(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    candidates: Array<Buffer>
  ): void;
  receivedHangup(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    hangupType: HangupType,
    hangupDeviceId: DeviceId | null
  ): void;
  receivedBusy(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId
  ): void;
  receivedCallMessage(
    remoteUserId: Buffer,
    remoteDeviceId: DeviceId,
    localDeviceId: DeviceId,
    data: Buffer,
    messageAgeSec: number
  ): void;

  receivedHttpResponse(requestId: number, status: number, body: Buffer): void;
  httpRequestFailed(requestId: number, debugInfo: string | undefined): void;

  // Group Calls

  createGroupCallClient(
    groupId: Buffer,
    sfuUrl: string,
    hkdfExtraInfo: Buffer,
    audioLevelsIntervalMillis: number
  ): GroupCallClientId;
  createCallLinkCallClient(
    sfuUrl: string,
    authCredentialPresentation: Buffer,
    linkRootKey: Buffer,
    adminPasskey: Buffer | undefined,
    hkdfExtraInfo: Buffer,
    audioLevelsIntervalMillis: number
  ): GroupCallClientId;
  deleteGroupCallClient(clientId: GroupCallClientId): void;
  connect(clientId: GroupCallClientId): void;
  join(clientId: GroupCallClientId): void;
  leave(clientId: GroupCallClientId): void;
  disconnect(clientId: GroupCallClientId): void;
  setOutgoingAudioMuted(clientId: GroupCallClientId, muted: boolean): void;
  setOutgoingVideoMuted(clientId: GroupCallClientId, muted: boolean): void;
  setPresenting(clientId: GroupCallClientId, presenting: boolean): void;
  setOutgoingGroupCallVideoIsScreenShare(
    clientId: GroupCallClientId,
    isScreenShare: boolean
  ): void;
  groupRing(clientId: GroupCallClientId, recipient: Buffer | undefined): void;
  groupReact(clientId: GroupCallClientId, value: string): void;
  groupRaiseHand(clientId: GroupCallClientId, raise: boolean): void;
  resendMediaKeys(clientId: GroupCallClientId): void;
  setDataMode(clientId: GroupCallClientId, dataMode: DataMode): void;
  requestVideo(
    clientId: GroupCallClientId,
    resolutions: Array<VideoRequest>,
    activeSpeakerHeight: number
  ): void;
  approveUser(clientId: GroupCallClientId, otherUserId: Buffer): void;
  denyUser(clientId: GroupCallClientId, otherUserId: Buffer): void;
  removeClient(clientId: GroupCallClientId, otherClientDemuxId: number): void;
  blockClient(clientId: GroupCallClientId, otherClientDemuxId: number): void;
  setGroupMembers(
    clientId: GroupCallClientId,
    members: Array<GroupMemberInfo>
  ): void;
  setMembershipProof(clientId: GroupCallClientId, proof: Buffer): void;
  // Same as receiveVideoFrame, but with a specific GroupCallClientId and remoteDemuxId.
  receiveGroupCallVideoFrame(
    clientId: GroupCallClientId,
    remoteDemuxId: number,
    buffer: Buffer,
    maxWidth: number,
    maxHeight: number
  ): [number, number] | undefined;
  setRtcStatsInterval(
    clientId: GroupCallClientId,
    intervalMillis: number
  ): void;
  // Responses come back via handleCallLinkResponse
  readCallLink(
    requestId: number,
    sfuUrl: string,
    authCredentialPresentation: Buffer,
    linkRootKey: Buffer
  ): void;
  createCallLink(
    requestId: number,
    sfuUrl: string,
    createCredentialPresentation: Buffer,
    linkRootKey: Buffer,
    adminPasskey: Buffer,
    callLinkPublicParams: Buffer
  ): void;
  updateCallLink(
    requestId: number,
    sfuUrl: string,
    authCredentialPresentation: Buffer,
    linkRootKey: Buffer,
    adminPasskey: Buffer,
    newName: string | undefined,
    newRestrictions: number | undefined,
    newRevoked: boolean | undefined
  ): void;
  deleteCallLink(
    requestId: number,
    sfuUrl: string,
    authCredentialPresentation: Buffer,
    linkRootKey: Buffer,
    adminPasskey: Buffer
  ): void;
  // Response comes back via handlePeekResponse
  peekGroupCall(
    requestId: number,
    sfu_url: string,
    membership_proof: Buffer,
    group_members: Array<GroupMemberInfo>
  ): void;
  // Response comes back via handlePeekResponse
  peekCallLinkCall(
    requestId: number,
    sfuUrl: string,
    authCredentialPresentation: Buffer,
    linkRootKey: Buffer
  ): void;

  getAudioInputs(): Array<AudioDevice>;
  setAudioInput(index: number): void;
  getAudioOutputs(): Array<AudioDevice>;
  setAudioOutput(index: number): void;
}

export interface CallManagerCallbacks {
  onStartOutgoingCall(remoteUserId: UserId, callId: CallId): void;
  onStartIncomingCall(
    remoteUserId: UserId,
    callId: CallId,
    isVideoCall: boolean
  ): void;
  onCallState(remoteUserId: UserId, state: CallState): void;
  onCallEnded(
    remoteUserId: UserId,
    callId: CallId,
    endedReason: CallEndedReason,
    ageSec: number
  ): void;
  onRemoteVideoEnabled(remoteUserId: UserId, enabled: boolean): void;
  onRemoteSharingScreen(remoteUserId: UserId, enabled: boolean): void;
  onSendOffer(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    mediaType: number,
    opaque: Buffer
  ): void;
  onSendAnswer(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    opaque: Buffer
  ): void;
  onSendIceCandidates(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    candidates: Array<Buffer>
  ): void;
  onSendHangup(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    HangupType: HangupType,
    hangupDeviceId: DeviceId | null
  ): void;
  onSendBusy(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean
  ): void;
  sendCallMessage(
    recipientUuid: Buffer,
    message: Buffer,
    urgency: CallMessageUrgency
  ): void;
  sendCallMessageToGroup(
    groupId: Buffer,
    message: Buffer,
    urgency: CallMessageUrgency,
    overrideRecipients: Array<Buffer>
  ): void;
  sendHttpRequest(
    requestId: number,
    url: string,
    method: HttpMethod,
    headers: { [name: string]: string },
    body: Buffer | undefined
  ): void;

  // Group Calls

  requestMembershipProof(clientId: GroupCallClientId): void;
  requestGroupMembers(clientId: GroupCallClientId): void;
  handleConnectionStateChanged(
    clientId: GroupCallClientId,
    connectionState: ConnectionState
  ): void;
  handleJoinStateChanged(
    clientId: GroupCallClientId,
    joinState: JoinState,
    demuxId: number | undefined
  ): void;
  handleRemoteDevicesChanged(
    clientId: GroupCallClientId,
    remoteDeviceStates: Array<RemoteDeviceState>
  ): void;
  handlePeekChanged(clientId: GroupCallClientId, rawInfo: RawPeekInfo): void;
  handlePeekResponse(
    requestId: number,
    statusCode: number,
    rawInfo: RawPeekInfo | undefined
  ): void;
  handleEnded(clientId: GroupCallClientId, reason: GroupCallEndReason): void;

  onLogMessage(
    level: number,
    fileName: string,
    line: number,
    message: string
  ): void;
}

export enum CallState {
  Prering = 'idle',
  Ringing = 'ringing',
  Accepted = 'connected',
  Reconnecting = 'connecting',
  Ended = 'ended',
}

export enum CallEndedReason {
  LocalHangup = 'LocalHangup',
  RemoteHangup = 'RemoteHangup',
  RemoteHangupNeedPermission = 'RemoteHangupNeedPermission',
  Declined = 'Declined',
  Busy = 'Busy',
  Glare = 'Glare',
  ReCall = 'ReCall',
  ReceivedOfferExpired = 'ReceivedOfferExpired',
  ReceivedOfferWhileActive = 'ReceivedOfferWhileActive',
  ReceivedOfferWithGlare = 'ReceivedOfferWithGlare',
  SignalingFailure = 'SignalingFailure',
  GlareFailure = 'GlareFailure',
  ConnectionFailure = 'ConnectionFailure',
  InternalFailure = 'InternalFailure',
  Timeout = 'Timeout',
  AcceptedOnAnotherDevice = 'AcceptedOnAnotherDevice',
  DeclinedOnAnotherDevice = 'DeclinedOnAnotherDevice',
  BusyOnAnotherDevice = 'BusyOnAnotherDevice',
}

export enum CallLogLevel {
  Off,
  Error,
  Warn,
  Info,
  Debug,
  Trace,
}

function sillyDeadlockProtection(f: () => void) {
  void (async () => {
    // This is a silly way of preventing a deadlock.
    // eslint-disable-next-line @typescript-eslint/await-thenable
    await 0;

    f();
  })();
}
