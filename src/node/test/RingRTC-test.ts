//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import { assert, use } from 'chai';
import * as chaiAsPromised from 'chai-as-promised';
import { CallEndedReason, CallingMessage, OfferType, RingRTC } from '../index';

use(chaiAsPromised);

describe('RingRTC', () => {
  it('testsInitialization', () => {
    assert.isNotNull(RingRTC, "RingRTC didn't initialize!");
  });

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
});
