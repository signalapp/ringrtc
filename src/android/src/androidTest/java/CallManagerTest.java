/*
 * Copyright 2023 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

import org.signal.ringrtc.CallManager;
import org.signal.ringrtc.GroupCall;

import org.hamcrest.CoreMatchers;
import org.junit.Rule;
import org.junit.Test;
import org.junit.rules.ErrorCollector;
import org.mockito.ArgumentCaptor;

import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.Arrays;
import java.util.HashSet;
import java.util.UUID;
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

    private byte[] fakeOpaqueUserId(int fill) {
        byte[] result = new byte[65];
        Arrays.fill(result, (byte)fill);
        return result;
    }

    private String sha256Hex(byte[] input) {
        try {
            final char[] hexDigits = new char[] {
                '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f'
            };
            byte[] digestBytes = MessageDigest.getInstance("SHA-256").digest(input);
            StringBuilder result = new StringBuilder();
            for (byte b : digestBytes) {
                result.append(hexDigits[(b >> 4) & 0xF]);
                result.append(hexDigits[b & 0xF]);
            }
            return result.toString();
        } catch (Exception e) {
            throw new AssertionError(e);
        }
    }

    @Test
    public void testPeekWithPendingUsers() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        UUID user1 = UUID.nameUUIDFromBytes(new byte[] { 1 });
        UUID user2 = UUID.nameUUIDFromBytes(new byte[] { 2 });
        UUID user3 = UUID.nameUUIDFromBytes(new byte[] { 3 });

        GroupCall.GroupMemberInfo[] members = new GroupCall.GroupMemberInfo[] {
            new GroupCall.GroupMemberInfo(user1, fakeOpaqueUserId(1)),
            new GroupCall.GroupMemberInfo(user2, fakeOpaqueUserId(2)),
            new GroupCall.GroupMemberInfo(user3, fakeOpaqueUserId(3))
        };

        CountDownLatch latch = new CountDownLatch(1);
        callManager.peekGroupCall("sfu.example", new byte[] { 1, 2, 3 }, Arrays.asList(members), result -> {
            errors.checkThat(result.getEraId(), is("mesozoic"));
            errors.checkThat(result.getDeviceCount(), is(7L));
            errors.checkThat(result.getMaxDevices(), is(20L));
            errors.checkThat(result.getCreator(), is(user1));
            errors.checkThat(
                new HashSet<>(result.getJoinedMembers()),
                is(new HashSet<>(Arrays.asList(user1, user2))));
            errors.checkThat(result.getPendingUsers(), is(Arrays.asList(user3)));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.GET), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 200, (
            "{" +
                "\"conferenceId\":\"mesozoic\"," +
                "\"maxDevices\":20," +
                "\"creator\":\"" + sha256Hex(fakeOpaqueUserId(1)) + "\"," +
                "\"participants\":[" +
                    "{\"opaqueUserId\":\"" + sha256Hex(fakeOpaqueUserId(1)) + "\",\"demuxId\":" + 32 * 1 + "}," +
                    "{\"opaqueUserId\":\"" + sha256Hex(fakeOpaqueUserId(2)) + "\",\"demuxId\":" + 32 * 2 + "}," +
                    "{\"opaqueUserId\":\"" + sha256Hex(fakeOpaqueUserId(4)) + "\",\"demuxId\":" + 32 * 3 + "}" +
                "]," +
                "\"pendingClients\":[" +
                    "{\"opaqueUserId\":\"" + sha256Hex(fakeOpaqueUserId(3)) + "\",\"demuxId\":" + 32 * 4 + "}," +
                    "{\"opaqueUserId\":\"" + sha256Hex(fakeOpaqueUserId(3)) + "\",\"demuxId\":" + 32 * 5 + "}," +
                    "{\"opaqueUserId\":\"" + sha256Hex(fakeOpaqueUserId(4)) + "\",\"demuxId\":" + 32 * 6 + "}," +
                    "{\"demuxId\":" + 32 * 7 + "}" +
                "]" +
            "}").getBytes("UTF-8"));
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
