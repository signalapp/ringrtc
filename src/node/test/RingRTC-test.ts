import { assert } from 'chai';
import RingRTC = require('../index');

describe('RingRTC', () => {
    it('testsInitialization', () => {
        assert.isNotNull(RingRTC, "RingRTC didn't initialize!");
    });
});
