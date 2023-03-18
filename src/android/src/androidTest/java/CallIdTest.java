/*
 * Copyright 2023 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

import org.signal.ringrtc.CallId;

import org.junit.Test;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertNotEquals;

public class CallIdTest extends CallTestBase {
    @Test
    public void testFromEra() throws Exception {
        CallId fromEra = CallId.fromEra("1122334455667788");
        CallId fromHex = new CallId(0x1122334455667788L);
        assertEquals(fromEra, fromHex);

        // Just don't crash.
        CallId fromUnusualEra = CallId.fromEra("mesozoic");
        assertNotEquals(fromEra, fromUnusualEra);
        assertNotEquals(0, fromUnusualEra.longValue());
    }
}
