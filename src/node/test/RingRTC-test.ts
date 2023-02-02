//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import { assert, expect, use } from 'chai';
import chaiAsPromised from 'chai-as-promised';
import { randomBytes } from 'crypto';
import {
  CallEndedReason,
  CallingMessage,
  CallState,
  OfferType,
  RingRTC,
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
        callId: { high: 0, low: 123 },
        type: OfferType.AudioCall,
        opaque: Buffer.from([]),
      },
      supportsMultiRing: true,
    };
    const age = 60 * 60;
    try {
      const { reason, ageSec: reportedAge } = await new Promise(
        (resolve, _reject) => {
          RingRTC.handleAutoEndedIncomingCallRequest = (
            _callId,
            _remoteUserId,
            reason,
            ageSec
          ) => {
            resolve({ reason, ageSec });
          };
          RingRTC.handleCallingMessage(
            'remote',
            null,
            4,
            2,
            age,
            1,
            offer,
            Buffer.from([]),
            Buffer.from([])
          );
        }
      );
      assert.equal(reason, CallEndedReason.ReceivedOfferExpired);
      assert.equal(reportedAge, age);
    } finally {
      RingRTC.handleAutoEndedIncomingCallRequest = null;
    }
  });

  it('reports 0 as the age of other auto-ended offers', async () => {
    const offer: CallingMessage = {
      offer: {
        callId: { high: 0, low: 123 },
        type: OfferType.AudioCall,
        opaque: Buffer.from([]),
      },
      supportsMultiRing: true,
    };
    try {
      const { reason, ageSec: reportedAge } = await new Promise(
        (resolve, _reject) => {
          RingRTC.handleAutoEndedIncomingCallRequest = (
            _callId,
            _remoteUserId,
            reason,
            ageSec
          ) => {
            resolve({ reason, ageSec });
          };
          RingRTC.handleCallingMessage(
            'remote',
            null,
            4,
            2,
            10,
            2,
            offer,
            Buffer.from([]),
            Buffer.from([])
          );
        }
      );
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
    let calling = new CallingClass(user1_name, user1_id);
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
    let calling = new CallingClass(user1_name, user1_id);
    calling.initialize();
    initializeSpies();

    // Generate incoming calling message
    const message_age_sec = 1;
    const message_received_at_counter = 10;
    const callId = new Long(1, 1, true);
    const offerCallingMessage = generateOfferCallingMessage(callId);

    RingRTC.handleCallingMessage(
      user2_id,
      Buffer.from(uuidToBytes(user2_id)),
      user2_device_id,
      user1_device_id,
      message_age_sec,
      message_received_at_counter,
      offerCallingMessage,
      user2_identity_key,
      user1_identity_key
    );

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

  it('ignores outgoing call when incoming call is in progress', async () => {
    let calling = new CallingClass(user1_name, user1_id);
    calling.initialize();
    initializeSpies();

    // Delay the outgoing direct call emulating the case when an incoming call is already in progress
    calling.delayCallSettingsRequest = 1000;

    const c = countDownLatch(1);

    calling
      .startOutgoingDirectCall(user2_id)
      .then(result => {
        assert.fail('Outgoing direct call not rejected');
      })
      .catch(e => {
        log('Outgoing call failed as expected');
        c.countDown();
      });

    // Start an incoming call

    // Generate incoming calling message
    const message_age_sec = 1;
    const message_received_at_counter = 10;
    const callId = new Long(1, 1, true);
    const offerCallingMessage = generateOfferCallingMessage(callId);

    calling.delayCallSettingsRequest = 0;

    RingRTC.handleCallingMessage(
      user2_id,
      Buffer.from(uuidToBytes(user2_id)),
      user2_device_id,
      user1_device_id,
      message_age_sec,
      message_received_at_counter,
      offerCallingMessage,
      user2_identity_key,
      user1_identity_key
    );

    await sleep(1000);

    try {
      await c.finished;
    } catch (e) {
      assert.fail('Outgoing call did not fail as expected');
    }

    // Validate that the incoming call is the current RingRTC call
    assert.isTrue(callId.eq(Long.fromValue(RingRTC.call!.callId)));

    handleIncomingCallSpy.should.have.been.calledOnce;
    assert.equal(CallState.Prering, RingRTC.call!.state);

    // Hangup call
    expect(calling.hangup()).to.be.true;
    await sleep(500);

    // Validate hangup related callbacks and call state
    handleAutoEndedIncomingCallRequestSpy.should.have.been.calledOnce;
    expect(handleOutgoingSignalingSpy.callCount).to.be.gt(0);
    assert.equal(CallState.Ended, RingRTC.call!.state);
  });

  it('ignores incoming call when outgoing call in progress', async () => {
    let calling = new CallingClass(user1_name, user1_id);
    calling.initialize();
    initializeSpies();

    const outgoingCallLatch = countDownLatch(1);

    calling
      .startOutgoingDirectCall(user2_id)
      .then(result => {
        log('Outgoing call succeeded as expected');
        outgoingCallLatch.countDown();
      })
      .catch(e => {
        assert.fail('Outgoing direct call should not have been rejected');
      });

    await sleep(1000);

    expect(handleOutgoingSignalingSpy.callCount).to.be.gt(1);
    assert.equal(CallState.Prering, RingRTC.call!.state);

    // Start an incoming call

    // Generate incoming calling message
    const message_age_sec = 1;
    const message_received_at_counter = 10;
    const callId = new Long(1, 1, true);
    const offerCallingMessage = generateOfferCallingMessage(callId);

    RingRTC.handleCallingMessage(
      user2_id,
      Buffer.from(uuidToBytes(user2_id)),
      user2_device_id,
      user1_device_id,
      message_age_sec,
      message_received_at_counter,
      offerCallingMessage,
      user2_identity_key,
      user1_identity_key
    );

    await sleep(1000);

    try {
      await outgoingCallLatch.finished;
    } catch (e) {
      assert.fail('Outgoing call did not succeed as expected');
    }

    // Validate that the incoming call is not the current RingRTC call
    assert.isTrue(callId.neq(Long.fromValue(RingRTC.call!.callId)));

    const handleStateChangedSpy = sinon.spy(
      RingRTC.call!,
      'handleStateChanged'
    );

    // Cleanup.
    expect(calling.hangup()).to.be.true;
    await sleep(500);
    handleStateChangedSpy.should.have.been.calledOnce;
    expect(calling.hangup()).to.be.false;
    await sleep(100);
  });

  it('outgoing call is replaced by incoming call when call settings delayed for the outgoing call', async () => {
    let calling = new CallingClass(user1_name, user1_id);
    calling.initialize();
    initializeSpies();

    const outgoingCallLatch = countDownLatch(1);

    calling.delayCallSettingsRequest = 1000;

    calling
      .startOutgoingDirectCall(user2_id)
      .then(result => {
        assert.fail('Outgoing call should not have succeeded');
      })
      .catch(e => {
        log('Outgoing call failed as expected');
        outgoingCallLatch.countDown();
      });

    // Start an incoming call
    calling.delayCallSettingsRequest = 0;

    // Generate incoming calling message
    const message_age_sec = 1;
    const message_received_at_counter = 10;
    const callId = new Long(1, 1, true);
    const offerCallingMessage = generateOfferCallingMessage(callId);

    RingRTC.handleCallingMessage(
      user2_id,
      Buffer.from(uuidToBytes(user2_id)),
      user2_device_id,
      user1_device_id,
      message_age_sec,
      message_received_at_counter,
      offerCallingMessage,
      user2_identity_key,
      user1_identity_key
    );

    await sleep(1000);

    try {
      await outgoingCallLatch.finished;
    } catch (e) {
      assert.fail('Outgoing call failed as expected');
    }

    // Validate that the incoming call is the current RingRTC call
    assert.isTrue(callId.eq(Long.fromValue(RingRTC.call!.callId)));

    // Cleanup.
    expect(calling.hangup()).to.be.true;
    await sleep(500);

    // Validate hangup related callbacks and call state
    handleAutoEndedIncomingCallRequestSpy.should.have.been.calledOnce;
    expect(handleOutgoingSignalingSpy.callCount).to.be.gt(0);
    assert.equal(CallState.Ended, RingRTC.call!.state);
  });

  it('outgoing call wins glare when incoming call id is lower', async () => {
    let calling = new CallingClass(user1_name, user1_id);
    calling.initialize();
    initializeSpies();

    await runGlareScenario(calling, true);
  });

  it('outgoing call loses glare when incoming call id is higher', async () => {
    let calling = new CallingClass(user1_name, user1_id);
    calling.initialize();
    initializeSpies();

    await runGlareScenario(calling, false);
  });

  async function runGlareScenario(
    calling: CallingClass,
    outgoingWinner: boolean
  ) {
    const outgoingCallLatch = countDownLatch(1);
    calling
      .startOutgoingDirectCall(user2_id)
      .then(result => {
        log('Outgoing call succeeded as expected');
        outgoingCallLatch.countDown();
      })
      .catch(e => {
        assert.fail('Outgoing call should not have failed');
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
    const message_age_sec = 1;
    const message_received_at_counter = 10;
    const offerCallingMessage = generateOfferCallingMessage(incomingCallId);

    // Initiate an incoming call
    RingRTC.handleCallingMessage(
      user2_id,
      Buffer.from(uuidToBytes(user2_id)),
      user2_device_id,
      user1_device_id,
      message_age_sec,
      message_received_at_counter,
      offerCallingMessage,
      user2_identity_key,
      user1_identity_key
    );

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
});
