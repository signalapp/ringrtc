//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

/* eslint-disable no-console, @typescript-eslint/require-await, @typescript-eslint/no-unused-vars */

import {
  DataMode,
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
    call.handleStateChanged = () => {
      log('handleCallStateChanged');
      log(`call.state === ${call.state}`);
      if (call.state === CallState.Ended) {
        log(`call.endedReason === ${call.endedReason}`);
        this._call = undefined;
      }
    };

    // eslint-disable-next-line no-param-reassign
    call.handleRemoteAudioEnabled = () => {
      log('handleRemoteAudioEnabled');
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
    log(`handleOutgoingSignaling remoteUserId: ${remoteUserId}`);

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

  private handleAutoEndedIncomingCallRequest(
    callId: CallId,
    remoteUserId: UserId,
    reason: CallEndedReason,
    ageInSeconds: number,
    wasVideoCall: boolean,
    receivedAtCounter: number | undefined
  ) {
    log('handleAutoEndedIncomingCallRequest');
  }

  static handleLogMessage(
    level: CallLogLevel,
    fileName: string,
    line: number,
    message: string
  ): void {
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

  private handleSendHttpRequest(
    requestId: number,
    url: string,
    method: HttpMethod,
    headers: { [name: string]: string },
    body: Uint8Array | undefined
  ) {
    log('handleSendHttpRequest');
  }

  private handleSendCallMessage(
    recipient: Uint8Array,
    data: Uint8Array,
    urgency: CallMessageUrgency
  ): boolean {
    log('handleSendCallMessage');

    return true;
  }

  private handleSendCallMessageToGroup(
    groupIdBytes: Uint8Array,
    data: Buffer,
    urgency: CallMessageUrgency
  ): void {
    log('handleSendCallMessageToGroup');
  }

  private handleGroupCallRingUpdate(
    groupIdBytes: Uint8Array,
    ringId: bigint,
    ringerBytes: Buffer,
    update: RingUpdate
  ): void {
    log('handleGroupCallRingUpdate');
  }

  private handleRtcStatsReport(reportJson: string): void {
    log('handleRtcStatsReport');
  }

  ////////////////////////////////////////////////////////////////////////////////
  // Support

  private async getCallSettings(isIncoming: boolean): Promise<CallSettings> {
    if (isIncoming) {
      log(
        `getCallSettings delayed by ${this._delayIncomingCallSettingsRequest}ms`
      );
      await sleep(this._delayIncomingCallSettingsRequest);
    } else {
      log(
        `getCallSettings delayed by ${this._delayOutgoingCallSettingsRequest}ms`
      );
      await sleep(this._delayOutgoingCallSettingsRequest);
    }

    return {
      iceServers: [
        {
          hostname: '',
          username: 'name',
          password: 'pass',
          urls: ['stun:turn3.voip.signal.org'],
        },
        {
          hostname: 'example.org',
          username: 'name',
          password: 'pass',
          urls: ['stun:123.123.123.1'],
        },
        {
          urls: ['stun:127.0.0.1'],
        },
      ],
      hideIp: false,
      dataMode: DataMode.Normal,
    };
  }

  ////////////////////////////////////////////////////////////////////////////////
  // Actions

  initialize(): void {
    log('initialize');

    RingRTC.setConfig({
      field_trials: undefined,
    });

    RingRTC.handleOutgoingSignaling = this.handleOutgoingSignaling.bind(this);
    RingRTC.handleIncomingCall = this.handleIncomingCall.bind(this);
    RingRTC.handleStartCall = this.handleStartCall.bind(this);
    RingRTC.handleAutoEndedIncomingCallRequest =
      this.handleAutoEndedIncomingCallRequest.bind(this);
    RingRTC.handleLogMessage = CallingClass.handleLogMessage;
    RingRTC.handleSendHttpRequest = this.handleSendHttpRequest.bind(this);
    RingRTC.handleSendCallMessage = this.handleSendCallMessage.bind(this);
    RingRTC.handleSendCallMessageToGroup =
      this.handleSendCallMessageToGroup.bind(this);
    RingRTC.handleGroupCallRingUpdate =
      this.handleGroupCallRingUpdate.bind(this);
    RingRTC.handleRtcStatsReport = this.handleRtcStatsReport.bind(this);
    RingRTC.setSelfUuid(Buffer.from(uuidToBytes(this._id)));
  }

  static initializeLoggingOnly(): void {
    RingRTC.handleLogMessage = CallingClass.handleLogMessage;
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

    log(`Outgoing callId ${Long.fromValue(call.callId)}`);

    call.setOutgoingAudioMuted(false);

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
