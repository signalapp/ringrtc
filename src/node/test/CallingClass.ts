//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import {
  BandwidthMode,
  Call,
  CallEndedReason,
  CallId,
  CallingMessage,
  CallLogLevel,
  CallMessageUrgency,
  CallSettings,
  CallState,
  HttpMethod,
  RingUpdate,
  UserId,
} from '../ringrtc/Service';
import { RingRTC } from '../index';
import Long from 'long';
import { log, sleep, uuidToBytes } from './Utils';

// This class mimics the Desktop Client CallingClass in ts/services/calling.ts to facilitate testing
export class CallingClass {
  private _name: string;
  private _id: string;
  private _localDeviceId: number;
  private _call: Call | undefined;
  private _delayIncomingCallSettingsRequest: number;
  private _delayOutgoingCallSettingsRequest: number;

  set delayIncomingCallSettingsRequest(value: number) {
    this._delayIncomingCallSettingsRequest = value;
  }
  set delayOutgoingCallSettingsRequest(value: number) {
    this._delayIncomingCallSettingsRequest = value;
  }

  constructor(name: string, id: string) {
    this._name = name;
    this._id = id;

    this._localDeviceId = 1;

    this._delayIncomingCallSettingsRequest = 0;
    this._delayOutgoingCallSettingsRequest = 0;
  }

  private setupCallCallbacks(call: Call) {
    // eslint-disable-next-line no-param-reassign
    call.handleStateChanged = async () => {
      log('handleCallStateChanged');
      log(`call.state === ${call.state}`);
      if (call.state === CallState.Ended) {
        log(`call.endedReason === ${call.endedReason}`);
        this._call = undefined;
      }
    };

    // eslint-disable-next-line no-param-reassign
    call.handleRemoteVideoEnabled = () => {
      log('handleRemoteVideoEnabled');
    };

    // eslint-disable-next-line no-param-reassign
    call.handleRemoteSharingScreen = () => {
      log('handleRemoteSharingScreen');
    };
  }

  ////////////////////////////////////////////////////////////////////////////////
  // Callbacks

  private async handleOutgoingSignaling(
    remoteUserId: UserId,
    message: CallingMessage,
    urgency?: CallMessageUrgency
  ): Promise<boolean> {
    log('handleOutgoingSignaling remoteUserId: ' + remoteUserId);

    return true;
  }

  private async handleIncomingCall(call: Call): Promise<boolean> {
    log('handleIncomingCall');

    this._call = call;

    this.setupCallCallbacks(call);

    return true;
  }

  private async handleStartCall(call: Call): Promise<boolean> {
    const callSettings = await this.getCallSettings(call.isIncoming);

    RingRTC.proceed(call.callId, callSettings);

    return true;
  }

  private async handleAutoEndedIncomingCallRequest(
    callId: CallId,
    remoteUserId: UserId,
    reason: CallEndedReason,
    ageInSeconds: number,
    wasVideoCall: boolean,
    receivedAtCounter: number | undefined
  ) {
    log('handleAutoEndedIncomingCallRequest');
  }

  private async handleLogMessage(
    level: CallLogLevel,
    fileName: string,
    line: number,
    message: string
  ) {
    switch (level) {
      case CallLogLevel.Info:
        // FgGray
        console.log(`\x1b[90m${fileName}:${line} ${message}\x1b[0m`);
        break;
      case CallLogLevel.Warn:
        // FgYellow
        console.warn(`\x1b[33m${fileName}:${line} ${message}\x1b[0m`);
        break;
      case CallLogLevel.Error:
        // FgRed
        console.error(`\x1b[31m${fileName}:${line} ${message}\x1b[0m`);
        break;
      default:
        break;
    }
  }

  private async handleSendHttpRequest(
    requestId: number,
    url: string,
    method: HttpMethod,
    headers: { [name: string]: string },
    body: Uint8Array | undefined
  ) {
    log('handleSendHttpRequest');
  }

  private async handleSendCallMessage(
    recipient: Uint8Array,
    data: Uint8Array,
    urgency: CallMessageUrgency
  ): Promise<boolean> {
    log('handleSendCallMessage');

    return true;
  }

  private async handleSendCallMessageToGroup(
    groupIdBytes: Buffer,
    data: Buffer,
    urgency: CallMessageUrgency
  ): Promise<void> {
    log('handleSendCallMessageToGroup');
  }

  private async handleGroupCallRingUpdate(
    groupIdBytes: Buffer,
    ringId: bigint,
    ringerBytes: Buffer,
    update: RingUpdate
  ): Promise<void> {
    log('handleGroupCallRingUpdate');
  }

  ////////////////////////////////////////////////////////////////////////////////
  // Support

  private async getCallSettings(isIncoming: boolean): Promise<CallSettings> {
    if (isIncoming) {
      log(
        'getCallSettings delayed by ' +
          this._delayIncomingCallSettingsRequest.toString() +
          'ms'
      );
      await sleep(this._delayIncomingCallSettingsRequest);
    } else {
      log(
        'getCallSettings delayed by ' +
          this._delayOutgoingCallSettingsRequest.toString() +
          'ms'
      );
      await sleep(this._delayOutgoingCallSettingsRequest);
    }

    return {
      iceServer: {
        urls: ['stun:turn3.voip.signal.org'],
      },
      hideIp: false,
      bandwidthMode: BandwidthMode.Normal,
    };
  }

  ////////////////////////////////////////////////////////////////////////////////
  // Actions

  initialize() {
    log('initialize');

    RingRTC.setConfig({
      use_new_audio_device_module: true,
      field_trials: undefined,
    });

    RingRTC.handleOutgoingSignaling = this.handleOutgoingSignaling.bind(this);
    RingRTC.handleIncomingCall = this.handleIncomingCall.bind(this);
    RingRTC.handleStartCall = this.handleStartCall.bind(this);
    RingRTC.handleAutoEndedIncomingCallRequest =
      this.handleAutoEndedIncomingCallRequest.bind(this);
    RingRTC.handleLogMessage = this.handleLogMessage.bind(this);
    RingRTC.handleSendHttpRequest = this.handleSendHttpRequest.bind(this);
    RingRTC.handleSendCallMessage = this.handleSendCallMessage.bind(this);
    RingRTC.handleSendCallMessageToGroup =
      this.handleSendCallMessageToGroup.bind(this);
    RingRTC.handleGroupCallRingUpdate =
      this.handleGroupCallRingUpdate.bind(this);

    RingRTC.setSelfUuid(Buffer.from(uuidToBytes(this._id)));
  }

  async startOutgoingDirectCall(remoteUserId: string): Promise<void> {
    log('startOutgoingDirectCall');

    if (RingRTC.call && RingRTC.call.state !== CallState.Ended) {
      log('Call already in progress, new call not allowed.');
      return;
    }

    const call = RingRTC.startOutgoingCall(
      remoteUserId,
      false,
      this._localDeviceId
    );

    log('Outgoing callId ' + Long.fromValue(call.callId).toString());

    RingRTC.setOutgoingAudio(call.callId, true);

    this._call = call;

    this.setupCallCallbacks(call);
  }

  hangup(): boolean {
    log('hangup');

    if (this._call) {
      RingRTC.hangup(this._call.callId);
      return true;
    }

    return false;
  }
}
