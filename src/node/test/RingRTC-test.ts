//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

/* eslint-disable @typescript-eslint/no-non-null-assertion */

import { assert, expect, use } from 'chai';
import chaiAsPromised from 'chai-as-promised';
import { createHash, randomBytes } from 'crypto';
import {
  CallEndedReason,
  CallLinkRestrictions,
  CallLinkRootKey,
  CallState,
  CallingMessage,
  GroupCall,
  GroupCallEndReason,
  GroupCallKind,
  GroupMemberInfo,
  HttpMethod,
  OfferType,
  PeekStatusCodes,
  Reaction,
  SpeechEvent,
  RingRTC,
  callIdFromEra,
  callIdFromRingId,
} from '../index';
import Long from 'long';
import { should } from 'chai';
import sinon, { SinonSpy } from 'sinon';
import sinonChai from 'sinon-chai';
import { CallingClass } from './CallingClass';
import { countDownLatch, log, sleep, uuidToBytes } from './Utils';

use(chaiAsPromised);
should();
use(sinonChai);

function generateOfferCallingMessage(callId: Long): CallingMessage {
  // Audio-only hex based SDP generated from a direct client call
  const audioOnlySdp = Buffer.from(
    '22560a204b18bc751315cb718c643db7b3a65aaabe826c7094932afaf5aebc86d36bb6491204484b6b481a18524b3041496f63334245514e5670424b57786f38787051712204082e1034220408281034220208082880897a',
    'hex'
  );
  return {
    offer: {
      callId: callId,
      opaque: audioOnlySdp,
      type: OfferType.AudioCall,
    },
  };
}

function sha256Hex(input: string): string {
  return createHash('sha256').update(input).digest('hex');
}

describe('RingRTC', () => {
  const identity_key_length = 31;
  const user1_name = 'user1';
  const user1_id = '11';
  const user1_device_id = 11;
  const user1_identity_key = randomBytes(identity_key_length);

  const user2_id = '22';
  const user2_device_id = 22;
  const user2_identity_key = randomBytes(identity_key_length);

  let handleOutgoingSignalingSpy: SinonSpy;
  let handleIncomingCallSpy: SinonSpy;
  let handleAutoEndedIncomingCallRequestSpy: SinonSpy;

  it('reports an age for expired offers', async () => {
    const offer: CallingMessage = {
      offer: {
        callId: { high: 0, low: 123, unsigned: true },
        type: OfferType.AudioCall,
        opaque: Buffer.from([]),
      },
    };
    const age = 60 * 60;
    try {
      const { reason, ageSec: reportedAge } = await new Promise<{
        reason: CallEndedReason;
        ageSec: number;
      }>((resolve, _reject) => {
        /* eslint-disable @typescript-eslint/no-shadow */
        RingRTC.handleAutoEndedIncomingCallRequest = (
          _callId,
          _remoteUserId,
          reason,
          ageSec
        ) => {
          resolve({ reason, ageSec });
        };
        /* eslint-enable @typescript-eslint/no-shadow */
        RingRTC.handleCallingMessage(offer, {
          remoteUserId: 'remote',
          remoteDeviceId: 4,
          localDeviceId: 2,
          ageSec: age,
          receivedAtCounter: 1,
          receivedAtDate: 100,
          senderIdentityKey: Buffer.from([]),
          receiverIdentityKey: Buffer.from([]),
        });
      });
      assert.equal(reason, CallEndedReason.ReceivedOfferExpired);
      assert.equal(reportedAge, age);
    } finally {
      RingRTC.handleAutoEndedIncomingCallRequest = null;
    }
  });

  it('reports 0 as the age of other auto-ended offers', async () => {
    const offer: CallingMessage = {
      offer: {
        callId: { high: 0, low: 123, unsigned: true },
        type: OfferType.AudioCall,
        opaque: Buffer.from([]),
      },
    };
    try {
      const { reason, ageSec: reportedAge } = await new Promise<{
        reason: CallEndedReason;
        ageSec: number;
      }>((resolve, _reject) => {
        /* eslint-disable @typescript-eslint/no-shadow */
        RingRTC.handleAutoEndedIncomingCallRequest = (
          _callId,
          _remoteUserId,
          reason,
          ageSec
        ) => {
          resolve({ reason, ageSec });
        };
        /* eslint-enable @typescript-eslint/no-shadow */
        RingRTC.handleCallingMessage(offer, {
          remoteUserId: 'remote',
          remoteDeviceId: 4,
          localDeviceId: 2,
          ageSec: 10,
          receivedAtCounter: 2,
          receivedAtDate: 200,
          senderIdentityKey: Buffer.from([]),
          receiverIdentityKey: Buffer.from([]),
        });
      });
      assert.equal(reason, CallEndedReason.Declined); // because we didn't set handleIncomingCall.
      assert.equal(reportedAge, 0);
    } finally {
      RingRTC.handleAutoEndedIncomingCallRequest = null;
    }
  });

  function initializeSpies() {
    handleAutoEndedIncomingCallRequestSpy = sinon.spy(
      RingRTC,
      'handleAutoEndedIncomingCallRequest'
    );
    handleIncomingCallSpy = sinon.spy(RingRTC, 'handleIncomingCall');
    handleOutgoingSignalingSpy = sinon.spy(RingRTC, 'handleOutgoingSignaling');
  }

  it('can initialize RingRTC', () => {
    assert.isNotNull(RingRTC, "RingRTC didn't initialize!");
  });

  it('can establish outgoing call', async () => {
    const calling = new CallingClass(user1_name, user1_id);
    calling.initialize();
    initializeSpies();

    await calling.startOutgoingDirectCall(user2_id);

    await sleep(1000);

    // An offer and at least one ICE message should have been sent.
    expect(handleOutgoingSignalingSpy.callCount).to.be.gt(1);

    await sleep(2000);

    // Cleanup.
    const handleStateChangedSpy = sinon.spy(
      RingRTC.call!,
      'handleStateChanged'
    );
    expect(calling.hangup()).to.be.true;
    await sleep(500);
    handleStateChangedSpy.should.have.been.calledOnce;
    expect(calling.hangup()).to.be.false;
    await sleep(100);
  });

  it('can establish incoming call', async () => {
    const calling = new CallingClass(user1_name, user1_id);
    calling.initialize();
    initializeSpies();

    // Generate incoming calling message
    const callId = new Long(1, 1, true);
    const offerCallingMessage = generateOfferCallingMessage(callId);

    RingRTC.handleCallingMessage(offerCallingMessage, {
      remoteUserId: user2_id,
      remoteUuid: Buffer.from(uuidToBytes(user2_id)),
      remoteDeviceId: user2_device_id,
      localDeviceId: user1_device_id,
      ageSec: 1,
      receivedAtCounter: 10,
      receivedAtDate: 1000,
      senderIdentityKey: user2_identity_key,
      receiverIdentityKey: user1_identity_key,
    });

    await sleep(1000);
    handleIncomingCallSpy.should.have.been.calledOnce;
    assert.equal(CallState.Prering, RingRTC.call!.state);

    // Hangup call
    expect(calling.hangup()).to.be.true;
    await sleep(500);

    // Validate hangup related callbacks and call state
    handleAutoEndedIncomingCallRequestSpy.should.have.been.calledOnce;
    expect(handleOutgoingSignalingSpy.callCount).to.be.gt(1);
    assert.equal(CallState.Ended, RingRTC.call!.state);
  });

  it('outgoing call wins glare when incoming call id is lower', async () => {
    const calling = new CallingClass(user1_name, user1_id);
    calling.initialize();
    initializeSpies();

    await runGlareScenario(calling, true, 0, 0);
  });

  it('outgoing call wins glare when incoming call id is lower even when outgoing call settings are delayed', async () => {
    const calling = new CallingClass(user1_name, user1_id);
    calling.initialize();
    initializeSpies();

    await runGlareScenario(calling, true, 0, 1000);
  });

  it('outgoing call loses glare when incoming call id is higher even when outgoing call settings are delayed', async () => {
    const calling = new CallingClass(user1_name, user1_id);
    calling.initialize();
    initializeSpies();

    await runGlareScenario(calling, false, 0, 1000);
  });

  it('outgoing call loses glare when incoming call id is higher', async () => {
    const calling = new CallingClass(user1_name, user1_id);
    calling.initialize();
    initializeSpies();

    await runGlareScenario(calling, false, 0, 0);
  });

  async function runGlareScenario(
    calling: CallingClass,
    outgoingWinner: boolean,
    delayIncomingCallSettings: number,
    delayOutgoingCallSettings: number
  ) {
    calling.delayOutgoingCallSettingsRequest = delayOutgoingCallSettings;
    calling.delayIncomingCallSettingsRequest = delayIncomingCallSettings;

    const outgoingCallLatch = countDownLatch(1);
    calling
      .startOutgoingDirectCall(user2_id)
      .then(_result => {
        log('Outgoing call succeeded as expected');
        outgoingCallLatch.countDown();
      })
      .catch(e => {
        assert.fail(`Outgoing call should not have failed: ${e}`);
      });

    await outgoingCallLatch.finished;

    const outgoingCallId = Long.fromValue(RingRTC.call!.callId);

    // Generate a call id based on the desired glare winner
    const incomingCallId = outgoingCallId.unsigned
      ? new Long(
          outgoingWinner ? outgoingCallId.low - 1 : outgoingCallId.low + 1,
          outgoingCallId.high,
          outgoingCallId.unsigned
        )
      : new Long(
          outgoingWinner ? outgoingCallId.low + 1 : outgoingCallId.low - 1,
          outgoingCallId.high,
          outgoingCallId.unsigned
        );

    // Generate incoming calling message
    const offerCallingMessage = generateOfferCallingMessage(incomingCallId);

    // Initiate an incoming call
    RingRTC.handleCallingMessage(offerCallingMessage, {
      remoteUserId: user2_id,
      remoteUuid: Buffer.from(uuidToBytes(user2_id)),
      remoteDeviceId: user2_device_id,
      localDeviceId: user1_device_id,
      ageSec: 1,
      receivedAtCounter: 10,
      receivedAtDate: 1000,
      senderIdentityKey: user2_identity_key,
      receiverIdentityKey: user1_identity_key,
    });

    await sleep(1000);

    if (outgoingWinner) {
      assert.isTrue(outgoingCallId.eq(Long.fromValue(RingRTC.call!.callId)));
    } else {
      assert.isTrue(incomingCallId.eq(Long.fromValue(RingRTC.call!.callId)));
    }

    // Cleanup.
    expect(calling.hangup()).to.be.true;
    await sleep(500);
    assert.equal(CallState.Ended, RingRTC.call!.state);
  }

  it('converts eras to call IDs', () => {
    const fromHex = callIdFromEra('8877665544332211');
    assert.isTrue(
      Long.fromValue(fromHex).eq(Long.fromString('8877665544332211', true, 16))
    );

    const fromUnusualEra = callIdFromEra('mesozoic');
    assert.isFalse(Long.fromValue(fromUnusualEra).eq(Long.fromValue(fromHex)));
    assert.isFalse(Long.fromValue(fromUnusualEra).isZero());
  });

  it('converts ring IDs to call IDs', () => {
    function testConversion(ringIdAsString: string) {
      const ringId = BigInt(ringIdAsString);
      const callId = callIdFromRingId(ringId);
      const expectedCallId = Long.fromValue(ringIdAsString).toUnsigned();
      assert.isTrue(
        Long.fromValue(callId).eq(expectedCallId),
        `${ringId} was converted to ${callId}, should be ${expectedCallId}`
      );
    }
    testConversion('0');
    testConversion('1');
    testConversion('-1');
    testConversion(Long.MAX_VALUE.toString());
    testConversion((-Long.MAX_VALUE).toString());
    testConversion(Long.MIN_VALUE.toString());
  });

  it('can peek with pending clients', async () => {
    const requestIdPromise = new Promise<number>((resolve, reject) => {
      RingRTC.handleSendHttpRequest = (
        requestId,
        url,
        method,
        _headers,
        _body
      ) => {
        try {
          assert.isTrue(url.startsWith('sfu.example'));
          assert.equal(method, HttpMethod.Get);
          resolve(requestId);
        } catch (e) {
          reject(e);
        }
      };
    });
    const peekResponse = RingRTC.peekGroupCall(
      'sfu.example',
      Buffer.of(1, 2, 3),
      [
        new GroupMemberInfo(
          Buffer.of(0x11, 0x11, 0x11, 0x11),
          Buffer.from('11', 'utf-8')
        ),
        new GroupMemberInfo(
          Buffer.of(0x22, 0x22, 0x22, 0x22),
          Buffer.from('22', 'utf-8')
        ),
        new GroupMemberInfo(
          Buffer.of(0x33, 0x33, 0x33, 0x33),
          Buffer.from('33', 'utf-8')
        ),
      ]
    );
    const requestId = await requestIdPromise;
    RingRTC.receivedHttpResponse(
      requestId,
      200,
      Buffer.from(
        `{
        "conferenceId":"mesozoic",
        "maxDevices":20,
        "creator":"${sha256Hex('11')}",
        "participants":[
          {"opaqueUserId":"${sha256Hex('11')}","demuxId":${32 * 1}},
          {"opaqueUserId":"${sha256Hex('22')}","demuxId":${32 * 2}},
          {"opaqueUserId":"${sha256Hex('44')}","demuxId":${32 * 3}}
        ],
        "pendingClients":[
          {"opaqueUserId":"${sha256Hex('33')}","demuxId":${32 * 4}},
          {"opaqueUserId":"${sha256Hex('33')}","demuxId":${32 * 5}},
          {"opaqueUserId":"${sha256Hex('44')}","demuxId":${32 * 6}},
          {"demuxId":${32 * 7}}
        ]
      }`,
        'utf-8'
      )
    );
    const peekInfo = await peekResponse;
    assert.equal(peekInfo.eraId, 'mesozoic');
    assert.equal(peekInfo.deviceCountIncludingPendingDevices, 7);
    assert.equal(peekInfo.deviceCountExcludingPendingDevices, 3);
    assert.equal(peekInfo.maxDevices, 20);
    assert.isTrue(peekInfo.creator?.equals(Buffer.of(0x11, 0x11, 0x11, 0x11)));
    assert.deepEqual(peekInfo.devices, [
      { demuxId: 32 * 1, userId: Buffer.of(0x11, 0x11, 0x11, 0x11) },
      { demuxId: 32 * 2, userId: Buffer.of(0x22, 0x22, 0x22, 0x22) },
      { demuxId: 32 * 3 },
    ]);
    assert.deepEqual(peekInfo.pendingUsers, [
      Buffer.of(0x33, 0x33, 0x33, 0x33),
    ]);
  });

  describe('CallLinkRootKey', () => {
    const EXAMPLE_PUBLIC_ENDORSEMENT_KEY = Buffer.of(
      0,
      86,
      35,
      236,
      48,
      147,
      33,
      66,
      168,
      208,
      215,
      207,
      250,
      177,
      151,
      88,
      0,
      158,
      219,
      130,
      38,
      212,
      159,
      171,
      211,
      130,
      220,
      217,
      29,
      133,
      9,
      96,
      97
    );
    const EXAMPLE_CALL_LINK_ROOT_KEY = CallLinkRootKey.parse(
      'bcdf-ghkm-npqr-stxz-bcdf-ghkm-npqr-stxz'
    );
    const EXPIRATION_EPOCH_SECONDS = 4133980800; // 2101-01-01
    const EXAMPLE_STATE_JSON = `{"restrictions": "none","name":"","revoked":false,"expiration":${EXPIRATION_EPOCH_SECONDS}}`;
    const EXAMPLE_EMPTY_RESPONSE = '{}';

    it('has accessors', () => {
      const anotherKey = CallLinkRootKey.generate();
      assert.isFalse(EXAMPLE_CALL_LINK_ROOT_KEY.bytes.equals(anotherKey.bytes));

      assert.isTrue(
        EXAMPLE_CALL_LINK_ROOT_KEY.deriveRoomId().equals(
          EXAMPLE_CALL_LINK_ROOT_KEY.deriveRoomId()
        )
      );
      assert.isFalse(
        EXAMPLE_CALL_LINK_ROOT_KEY.deriveRoomId().equals(
          anotherKey.deriveRoomId()
        )
      );
    });

    it('can be formatted', () => {
      assert.equal(
        `${EXAMPLE_CALL_LINK_ROOT_KEY}`,
        'bcdf-ghkm-npqr-stxz-bcdf-ghkm-npqr-stxz'
      );
    });

    it('can create call links', async () => {
      const requestIdPromise = new Promise<number>((resolve, reject) => {
        RingRTC.handleSendHttpRequest = (
          requestId,
          url,
          method,
          _headers,
          _body
        ) => {
          try {
            assert.isTrue(url.startsWith('sfu.example'));
            assert.equal(method, HttpMethod.Put);
            resolve(requestId);
          } catch (e) {
            reject(e);
          }
        };
      });
      const callLinkResponse = RingRTC.createCallLink(
        'sfu.example',
        Buffer.of(1, 2, 3),
        EXAMPLE_CALL_LINK_ROOT_KEY,
        CallLinkRootKey.generateAdminPassKey(),
        Buffer.of(4, 5, 6),
        CallLinkRestrictions.None
      );
      const requestId = await requestIdPromise;
      RingRTC.receivedHttpResponse(
        requestId,
        200,
        Buffer.from(EXAMPLE_STATE_JSON)
      );
      const state = await callLinkResponse;
      if (state.success) {
        assert.deepEqual(
          state.value.expiration,
          new Date(EXPIRATION_EPOCH_SECONDS * 1000)
        );
      } else {
        assert.fail('should have succeeded');
      }
    });

    it('can handle failure when creating call links', async () => {
      const requestIdPromise = new Promise<number>((resolve, reject) => {
        RingRTC.handleSendHttpRequest = (
          requestId,
          url,
          method,
          _headers,
          _body
        ) => {
          try {
            assert.isTrue(url.startsWith('sfu.example'));
            assert.equal(method, HttpMethod.Put);
            resolve(requestId);
          } catch (e) {
            reject(e);
          }
        };
      });
      const callLinkResponse = RingRTC.createCallLink(
        'sfu.example',
        Buffer.of(1, 2, 3),
        EXAMPLE_CALL_LINK_ROOT_KEY,
        CallLinkRootKey.generateAdminPassKey(),
        Buffer.of(4, 5, 6),
        CallLinkRestrictions.None
      );
      const requestId = await requestIdPromise;
      RingRTC.receivedHttpResponse(requestId, 403, Buffer.of());
      const state = await callLinkResponse;
      if (state.success) {
        assert.fail('should have failed');
      } else {
        assert.equal(state.errorStatusCode, 403);
      }
    });

    it('can read call links', async () => {
      const requestIdPromise = new Promise<number>((resolve, reject) => {
        RingRTC.handleSendHttpRequest = (
          requestId,
          url,
          method,
          _headers,
          _body
        ) => {
          try {
            assert.isTrue(url.startsWith('sfu.example'));
            assert.equal(method, HttpMethod.Get);
            resolve(requestId);
          } catch (e) {
            reject(e);
          }
        };
      });
      const callLinkResponse = RingRTC.readCallLink(
        'sfu.example',
        Buffer.of(1, 2, 3),
        EXAMPLE_CALL_LINK_ROOT_KEY,
        undefined
      );
      const requestId = await requestIdPromise;
      RingRTC.receivedHttpResponse(
        requestId,
        200,
        Buffer.from(EXAMPLE_STATE_JSON)
      );
      const state = await callLinkResponse;
      if (state.success) {
        assert.deepEqual(
          state.value.expiration,
          new Date(EXPIRATION_EPOCH_SECONDS * 1000)
        );
      } else {
        assert.fail('should have succeeded');
      }
    });

    it('can handle failure when reading call links', async () => {
      const requestIdPromise = new Promise<number>((resolve, reject) => {
        RingRTC.handleSendHttpRequest = (
          requestId,
          url,
          method,
          _headers,
          _body
        ) => {
          try {
            assert.isTrue(url.startsWith('sfu.example'));
            assert.equal(method, HttpMethod.Get);
            resolve(requestId);
          } catch (e) {
            reject(e);
          }
        };
      });
      const callLinkResponse = RingRTC.readCallLink(
        'sfu.example',
        Buffer.of(1, 2, 3),
        EXAMPLE_CALL_LINK_ROOT_KEY,
        undefined
      );
      const requestId = await requestIdPromise;
      RingRTC.receivedHttpResponse(requestId, 404, Buffer.of());
      const state = await callLinkResponse;
      if (state.success) {
        assert.fail('should have failed');
      } else {
        assert.equal(state.errorStatusCode, 404);
      }
    });

    it('can update call link names', async () => {
      const requestIdPromise = new Promise<number>((resolve, reject) => {
        RingRTC.handleSendHttpRequest = (
          requestId,
          url,
          method,
          _headers,
          _body
        ) => {
          try {
            assert.isTrue(url.startsWith('sfu.example'));
            assert.equal(method, HttpMethod.Put);
            resolve(requestId);
          } catch (e) {
            reject(e);
          }
        };
      });
      const callLinkResponse = RingRTC.updateCallLinkName(
        'sfu.example',
        Buffer.of(1, 2, 3),
        EXAMPLE_CALL_LINK_ROOT_KEY,
        undefined,
        CallLinkRootKey.generateAdminPassKey(),
        'Secret Hideout'
      );
      const requestId = await requestIdPromise;
      RingRTC.receivedHttpResponse(
        requestId,
        200,
        Buffer.from(EXAMPLE_STATE_JSON)
      );
      const state = await callLinkResponse;
      // Don't bother checking anything beyond status here, since we are mocking the SFU's responses anyway.
      assert.isTrue(state.success);
    });

    it('can handle failure when updating call link names', async () => {
      const requestIdPromise = new Promise<number>((resolve, reject) => {
        RingRTC.handleSendHttpRequest = (
          requestId,
          url,
          method,
          _headers,
          _body
        ) => {
          try {
            assert.isTrue(url.startsWith('sfu.example'));
            assert.equal(method, HttpMethod.Put);
            resolve(requestId);
          } catch (e) {
            reject(e);
          }
        };
      });
      const callLinkResponse = RingRTC.updateCallLinkName(
        'sfu.example',
        Buffer.of(1, 2, 3),
        EXAMPLE_CALL_LINK_ROOT_KEY,
        undefined,
        CallLinkRootKey.generateAdminPassKey(),
        'Secret Hideout'
      );
      const requestId = await requestIdPromise;
      RingRTC.receivedHttpResponse(requestId, 403, Buffer.of());
      const state = await callLinkResponse;
      if (state.success) {
        assert.fail('should have failed');
      } else {
        assert.equal(state.errorStatusCode, 403);
      }
    });

    it('can clear call link names', async () => {
      const requestIdPromise = new Promise<number>((resolve, reject) => {
        RingRTC.handleSendHttpRequest = (
          requestId,
          url,
          method,
          _headers,
          _body
        ) => {
          try {
            assert.isTrue(url.startsWith('sfu.example'));
            assert.equal(method, HttpMethod.Put);
            resolve(requestId);
          } catch (e) {
            reject(e);
          }
        };
      });
      const callLinkResponse = RingRTC.updateCallLinkName(
        'sfu.example',
        Buffer.of(1, 2, 3),
        EXAMPLE_CALL_LINK_ROOT_KEY,
        undefined,
        CallLinkRootKey.generateAdminPassKey(),
        ''
      );
      const requestId = await requestIdPromise;
      RingRTC.receivedHttpResponse(
        requestId,
        200,
        Buffer.from(EXAMPLE_STATE_JSON)
      );
      const state = await callLinkResponse;
      // Don't bother checking anything beyond status here, since we are mocking the SFU's responses anyway.
      assert.isTrue(state.success);
    });

    it('can update call link restrictions', async () => {
      const requestIdPromise = new Promise<number>((resolve, reject) => {
        RingRTC.handleSendHttpRequest = (
          requestId,
          url,
          method,
          _headers,
          _body
        ) => {
          try {
            assert.isTrue(url.startsWith('sfu.example'));
            assert.equal(method, HttpMethod.Put);
            resolve(requestId);
          } catch (e) {
            reject(e);
          }
        };
      });
      const callLinkResponse = RingRTC.updateCallLinkRestrictions(
        'sfu.example',
        Buffer.of(1, 2, 3),
        EXAMPLE_CALL_LINK_ROOT_KEY,
        undefined,
        CallLinkRootKey.generateAdminPassKey(),
        CallLinkRestrictions.AdminApproval
      );
      const requestId = await requestIdPromise;
      RingRTC.receivedHttpResponse(
        requestId,
        200,
        Buffer.from(EXAMPLE_STATE_JSON)
      );
      const state = await callLinkResponse;
      // Don't bother checking anything beyond status here, since we are mocking the SFU's responses anyway.
      assert.isTrue(state.success);
    });

    it('can delete call link', async () => {
      const requestIdPromise = new Promise<number>((resolve, reject) => {
        RingRTC.handleSendHttpRequest = (
          requestId,
          url,
          method,
          _headers,
          _body
        ) => {
          try {
            assert.isTrue(url.startsWith('sfu.example'));
            assert.equal(method, HttpMethod.Delete);
            resolve(requestId);
          } catch (e) {
            reject(e);
          }
        };
      });
      const callLinkResponse = RingRTC.deleteCallLink(
        'sfu.example',
        Buffer.of(1, 2, 3),
        EXAMPLE_CALL_LINK_ROOT_KEY,
        undefined,
        CallLinkRootKey.generateAdminPassKey()
      );
      const requestId = await requestIdPromise;
      RingRTC.receivedHttpResponse(
        requestId,
        200,
        Buffer.from(EXAMPLE_EMPTY_RESPONSE)
      );
      const state = await callLinkResponse;
      // Don't bother checking anything beyond status here, since we are mocking the SFU's responses anyway.
      assert.isTrue(state.success);
    });

    it('can peek with no active call', async () => {
      const requestIdPromise = new Promise<number>((resolve, reject) => {
        RingRTC.handleSendHttpRequest = (
          requestId,
          url,
          method,
          _headers,
          _body
        ) => {
          try {
            assert.isTrue(url.startsWith('sfu.example'));
            assert.equal(method, HttpMethod.Get);
            resolve(requestId);
          } catch (e) {
            reject(e);
          }
        };
      });
      const callLinkResponse = RingRTC.peekCallLinkCall(
        'sfu.example',
        Buffer.of(1, 2, 3),
        EXAMPLE_CALL_LINK_ROOT_KEY,
        undefined
      );
      const requestId = await requestIdPromise;
      RingRTC.receivedHttpResponse(requestId, 404, Buffer.from([]));
      const state = await callLinkResponse;
      if (state.success) {
        assert.isUndefined(state.value.eraId);
        assert.equal(state.value.deviceCountIncludingPendingDevices, 0);
        assert.equal(state.value.deviceCountExcludingPendingDevices, 0);
      } else {
        assert.fail('should have succeeded');
      }
    });

    it('can peek an expired link', async () => {
      const requestIdPromise = new Promise<number>((resolve, reject) => {
        RingRTC.handleSendHttpRequest = (
          requestId,
          url,
          method,
          _headers,
          _body
        ) => {
          try {
            assert.isTrue(url.startsWith('sfu.example'));
            assert.equal(method, HttpMethod.Get);
            resolve(requestId);
          } catch (e) {
            reject(e);
          }
        };
      });
      const callLinkResponse = RingRTC.peekCallLinkCall(
        'sfu.example',
        Buffer.of(1, 2, 3),
        EXAMPLE_CALL_LINK_ROOT_KEY,
        undefined
      );
      const requestId = await requestIdPromise;
      RingRTC.receivedHttpResponse(
        requestId,
        404,
        Buffer.from('{"reason":"expired"}', 'utf-8')
      );
      const state = await callLinkResponse;
      if (state.success) {
        assert.fail('should have failed');
      } else {
        assert.equal(state.errorStatusCode, PeekStatusCodes.EXPIRED_CALL_LINK);
      }
    });

    it('can peek an invalid link', async () => {
      const requestIdPromise = new Promise<number>((resolve, reject) => {
        RingRTC.handleSendHttpRequest = (
          requestId,
          url,
          method,
          _headers,
          _body
        ) => {
          try {
            assert.isTrue(url.startsWith('sfu.example'));
            assert.equal(method, HttpMethod.Get);
            resolve(requestId);
          } catch (e) {
            reject(e);
          }
        };
      });
      const callLinkResponse = RingRTC.peekCallLinkCall(
        'sfu.example',
        Buffer.of(1, 2, 3),
        EXAMPLE_CALL_LINK_ROOT_KEY,
        undefined
      );
      const requestId = await requestIdPromise;
      RingRTC.receivedHttpResponse(
        requestId,
        404,
        Buffer.from('{"reason":"invalid"}', 'utf-8')
      );
      const state = await callLinkResponse;
      if (state.success) {
        assert.fail('should have failed');
      } else {
        assert.equal(state.errorStatusCode, PeekStatusCodes.INVALID_CALL_LINK);
      }
    });

    class NullGroupObserver {
      /* eslint-disable @typescript-eslint/no-empty-function */
      requestMembershipProof(_call: GroupCall) {}
      requestGroupMembers(_call: GroupCall) {}
      onLocalDeviceStateChanged(_call: GroupCall) {}
      onRemoteDeviceStatesChanged(_call: GroupCall) {}
      onAudioLevels(_call: GroupCall) {}
      onLowBandwidthForVideo(_call: GroupCall, _recovered: boolean) {}
      onReactions(_call: GroupCall, _reactions: Array<Reaction>) {}
      onRaisedHands(_call: GroupCall, _raisedHands: Array<number>) {}
      onPeekChanged(_call: GroupCall) {}
      onEnded(_call: GroupCall, _reason: GroupCallEndReason) {}
      onSpeechEvent(_call: GroupCall, _event: SpeechEvent) {}
      onRemoteMute(_call: GroupCall, _demuxId: number) {}
      onObservedRemoteMute(
        _call: GroupCall,
        _sourceDemuxId: number,
        _targetDemuxId: number
      ) {}
      /* eslint-enable @typescript-eslint/no-empty-function */
    }

    it('can create a call and try to connect', async () => {
      CallingClass.initializeLoggingOnly();
      const observer = sinon.spy(new NullGroupObserver());
      const call = RingRTC.getCallLinkCall(
        'sfu.example',
        EXAMPLE_PUBLIC_ENDORSEMENT_KEY,
        Buffer.of(1, 2, 3), // auth creds
        EXAMPLE_CALL_LINK_ROOT_KEY,
        undefined, // epoch
        undefined, // admin pass
        Buffer.of(), // hkdf
        undefined, // audio levels
        observer
      );
      assert.isObject(call);
      assert.equal(call?.getKind(), GroupCallKind.CallLink);
      call?.connect();
      await sleep(1000);
      observer.requestMembershipProof.should.not.have.been.called;
      observer.requestGroupMembers.should.not.have.been.called;
    });
  });
});
