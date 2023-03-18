/*
 * Copyright 2023 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

import org.signal.ringrtc.CallManager;

import androidx.test.core.app.ApplicationProvider;
import java.util.HashMap;

import static org.mockito.Mockito.mock;

public class CallTestBase {
    static {
        CallManager.initialize(ApplicationProvider.getApplicationContext(), mock(), new HashMap<>());
    }
}
