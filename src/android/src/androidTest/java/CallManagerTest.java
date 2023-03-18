/*
 * Copyright 2023 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

import org.signal.ringrtc.CallManager;

import org.junit.Test;

import static org.mockito.Mockito.mock;

public class CallManagerTest extends CallTestBase {
    @Test
    public void testInitialization() throws Exception {
        CallManager.createCallManager(mock());
    }
}
