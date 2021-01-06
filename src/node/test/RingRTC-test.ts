//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import { assert } from 'chai';
import RingRTC = require('../index');

describe('RingRTC', () => {
    it('testsInitialization', () => {
        assert.isNotNull(RingRTC, "RingRTC didn't initialize!");
    });
});
