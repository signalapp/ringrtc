//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

/* tslint:disable max-classes-per-file */

const os = require('os');

// tslint:disable-next-line no-var-requires no-require-imports
const Native = require('../../build/' + os.platform() + '/libringrtc.node');

type GroupId = ArrayBuffer;
type GroupCallUserId = ArrayBuffer;

export class PeekInfo {
  joinedMembers: Array<GroupCallUserId>;
  creator?: GroupCallUserId;
  eraId?: string;
  maxDevices?: number;
  deviceCount: number;

  constructor() {
    this.joinedMembers = [];
    this.deviceCount = 0;
  }
}

class Requests<T> {
  private _resolveById: Map<number, (response: T) => void> = new Map();
  private _nextId: number = 1;

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

export class RingRTCType {
  private readonly callManager: CallManager;
  private _call: Call | null;
  private _groupCallByClientId: Map<GroupCallClientId, GroupCall>;
  private _peekRequests: Requests<PeekInfo>;

  // Set by UX
  handleOutgoingSignaling: ((remoteUserId: UserId, message: CallingMessage) => Promise<boolean>) | null = null;
  handleIncomingCall: ((call: Call) => Promise<CallSettings | null>) | null = null;
  handleAutoEndedIncomingCallRequest: ((remoteUserId: UserId, reason: CallEndedReason) => void) | null = null;
  handleLogMessage: ((level: CallLogLevel, fileName: string, line: number, message: string) => void) | null = null;
  handleSendHttpRequest: ((requestId: number, url: string, method: HttpMethod, headers: {[name: string]: string}, body: ArrayBuffer | undefined) => void) | null = null;
  handleSendCallMessage: ((recipientUuid: ArrayBuffer, message: ArrayBuffer) => void) | null = null;

  constructor() {
    this.callManager = new Native.CallManager() as CallManager;
    this._call = null;
    this._groupCallByClientId = new Map();
    this._peekRequests = new Requests<PeekInfo>();
    this.pollEvery(50);
  }

  private pollEvery(intervalMs: number): void {
    this.callManager.poll(this);
    setTimeout(() => {
      this.pollEvery(intervalMs);
    }, intervalMs);
  }

  // Called by UX
  startOutgoingCall(
    remoteUserId: UserId,
    isVideoCall: boolean,
    localDeviceId: DeviceId,
    settings: CallSettings
  ): Call {
    const callId = this.callManager.createOutgoingCall(remoteUserId, isVideoCall, localDeviceId);
    const isIncoming = false;
    const call = new Call(
      this.callManager,
      remoteUserId,
      callId,
      isIncoming,
      isVideoCall,
      settings,
      CallState.Prering
    );
    this._call = call;
    // We won't actually send anything until the remote side accepts.
    call.outgoingAudioEnabled = true;
    call.outgoingVideoEnabled = isVideoCall;
    return call;
  }

  // Called by Rust
  onStartOutgoingCall(remoteUserId: UserId, callId: CallId): void {
    const call = this._call;
    if (!call || call.remoteUserId !== remoteUserId || !call.settings) {
      return;
    }

    call.callId = callId;
    this.proceed(callId, call.settings);
  }

  // Called by Rust
  onStartIncomingCall(remoteUserId: UserId, callId: CallId, isVideoCall: boolean): void {
    // Temporary: Force hangup in all glare scenarios until handled gracefully.
    // In case of a glare loser, an incoming call will be generated right
    // after the outgoing call is ended. In that case, ignore it once.
    if (this._call && this._call.endedReason === CallEndedReason.Glare) {
      this._call.endedReason = undefined;
      this.ignore(callId);
      return;
    }

    const isIncoming = true;
    const call = new Call(
      this.callManager,
      remoteUserId,
      callId,
      isIncoming,
      isVideoCall,
      null,
      CallState.Prering
    );
    // Callback to UX not set
    const handleIncomingCall = this.handleIncomingCall;
    if (!handleIncomingCall) {
      call.ignore();
      return;
    }
    this._call = call;

    // tslint:disable no-floating-promises
    (async () => {
      const settings = await handleIncomingCall(call);
      if (!settings) {
        call.ignore();
        return;
      }
      call.settings = settings;
      this.proceed(callId, settings);
    })();
  }

  private proceed(callId: CallId, settings: CallSettings): void {
    silly_deadlock_protection(() => {
      this.callManager.proceed(
        callId,
        settings.iceServer.username || '',
        settings.iceServer.password || '',
        settings.iceServer.urls,
        settings.hideIp,
        settings.bandwidthMode,
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
  onCallEnded(remoteUserId: UserId, reason: CallEndedReason) {
    const call = this._call;

    // Temporary: Force hangup in all glare scenarios until handled gracefully.
    if (call && (reason === CallEndedReason.ReceivedOfferWithGlare || reason === CallEndedReason.Glare)) {
      call.hangup();
    }

    // If there is no call or the remoteUserId doesn't match that of
    // the current call, or if one of the "receive offer while alread
    // in a call" reasons are provided, don't end the current call, 
    // just update the call history.
    if (!call ||
        (call.remoteUserId !== remoteUserId) ||
        (reason === CallEndedReason.ReceivedOfferWhileActive) ||
        (reason === CallEndedReason.ReceivedOfferExpired)) {
      if (this.handleAutoEndedIncomingCallRequest) {
        this.handleAutoEndedIncomingCallRequest(remoteUserId, reason);
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

  renderVideoFrame(width: number, height: number, buffer: ArrayBuffer): void {
    const call = this._call;
    if (!call) {
      return;
    }

    if (!!this._call?.renderVideoFrame) {
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
    opaque: ArrayBuffer
  ): void {
    const message = new CallingMessage();
    message.offer = new OfferMessage();
    message.offer.callId = callId;
    message.offer.type = offerType;
    message.offer.opaque = opaque;
    this.sendSignaling(remoteUserId, remoteDeviceId, callId, broadcast, message);
  }

  // Called by Rust
  onSendAnswer(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    opaque: ArrayBuffer
  ): void {
    const message = new CallingMessage();
    message.answer = new AnswerMessage();
    message.answer.callId = callId;
    message.answer.opaque = opaque;
    this.sendSignaling(remoteUserId, remoteDeviceId, callId, broadcast, message);
  }

  // Called by Rust
  onSendIceCandidates(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    candidates: Array<ArrayBuffer>
  ): void {
    const message = new CallingMessage();
    message.iceCandidates = [];
    for (const candidate of candidates) {
      const copy = new IceCandidateMessage();
      copy.callId = callId;
      copy.opaque = candidate;
      message.iceCandidates.push(copy);
    }
    this.sendSignaling(remoteUserId, remoteDeviceId, callId, broadcast, message);
  }

  // Called by Rust
  onSendLegacyHangup(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    hangupType: HangupType,
    deviceId: DeviceId | null
  ): void {
    const message = new CallingMessage();
    message.legacyHangup = new HangupMessage();
    message.legacyHangup.callId = callId;
    message.legacyHangup.type = hangupType;
    message.legacyHangup.deviceId = deviceId || 0;
    this.sendSignaling(remoteUserId, remoteDeviceId, callId, broadcast, message);
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
    message.hangup = new HangupMessage();
    message.hangup.callId = callId;
    message.hangup.type = hangupType;
    message.hangup.deviceId = deviceId || 0;
    this.sendSignaling(remoteUserId, remoteDeviceId, callId, broadcast, message);
  }

  // Called by Rust
  onSendBusy(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean
  ): void {
    const message = new CallingMessage();
    message.busy = new BusyMessage();
    message.busy.callId = callId;
    this.sendSignaling(remoteUserId, remoteDeviceId, callId, broadcast, message);
  }

  private sendSignaling(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    message: CallingMessage
  ): void {
    message.supportsMultiRing = true;
    if (!broadcast) {
      message.destinationDeviceId = remoteDeviceId;
    }

    (async () => {
      if (this.handleOutgoingSignaling) {
        const signalingResult = await this.handleOutgoingSignaling(remoteUserId, message);
        if (signalingResult) {
          this.callManager.signalingMessageSent(callId);
        } else {
          this.callManager.signalingMessageSendFailed(callId);
        }
      } else {
        this.callManager.signalingMessageSendFailed(callId);
      }
    })();
  }

  receivedHttpResponse(requestId: number, status: number, body: ArrayBuffer) : void {
    silly_deadlock_protection(() => {
      try {
        this.callManager.receivedHttpResponse(requestId, status, body);
      } catch {
        // We may not have an active connection any more.
        // In which case it doesn't matter
      }
    });
  }

  httpRequestFailed(requestId: number, debugInfo: string | undefined) : void {
    silly_deadlock_protection(() => {
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
    groupId: ArrayBuffer,
    sfuUrl: string,
    observer: GroupCallObserver
  ): GroupCall | undefined {
    const groupCall = new GroupCall(this.callManager, groupId, sfuUrl, observer);

    this._groupCallByClientId.set(groupCall.clientId, groupCall);

    return groupCall;
  }

  // Called by UX
  // Returns a list of user IDs
  peekGroupCall(
    sfu_url: string,
    membership_proof: ArrayBuffer,
    group_members: Array<GroupMemberInfo>,

  ): Promise<PeekInfo> {
    let [requestId, promise] = this._peekRequests.add();
    // Response comes back via handlePeekResponse
    silly_deadlock_protection(() => {
      this.callManager.peekGroupCall(requestId, sfu_url, membership_proof, group_members);
    });
    return promise;
  }

  // Called by Rust
  requestMembershipProof(
    clientId: GroupCallClientId
  ): void {
    silly_deadlock_protection(() => {
      let groupCall = this._groupCallByClientId.get(clientId);
      if (!groupCall) {
        let error = new Error();
        this.onLogMessage(CallLogLevel.Error, 'Service.ts', 0, 'requestMembershipProof(): GroupCall not found in map!');
        return;
      }
      
      groupCall.requestMembershipProof();      
    });
  }

  // Called by Rust
  requestGroupMembers(
    clientId: GroupCallClientId
  ): void {
    silly_deadlock_protection(() => {
      let groupCall = this._groupCallByClientId.get(clientId);
      if (!groupCall) {
        let error = new Error();
        this.onLogMessage(CallLogLevel.Error, 'Service.ts', 0, 'requestGroupMembers(): GroupCall not found in map!');
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
    silly_deadlock_protection(() => {
      let groupCall = this._groupCallByClientId.get(clientId);
      if (!groupCall) {
        let error = new Error();
        this.onLogMessage(CallLogLevel.Error, 'Service.ts', 0, 'handleConnectionStateChanged(): GroupCall not found in map!');
        return;
      }
      
      groupCall.handleConnectionStateChanged(connectionState);
    });
  }

  // Called by Rust
  handleJoinStateChanged(
    clientId: GroupCallClientId,
    joinState: JoinState
  ): void {
    silly_deadlock_protection(() => {
      let groupCall = this._groupCallByClientId.get(clientId);
      if (!groupCall) {
        let error = new Error();
        this.onLogMessage(CallLogLevel.Error, 'Service.ts', 0, 'handleJoinStateChanged(): GroupCall not found in map!');
        return;
      }
      
      groupCall.handleJoinStateChanged(joinState);
    });
  }

  // Called by Rust
  handleRemoteDevicesChanged(
    clientId: GroupCallClientId,
    remoteDeviceStates: Array<RemoteDeviceState>
  ): void {
    silly_deadlock_protection(() => {
      let groupCall = this._groupCallByClientId.get(clientId);
      if (!groupCall) {
        let error = new Error();
        this.onLogMessage(CallLogLevel.Error, 'Service.ts', 0, 'handleRemoteDevicesChanged(): GroupCall not found in map!');
        return;
      }

      groupCall.handleRemoteDevicesChanged(remoteDeviceStates);
    });
  }

  // Called by Rust
  handlePeekChanged(
    clientId: GroupCallClientId,
    info: PeekInfo,
  ): void {
    silly_deadlock_protection(() => {
      let groupCall = this._groupCallByClientId.get(clientId);
      if (!groupCall) {
        let error = new Error();
        this.onLogMessage(CallLogLevel.Error, 'Service.ts', 0, 'handlePeekChanged(): GroupCall not found in map!');
        return;
      }
      
      groupCall.handlePeekChanged(info);
    });
  }

  // Called by Rust
  handlePeekResponse(
    request_id: number,
    info: PeekInfo,
  ): void {
    silly_deadlock_protection(() => {
      if (!this._peekRequests.resolve(request_id, info)) {
        this.onLogMessage(CallLogLevel.Warn, 'Service.ts', 0, `Invalid request ID for handlePeekResponse: ${request_id}`);
      }
    });
  }

  // Called by Rust
  handleEnded(
    clientId: GroupCallClientId,
    reason: GroupCallEndReason
  ): void {
    silly_deadlock_protection(() => {
      let groupCall = this._groupCallByClientId.get(clientId);
      if (!groupCall) {
        let error = new Error();
        this.onLogMessage(CallLogLevel.Error, 'Service.ts', 0, 'handleEnded(): GroupCall not found in map!');
        return;
      }
  
      this._groupCallByClientId.delete(clientId);
      
      groupCall.handleEnded(reason);
    });
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

  // Called by MessageReceiver
  // tslint:disable-next-line cyclomatic-complexity
  handleCallingMessage(
    remoteUserId: UserId,
    remoteUuid: ArrayBuffer | null,
    remoteDeviceId: DeviceId,
    localDeviceId: DeviceId,
    messageAgeSec: number,
    message: CallingMessage,
    senderIdentityKey: ArrayBuffer,
    receiverIdentityKey: ArrayBuffer,
  ): void {
    const remoteSupportsMultiRing = message.supportsMultiRing || false;

    if (message.destinationDeviceId && message.destinationDeviceId !== localDeviceId) {
      // Drop the message as it isn't for this device, handleIgnoredCall() is not needed.
      return;
    }

    if (message.offer && message.offer.callId) {
      const callId = message.offer.callId;
      const opaque = to_array_buffer(message.offer.opaque);

      // opaque is required. sdp is obsolete, but it might still come with opaque.
      if (!opaque) {
        // TODO: Remove once the proto is updated to only support opaque and require it.
        this.onLogMessage(CallLogLevel.Error, 'Service.ts', 0, 'handleCallingMessage(): opaque not received for offer, remote should update');
        return;
      }

      const offerType = message.offer.type || OfferType.AudioCall;
      this.callManager.receivedOffer(
        remoteUserId,
        remoteDeviceId,
        localDeviceId,
        messageAgeSec,
        callId,
        offerType,
        remoteSupportsMultiRing,
        opaque,
        senderIdentityKey,
        receiverIdentityKey
      );
    }
    if (message.answer && message.answer.callId) {
      const callId = message.answer.callId;
      const opaque = to_array_buffer(message.answer.opaque);

      // opaque is required. sdp is obsolete, but it might still come with opaque.
      if (!opaque) {
        // TODO: Remove once the proto is updated to only support opaque and require it.
        this.onLogMessage(CallLogLevel.Error, 'Service.ts', 0, 'handleCallingMessage(): opaque not received for answer, remote should update');
        return;
      }

      this.callManager.receivedAnswer(
        remoteUserId,
        remoteDeviceId,
        callId,
        remoteSupportsMultiRing,
        opaque,
        senderIdentityKey,
        receiverIdentityKey
      );
    }
    if (message.iceCandidates && message.iceCandidates.length > 0) {
      // We assume they all have the same .callId
      let callId = message.iceCandidates[0].callId;
      // We have to copy them to do the .toArrayBuffer() thing.
      const candidates: Array<ArrayBuffer> = [];
      for (const candidate of message.iceCandidates) {
        const copy = to_array_buffer(candidate.opaque);
        if (copy) {
          candidates.push(copy);
        } else {
          // TODO: Remove once the proto is updated to only support opaque and require it.
          this.onLogMessage(CallLogLevel.Error, 'Service.ts', 0, 'handleCallingMessage(): opaque not received for ice candidate, remote should update');
          continue;
        }
      }

      if (candidates.length == 0) {
        this.onLogMessage(CallLogLevel.Warn, 'Service.ts', 0, 'handleCallingMessage(): No ice candidates in ice message, remote should update');
        return;
      }

      this.callManager.receivedIceCandidates(
        remoteUserId,
        remoteDeviceId,
        callId,
        candidates,
      );
    }
    if (message.hangup && message.hangup.callId) {
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
    if (message.legacyHangup && message.legacyHangup.callId) {
      const callId = message.legacyHangup.callId;
      const hangupType = message.legacyHangup.type || HangupType.Normal;
      const hangupDeviceId = message.legacyHangup.deviceId || null;
      this.callManager.receivedHangup(
        remoteUserId,
        remoteDeviceId,
        callId,
        hangupType,
        hangupDeviceId
      );
    }
    if (message.busy && message.busy.callId) {
      const callId = message.busy.callId;
      this.callManager.receivedBusy(remoteUserId, remoteDeviceId, callId);
    }
    if (message.opaque) {
      if (remoteUuid == null) {
        this.onLogMessage(CallLogLevel.Error, 'Service.ts', 0, 'handleCallingMessage(): opaque message received without UUID!');
        return;
      }
      const data = to_array_buffer(message.opaque.data);
      if (data == undefined) {
        this.onLogMessage(CallLogLevel.Error, 'Service.ts', 0, 'handleCallingMessage(): opaque message received without data!');
        return;
      }
      this.callManager.receivedCallMessage(
        remoteUuid,
        remoteDeviceId,
        localDeviceId,
        data,
        messageAgeSec
      )
    }
  }

  // Called by Rust
  sendHttpRequest(requestId: number, url: string, method: HttpMethod, headers: {[name: string]: string}, body: ArrayBuffer | undefined) {
    if (this.handleSendHttpRequest) {
      this.handleSendHttpRequest(requestId, url, method, headers, body);
    } else {
      console.log("RingRTC.handleSendHttpRequest is not set!");
    }
  }

  // Called by Rust
  sendCallMessage(recipientUuid: ArrayBuffer, message: ArrayBuffer) : void {
    if (this.handleSendCallMessage) {
      this.handleSendCallMessage(recipientUuid, message);
    } else {
      console.log("RingRTC.handleSendCallMessage is not set!");
    }
  }

 // These are convenience methods.  One could use the Call class instead.
  get call(): Call | null {
    return this._call;
  }

  getCall(callId: CallId): Call | null {
    const { call } = this;

    if (call && call.callId.high === callId.high && call.callId.low === call.callId.low) {
      return call;
    }
    return null;
  }

  accept(callId: CallId, asVideoCall: boolean) {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.accept();
    call.outgoingAudioEnabled = true;
    call.outgoingVideoEnabled = asVideoCall;
  }

  decline(callId: CallId) {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.decline();
  }

  ignore(callId: CallId) {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.ignore();
  }

  hangup(callId: CallId) {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.hangup();
  }

  setOutgoingAudio(callId: CallId, enabled: boolean) {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.outgoingAudioEnabled = enabled;
  }

  setOutgoingVideo(callId: CallId, enabled: boolean) {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.outgoingVideoEnabled = enabled;
  }

  setVideoCapturer(callId: CallId, capturer: VideoCapturer | null) {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.videoCapturer = capturer;
  }

  setVideoRenderer(callId: CallId, renderer: VideoRenderer | null) {
    const call = this.getCall(callId);
    if (!call) {
      return;
    }

    call.videoRenderer = renderer;
  }

  getAudioInputs() : AudioDevice[] {
    return this.callManager.getAudioInputs();
  }

  setAudioInput(index: number) : void {
    this.callManager.setAudioInput(index);
  }

  getAudioOutputs() : AudioDevice[] {
    return this.callManager.getAudioOutputs();
  }

  setAudioOutput(index: number) : void {
    this.callManager.setAudioOutput(index);
  }
}

export interface CallSettings {
  iceServer: IceServer;
  hideIp: boolean;
  bandwidthMode: BandwidthMode;
}

interface IceServer {
  username?: string;
  password?: string;
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
  enableCaptureAndSend(call: Call): void;
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
  // We can have a null CallSettings while we're waiting for the UX to give us one.
  settings: CallSettings | null;
  private _state: CallState;
  private _outgoingAudioEnabled: boolean = false;
  private _outgoingVideoEnabled: boolean = false;
  private _remoteVideoEnabled: boolean = false;
  private _videoCapturer: VideoCapturer | null = null;
  private _videoRenderer: VideoRenderer | null = null;
  endedReason?: CallEndedReason;

  // These callbacks should be set by the UX code.
  handleStateChanged?: () => void;
  handleRemoteVideoEnabled?: () => void;

  // This callback should be set by the VideoCapturer,
  // But could also be set by the UX.
  renderVideoFrame?: (
    width: number,
    height: number,
    buffer: ArrayBuffer
  ) => void;

  constructor(
    callManager: CallManager,
    remoteUserId: UserId,
    callId: CallId,
    isIncoming: boolean,
    isVideoCall: boolean,
    settings: CallSettings | null,
    state: CallState
  ) {
    this._callManager = callManager;
    this._remoteUserId = remoteUserId;
    this.callId = callId;
    this._isIncoming = isIncoming;
    this._isVideoCall = isVideoCall;
    this.settings = settings;
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
    if (!!this.handleStateChanged) {
      this.handleStateChanged();
    }
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
    // This assumes we only have one active all.
    silly_deadlock_protection(() => {
      this._callManager.hangup();
    });
  }

  get outgoingAudioEnabled(): boolean {
    return this._outgoingAudioEnabled;
  }

  set outgoingAudioEnabled(enabled: boolean) {
    this._outgoingAudioEnabled = enabled;
    // This assumes we only have one active all.
    silly_deadlock_protection(() => {
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

  get remoteVideoEnabled(): boolean {
    return this._remoteVideoEnabled;
  }

  set remoteVideoEnabled(enabled: boolean) {
    this._remoteVideoEnabled = enabled;
    this.enableOrDisableRenderer();
  }

  // With this method, a Call is a VideoFrameSender
  sendVideoFrame(width: number, height: number, rgbaBuffer: ArrayBuffer): void {
    // This assumes we only have one active all.
    this._callManager.sendVideoFrame(width, height, rgbaBuffer);
  }

  // With this method, a Call is a VideoFrameSource
  receiveVideoFrame(buffer: ArrayBuffer): [number, number] | undefined {
    // This assumes we only have one active all.
    return this._callManager.receiveVideoFrame(buffer);
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
    silly_deadlock_protection(() => {
      try {
        this._callManager.setOutgoingVideoEnabled(enabled);
      } catch {
        // We may not have an active connection any more.
        // In which case it doesn't matter
      }
    });
  }

  updateBandwidthMode(bandwidthMode: BandwidthMode) {
    silly_deadlock_protection(() => {
      try {
        this._callManager.updateBandwidthMode(bandwidthMode);
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
  Connecting = 1,
  Connected = 2,
  Reconnecting = 3,
}

// Represents whether or not a user is joined to a group call and can exchange media.
export enum JoinState {
  NotJoined = 0,
  Joining = 1,
  Joined = 2,
}

// If not ended purposely by the user, gives the reason why a group call ended.
export enum GroupCallEndReason {
    // Normal events
    DeviceExplicitlyDisconnected = 0,
    ServerExplicitlyDisconnected = 1,

    // Things that can go wrong
    CallManagerIsBusy = 2,
    SfuClientFailedToJoin = 3,
    FailedToCreatePeerConnectionFactory = 4,
    FailedToGenerateCertificate = 5,
    FailedToCreatePeerConnection = 6,
    FailedToCreateDataChannel = 7,
    FailedToStartPeerConnection = 8,
    FailedToUpdatePeerConnection = 9,
    FailedToSetMaxSendBitrate = 10,
    IceFailedWhileConnecting = 11,
    IceFailedAfterConnected = 12,
    ServerChangedDemuxId = 13,
    HasMaxDevices = 14,
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
  audioMuted: boolean;
  videoMuted: boolean;

  constructor() {
    this.connectionState = ConnectionState.NotConnected;
    this.joinState = JoinState.NotJoined;
    // By default audio and video are muted.
    this.audioMuted = true;
    this.videoMuted = true;
  }
}

// All remote devices in a group call and their associated state.
export class RemoteDeviceState {
  demuxId: number;  // UInt32
  userId: ArrayBuffer;
  mediaKeysReceived: boolean;

  audioMuted: boolean | undefined;
  videoMuted: boolean | undefined;
  videoAspectRatio: number | undefined;  // Float
  addedTime: string | undefined;  // unix millis (to be converted to a numeric type)
  speakerTime: string | undefined;  // unix millis; 0 if they've never spoken (to be converted to a numeric type)

  constructor(
    demuxId: number,
    userId: ArrayBuffer,
    mediaKeysReceived: boolean
  ) {
    this.demuxId = demuxId;
    this.userId = userId;
    this.mediaKeysReceived = mediaKeysReceived
  }
}

// Used to communicate the group membership to RingRTC for a group call.
export class GroupMemberInfo {
  userId: ArrayBuffer;
  userIdCipherText: ArrayBuffer;

  constructor(
    userId: ArrayBuffer,
    userIdCipherText: ArrayBuffer
  ) {
    this.userId = userId;
    this.userIdCipherText = userIdCipherText;
  }
}

// Used for the application to communicate the actual resolutions of
// each device in a group call to RingRTC and the SFU.
export class VideoRequest {
  demuxId: number;  // UInt32
  width: number;  // UInt16
  height: number;  // UInt16
  framerate: number | undefined;  // UInt16

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

export interface GroupCallObserver {
  requestMembershipProof(groupCall: GroupCall): void;
  requestGroupMembers(groupCall: GroupCall): void;
  onLocalDeviceStateChanged(groupCall: GroupCall): void;
  onRemoteDeviceStatesChanged(groupCall: GroupCall): void;
  onPeekChanged(groupCall: GroupCall): void;
  onEnded(groupCall: GroupCall, reason: GroupCallEndReason): void;
}

export class GroupCall {
  private readonly _callManager: CallManager;
  private readonly _observer: GroupCallObserver;

  private readonly _clientId: GroupCallClientId;
  public get clientId(): GroupCallClientId {
    return this._clientId;
  }

  private _localDeviceState: LocalDeviceState;
  private _remoteDeviceStates: Array<RemoteDeviceState> | undefined;

  private _peekInfo: PeekInfo | undefined;  // uuid

  // Called by UI via RingRTC object
  constructor(
    callManager: CallManager,
    groupId: ArrayBuffer,
    sfuUrl: string,
    observer: GroupCallObserver
  ) {
    this._callManager = callManager;
    this._observer = observer;

    this._localDeviceState = new LocalDeviceState();

    this._clientId = this._callManager.createGroupCallClient(groupId, sfuUrl);
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
  setOutgoingVideoMuted(muted: boolean): void {
    this._localDeviceState.videoMuted = muted;
    this._callManager.setOutgoingVideoMuted(this._clientId, muted);
    this._observer.onLocalDeviceStateChanged(this);
  }

  // Called by UI
  resendMediaKeys(): void {
    this._callManager.resendMediaKeys(this._clientId);
  }

  // Called by UI
  setBandwidthMode(bandwidthMode: BandwidthMode): void {
    this._callManager.setBandwidthMode(this._clientId, bandwidthMode);
  }

  // Called by UI
  requestVideo(resolutions: Array<VideoRequest>): void {
    this._callManager.requestVideo(this._clientId, resolutions);
  }

  // Called by UI
  setGroupMembers(members: Array<GroupMemberInfo>): void {
    this._callManager.setGroupMembers(this._clientId, members);
  }

  // Called by UI
  setMembershipProof(proof: ArrayBuffer): void {
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
  handleJoinStateChanged(joinState: JoinState): void {
    this._localDeviceState.joinState = joinState;

    this._observer.onLocalDeviceStateChanged(this);
  }

  // Called by Rust via RingRTC object
  handleRemoteDevicesChanged(remoteDeviceStates: Array<RemoteDeviceState>): void {
    // We don't get aspect ratios from RingRTC, so make sure to copy them over.
    for (const noo of remoteDeviceStates) {
      const old = this._remoteDeviceStates?.find((old) => old.demuxId == noo.demuxId);
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
  sendVideoFrame(width: number, height: number, rgbaBuffer: ArrayBuffer): void {
    // This assumes we only have one active all.
    this._callManager.sendVideoFrame(width, height, rgbaBuffer);
  }

  // With this, a GroupCall can provide a VideoFrameSource for each remote device.
  getVideoSource(remoteDemuxId: number): GroupCallVideoFrameSource {
    return new GroupCallVideoFrameSource(this._callManager, this, remoteDemuxId);
  }

  // Called by the GroupCallVideoFrameSource when it receives a video frame.
  setRemoteAspectRatio(remoteDemuxId: number, aspectRatio: number) {
    const remoteDevice = this._remoteDeviceStates?.find((device) => device.demuxId == remoteDemuxId);
    if (!!remoteDevice && remoteDevice.videoAspectRatio != aspectRatio) {
      remoteDevice.videoAspectRatio = aspectRatio;
      this._observer.onRemoteDeviceStatesChanged(this);
    }
  }
}

// Implements VideoSource for use in CanvasVideoRenderer
class GroupCallVideoFrameSource {
  private readonly _callManager: CallManager;
  private readonly _groupCall: GroupCall;
  private readonly _remoteDemuxId: number;  // Uint32

  constructor(
    callManager: CallManager,
    groupCall: GroupCall,
    remoteDemuxId: number,  // Uint32
  ) {
    this._callManager = callManager;
    this._groupCall = groupCall;
    this._remoteDemuxId = remoteDemuxId;
  }
  
  receiveVideoFrame(buffer: ArrayBuffer): [number, number] | undefined {
    // This assumes we only have one active all.
    const frame = this._callManager.receiveGroupCallVideoFrame(this._groupCall.clientId, this._remoteDemuxId, buffer);
    if (!!frame) {
      const [ width, height ] = frame;
      this._groupCall.setRemoteAspectRatio(this._remoteDemuxId, width / height);
    }
    return frame;
  }
}

// When sending, we just set an ArrayBuffer.
// When receiving, we call .toArrayBuffer().
type ProtobufArrayBuffer = ArrayBuffer | {toArrayBuffer: () => ArrayBuffer};

function to_array_buffer(pbab: ProtobufArrayBuffer | undefined): ArrayBuffer | undefined {
  if (!pbab) {
    return pbab;
  }
  if (pbab instanceof ArrayBuffer) {
    return pbab;
  }
  return pbab.toArrayBuffer();
}

export type UserId = string;

export type DeviceId = number;

export type CallId = any;

export class CallingMessage {
  offer?: OfferMessage;
  answer?: AnswerMessage;
  iceCandidates?: Array<IceCandidateMessage>;
  legacyHangup?: HangupMessage;
  busy?: BusyMessage;
  hangup?: HangupMessage;
  opaque?: OpaqueMessage;
  supportsMultiRing?: boolean;
  destinationDeviceId?: DeviceId;
}

export class OfferMessage {
  callId?: CallId;
  type?: OfferType;
  opaque?: ProtobufArrayBuffer;
  sdp?: string;
}

export enum OfferType {
  AudioCall = 0,
  VideoCall = 1,
}

export class AnswerMessage {
  callId?: CallId;
  opaque?: ProtobufArrayBuffer;
  sdp?: string;
}

export class IceCandidateMessage {
  callId?: CallId;
  mid?: string;
  line?: number;
  opaque?: ProtobufArrayBuffer;
  sdp?: string;
}

export class BusyMessage {
  callId?: CallId;
}

export class HangupMessage {
  callId?: CallId;
  type?: HangupType;
  deviceId?: DeviceId;
}

export class OpaqueMessage {
  data?: ProtobufArrayBuffer;
}

export enum HangupType {
  Normal = 0,
  Accepted = 1,
  Declined = 2,
  Busy = 3,
  NeedPermission = 4,
}

export enum BandwidthMode {
  VeryLow = 0,
  Low = 1,
  Normal = 2,
}

export interface CallManager {
  createOutgoingCall(remoteUserId: UserId, isVideoCall: boolean, localDeviceId: DeviceId): CallId;
  proceed(
    callId: CallId,
    iceServerUsername: string,
    iceServerPassword: string,
    iceServerUrls: Array<string>,
    hideIp: boolean,
    bandwidthMode: BandwidthMode
  ): void;
  accept(callId: CallId): void;
  ignore(callId: CallId): void;
  hangup(): void;
  signalingMessageSent(callId: CallId): void;
  signalingMessageSendFailed(callId: CallId): void;
  setOutgoingAudioEnabled(enabled: boolean): void;
  setOutgoingVideoEnabled(enabled: boolean): void;
  updateBandwidthMode(bandwidthMode: BandwidthMode): void;
  sendVideoFrame(width: number, height: number, buffer: ArrayBuffer): void;
  receiveVideoFrame(buffer: ArrayBuffer): [number, number] | undefined;
  receivedOffer(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    messageAgeSec: number,
    callId: CallId,
    offerType: OfferType,
    localDeviceId: DeviceId,
    remoteSupportsMultiRing: boolean,
    opaque: ArrayBuffer,
    senderIdentityKey: ArrayBuffer,
    receiverIdentityKey: ArrayBuffer
  ): void;
  receivedAnswer(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    remoteSupportsMultiRing: boolean,
    opaque: ArrayBuffer,
    senderIdentityKey: ArrayBuffer,
    receiverIdentityKey: ArrayBuffer
  ): void;
  receivedIceCandidates(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    candidates: Array<ArrayBuffer>
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
    remoteUserId: ArrayBuffer,
    remoteDeviceId: DeviceId,
    localDeviceId: DeviceId,
    data: ArrayBuffer,
    messageAgeSec: number
  ): void;

  receivedHttpResponse(
    requestId: number,
    status: number,
    body: ArrayBuffer
  ): void;
  httpRequestFailed(requestId: number, debugInfo: string | undefined): void;

  // Group Calls

  createGroupCallClient(groupId: ArrayBuffer, sfuUrl: string) : GroupCallClientId;
  deleteGroupCallClient(clientId: GroupCallClientId): void;
  connect(clientId: GroupCallClientId): void;
  join(clientId: GroupCallClientId): void;
  leave(clientId: GroupCallClientId): void;
  disconnect(clientId: GroupCallClientId): void;
  setOutgoingAudioMuted(clientId: GroupCallClientId, muted: boolean): void;
  setOutgoingVideoMuted(clientId: GroupCallClientId, muted: boolean): void;
  resendMediaKeys(clientId: GroupCallClientId): void;
  setBandwidthMode(clientId: GroupCallClientId, bandwidthMode: BandwidthMode): void;
  requestVideo(clientId: GroupCallClientId, resolutions: Array<VideoRequest>): void;
  setGroupMembers(clientId: GroupCallClientId, members: Array<GroupMemberInfo>): void;
  setMembershipProof(clientId: GroupCallClientId, proof: ArrayBuffer): void;
  // Same as receiveVideoFrame, but with a specific GroupCallClientId and remoteDemuxId.
  receiveGroupCallVideoFrame(clientId: GroupCallClientId, remoteDemuxId: number, buffer: ArrayBuffer): [number, number] | undefined;
  // Response comes back via handlePeekResponse
  peekGroupCall(requestId: number, sfu_url: string, membership_proof: ArrayBuffer, group_members: Array<GroupMemberInfo>): Promise<PeekInfo>;

  getAudioInputs() : AudioDevice[];
  setAudioInput(index: number) : void;
  getAudioOutputs() : AudioDevice[];
  setAudioOutput(index: number): void;
  poll(callbacks: CallManagerCallbacks): void;
}

export interface CallManagerCallbacks {
  onStartOutgoingCall(remoteUserId: UserId, callId: CallId): void;
  onStartIncomingCall(remoteUserId: UserId, callId: CallId, isVideoCall: boolean): void;
  onCallState(remoteUserId: UserId, state: CallState): void;
  onCallEnded(remoteUserId: UserId, endedReason: CallEndedReason): void;
  onRemoteVideoEnabled(remoteUserId: UserId, enabled: boolean): void;
  onSendOffer(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    mediaType: number,
    opaque: ArrayBuffer
  ): void;
  onSendAnswer(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    opaque: ArrayBuffer
  ): void;
  onSendIceCandidates(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    candidates: Array<ArrayBuffer>
  ): void;
  onSendLegacyHangup(
    remoteUserId: UserId,
    remoteDeviceId: DeviceId,
    callId: CallId,
    broadcast: boolean,
    HangupType: HangupType,
    hangupDeviceId: DeviceId | null
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
    recipientUuid: ArrayBuffer,
    message: ArrayBuffer
  ) : void;
  sendHttpRequest(
    requestId: number,
    url: string,
    method: HttpMethod,
    headers: {[name: string]: string},
    body: ArrayBuffer | undefined
  ) : void;

  // Group Calls

  requestMembershipProof(
    clientId: GroupCallClientId
  ): void;
  requestGroupMembers(
    clientId: GroupCallClientId
  ): void;
  handleConnectionStateChanged(
    clientId: GroupCallClientId,
    connectionState: ConnectionState
  ): void;
  handleJoinStateChanged(
    clientId: GroupCallClientId,
    joinState: JoinState
  ): void;
  handleRemoteDevicesChanged(
    clientId: GroupCallClientId,
    remoteDeviceStates: Array<RemoteDeviceState>
  ): void;
  handlePeekChanged(
    clientId: GroupCallClientId,
    info: PeekInfo
  ): void;
  handlePeekResponse(
    request_id: number,
    info: PeekInfo
  ): void;
  handleEnded(
    clientId: GroupCallClientId,
    reason: GroupCallEndReason
  ): void;

  onLogMessage(
    level: number,
    fileName: string,
    line: number,
    message: string
  ): void;
}

export enum CallState {
  Prering = 'init',
  Ringing = 'ringing',
  Accepted = 'connected',
  Reconnecting = 'connecting',
  Ended = 'ended',
}

export enum CallEndedReason {
  LocalHangup = "LocalHangup",
  RemoteHangup = "RemoteHangup",
  RemoteHangupNeedPermission = "RemoteHangupNeedPermission",
  Declined = "Declined",
  Busy = "Busy",
  Glare = "Glare",
  ReceivedOfferExpired = "ReceivedOfferExpired",
  ReceivedOfferWhileActive = "ReceivedOfferWhileActive",
  ReceivedOfferWithGlare = "ReceivedOfferWithGlare",
  SignalingFailure = "SignalingFailure",
  ConnectionFailure = "ConnectionFailure",
  InternalFailure = "InternalFailure",
  Timeout = "Timeout",
  AcceptedOnAnotherDevice = "AcceptedOnAnotherDevice",
  DeclinedOnAnotherDevice = "DeclinedOnAnotherDevice",
  BusyOnAnotherDevice = "BusyOnAnotherDevice",
  CallerIsNotMultiring = "CallerIsNotMultiring",
}

export enum CallLogLevel {
  Off,
  Error,
  Warn,
  Info,
  Debug,
  Trace,
}

function silly_deadlock_protection(f: () => void) {
    // tslint:disable no-floating-promises
    (async () => {
      // This is a silly way of preventing a deadlock.
      // tslint:disable-next-line await-promise
      await 0;

      f();
    })();
}
