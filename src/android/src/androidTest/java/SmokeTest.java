/*
 * Copyright 2023 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

import org.signal.ringrtc.CallManager;

import androidx.test.core.app.ApplicationProvider;
import java.util.HashMap;
import org.junit.Test;

import static org.mockito.Mockito.mock;

public class SmokeTest {
    @Test
    public void testInitialization() throws Exception {
        CallManager.initialize(ApplicationProvider.getApplicationContext(), mock(), new HashMap<>());
        CallManager.createCallManager(mock());
    }
}
