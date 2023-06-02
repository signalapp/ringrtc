/*
 * Copyright 2023 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

import org.signal.ringrtc.CallManager;

import org.hamcrest.CoreMatchers;
import org.junit.Rule;
import org.junit.Test;
import org.junit.rules.ErrorCollector;
import org.mockito.ArgumentCaptor;

import java.util.ArrayList;
import java.util.concurrent.CountDownLatch;

import static org.hamcrest.CoreMatchers.is;
import static org.mockito.Mockito.*;

public class CallManagerTest extends CallTestBase {
    @Rule
    public ErrorCollector errors = new ErrorCollector();

    @Test
    public void testInitialization() throws Exception {
        CallManager.createCallManager(mock());
    }

    // Testing a non-empty group call peek is complicated because of the use of hashing for opaque user IDs.
    // But we can at least test that a peek with no active call comes back with empty PeekInfo.
    @Test
    public void testPeekNoActiveCall() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.peekGroupCall("sfu.example", new byte[] { 1, 2, 3 }, new ArrayList<>(), result -> {
            errors.checkThat(result.getEraId(), is((String)null));
            errors.checkThat(result.getDeviceCount(), is(0L));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.GET), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 404, new byte[] {});
        latch.await();
    }

    @Test
    public void testCallbackExceptionHandling() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.peekGroupCall("sfu.example", new byte[] { 1, 2, 3 }, new ArrayList<>(), result -> {
            Thread.currentThread().setUncaughtExceptionHandler((thread, exception) -> {
                errors.checkThat(exception, is(CoreMatchers.instanceOf(IllegalStateException.class)));
                errors.checkThat(exception.getMessage(), is("abc"));
                latch.countDown();
            });
            throw new IllegalStateException("abc");
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.GET), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 404, new byte[] {});
        latch.await();
    }
}
