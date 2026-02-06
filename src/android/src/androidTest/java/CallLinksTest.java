/*
 * Copyright 2023 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

import org.signal.ringrtc.AudioConfig;
import org.signal.ringrtc.CallException;
import org.signal.ringrtc.CallLinkState;
import org.signal.ringrtc.CallLinkRootKey;
import org.signal.ringrtc.CallManager;
import org.signal.ringrtc.GroupCall;
import org.signal.ringrtc.PeekInfo;

import org.junit.Rule;
import org.junit.Test;
import org.junit.rules.ErrorCollector;
import org.mockito.ArgumentCaptor;

import java.util.Arrays;
import java.util.concurrent.CountDownLatch;

import static org.hamcrest.CoreMatchers.is;
import static org.junit.Assert.assertArrayEquals;
import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.mockito.Mockito.*;

public class CallLinksTest extends CallTestBase {
    private static final CallLinkRootKey EXAMPLE_KEY;
    private static final CallLinkRootKey EXAMPLE_KEY_V1_VALID;
    private static final CallLinkRootKey EXAMPLE_KEY_V1_INVALID;
    private static final byte[] ENDORSEMENT_PUBLIC_KEY = {
            (byte) 0,
            (byte) 86,
            (byte) 35,
            (byte) 236,
            (byte) 48,
            (byte) 147,
            (byte) 33,
            (byte) 66,
            (byte) 168,
            (byte) 208,
            (byte) 215,
            (byte) 207,
            (byte) 250,
            (byte) 177,
            (byte) 151,
            (byte) 88,
            (byte) 0,
            (byte) 158,
            (byte) 219,
            (byte) 130,
            (byte) 38,
            (byte) 212,
            (byte) 159,
            (byte) 171,
            (byte) 211,
            (byte) 130,
            (byte) 220,
            (byte) 217,
            (byte) 29,
            (byte) 133,
            (byte) 9,
            (byte) 96,
            (byte) 97
    };
    static {
        try {
            EXAMPLE_KEY = new CallLinkRootKey("bcdf-ghkm-npqr-stxz-bcdf-ghkm-npqr-stxz");
            EXAMPLE_KEY_V1_VALID = new CallLinkRootKey("bcdfghkm-npqrstxz-bcdfghkm-npqrstxz-bc-sbspxdpx");
            EXAMPLE_KEY_V1_INVALID = new CallLinkRootKey("bcdfghkm-npqrstxz-bcdfghkm-npqrstxz-nc-bbbbbbbb");
        } catch (CallException e) {
            throw new AssertionError(e);
        }
    }
    private static final long EXPIRATION_EPOCH_SECONDS = 4133980800L; // 2101-01-01
    private static final long EPOCH = 3234456222L;
    private static final String EXAMPLE_STATE_JSON = "{\"restrictions\": \"none\",\"name\":\"\",\"revoked\":false,\"expiration\":" + EXPIRATION_EPOCH_SECONDS + "}";
    private static final String EXAMPLE_STATE_JSON_WITH_EPOCH = "{\"restrictions\": \"none\",\"name\":\"\",\"revoked\":false,\"expiration\":" + EXPIRATION_EPOCH_SECONDS + ",\"epoch\":" + EPOCH + "}";
    private static final String EXAMPLE_EMPTY_JSON = "{}";

    @Rule
    public ErrorCollector errors = new ErrorCollector();

    @Test
    public void testKeyAccessors() throws Exception {
        final CallLinkRootKey anotherKey = CallLinkRootKey.generate();
        assertFalse(Arrays.equals(EXAMPLE_KEY.getKeyBytes(), anotherKey.getKeyBytes()));

        assertArrayEquals(EXAMPLE_KEY.deriveRoomId(), EXAMPLE_KEY.deriveRoomId());
        assertFalse(Arrays.equals(EXAMPLE_KEY.deriveRoomId(), anotherKey.deriveRoomId()));
    }

    @Test
    public void testFormatting() throws Exception {
        assertEquals("bcdf-ghkm-npqr-stxz-bcdf-ghkm-npqr-stxz", EXAMPLE_KEY.toString());
        assertEquals("bcdfghkm-npqrstxz-bcdfghkm-npqrstxz-bc-sbspxdpx", EXAMPLE_KEY_V1_VALID.toString());
    }

    @Test
    public void testCreateSuccessV0() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.createCallLink("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY_V1_INVALID, CallLinkRootKey.generateAdminPasskey(), new byte[] { 4, 5, 6 }, CallLinkState.Restrictions.NONE, result -> {
            errors.checkThat(result.getStatus(), is((short)200));
            errors.checkThat(result.isSuccess(), is(true));
            errors.checkThat(result.getValue().getExpiration().getEpochSecond(), is(EXPIRATION_EPOCH_SECONDS));
            errors.checkThat(result.getValue().getRootKey().getKeyBytes(), is(EXAMPLE_KEY.getKeyBytes()));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.PUT), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 200, EXAMPLE_STATE_JSON.getBytes("UTF-8"));
        latch.await();
    }

    @Test
    public void testCreateSuccessV1() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.createCallLink("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY_V1_INVALID, CallLinkRootKey.generateAdminPasskey(), new byte[] { 4, 5, 6 }, CallLinkState.Restrictions.NONE, result -> {
            errors.checkThat(result.getStatus(), is((short)200));
            errors.checkThat(result.isSuccess(), is(true));
            errors.checkThat(result.getValue().getExpiration().getEpochSecond(), is(EXPIRATION_EPOCH_SECONDS));
            errors.checkThat(result.getValue().getRootKey().getKeyBytes(), is(EXAMPLE_KEY_V1_VALID.getKeyBytes()));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.PUT), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 200, EXAMPLE_STATE_JSON_WITH_EPOCH.getBytes("UTF-8"));
        latch.await();
    }

    @Test
    public void testCreateFailure() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.createCallLink("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY_V1_INVALID, CallLinkRootKey.generateAdminPasskey(), new byte[] { 4, 5, 6 }, CallLinkState.Restrictions.NONE, result -> {
            errors.checkThat(result.getStatus(), is((short)403));
            errors.checkThat(result.isSuccess(), is(false));
            errors.checkThat(result.getValue(), is((CallLinkState)null));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.PUT), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 403, new byte[] {});
        latch.await();
    }

    @Test
    public void testReadSuccessV0() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.readCallLink("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY, result -> {
            errors.checkThat(result.getStatus(), is((short)200));
            errors.checkThat(result.isSuccess(), is(true));
            errors.checkThat(result.getValue().getExpiration().getEpochSecond(), is(EXPIRATION_EPOCH_SECONDS));
            errors.checkThat(result.getValue().getRootKey().getKeyBytes(), is(EXAMPLE_KEY.getKeyBytes()));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.GET), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 200, EXAMPLE_STATE_JSON.getBytes("UTF-8"));
        latch.await();
    }

    @Test
    public void testReadSuccessV1() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.readCallLink("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY_V1_VALID, result -> {
            errors.checkThat(result.getStatus(), is((short)200));
            errors.checkThat(result.isSuccess(), is(true));
            errors.checkThat(result.getValue().getExpiration().getEpochSecond(), is(EXPIRATION_EPOCH_SECONDS));
            errors.checkThat(result.getValue().getRootKey().getKeyBytes(), is(EXAMPLE_KEY_V1_VALID.getKeyBytes()));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.GET), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 200, EXAMPLE_STATE_JSON.getBytes("UTF-8"));
        latch.await();
    }

    @Test
    public void testReadFailure() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.readCallLink("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY, result -> {
            errors.checkThat(result.getStatus(), is((short)404));
            errors.checkThat(result.isSuccess(), is(false));
            errors.checkThat(result.getValue(), is((CallLinkState)null));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.GET), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 404, new byte[] {});
        latch.await();
    }

    @Test
    public void testUpdateNameSuccessV0() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.updateCallLinkName("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY, CallLinkRootKey.generateAdminPasskey(), "Secret Hideout", result -> {
            errors.checkThat(result.isSuccess(), is(true));
            errors.checkThat(result.getValue().getRootKey().getKeyBytes(), is(EXAMPLE_KEY.getKeyBytes()));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.PUT), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 200, EXAMPLE_STATE_JSON.getBytes("UTF-8"));
        latch.await();
    }

    @Test
    public void testUpdateNameSuccessV1() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.updateCallLinkName("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY_V1_VALID, CallLinkRootKey.generateAdminPasskey(), "Secret Hideout", result -> {
            errors.checkThat(result.isSuccess(), is(true));
            errors.checkThat(result.getValue().getRootKey().getKeyBytes(), is(EXAMPLE_KEY_V1_VALID.getKeyBytes()));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.PUT), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 200, EXAMPLE_STATE_JSON.getBytes("UTF-8"));
        latch.await();
    }

    @Test
    public void testUpdateNameFailure() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.updateCallLinkName("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY, CallLinkRootKey.generateAdminPasskey(), "Secret Hideout", result -> {
            errors.checkThat(result.isSuccess(), is(false));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.PUT), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 403, new byte[] {});
        latch.await();
    }

    @Test
    public void testUpdateNameEmptySuccess() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.updateCallLinkName("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY, CallLinkRootKey.generateAdminPasskey(), "", result -> {
            errors.checkThat(result.isSuccess(), is(true));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.PUT), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 200, EXAMPLE_STATE_JSON.getBytes("UTF-8"));
        latch.await();
    }

    @Test
    public void testUpdateRestrictionsSuccess() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.updateCallLinkRestrictions("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY, CallLinkRootKey.generateAdminPasskey(), CallLinkState.Restrictions.ADMIN_APPROVAL, result -> {
            errors.checkThat(result.isSuccess(), is(true));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.PUT), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 200, EXAMPLE_STATE_JSON.getBytes("UTF-8"));
        latch.await();
    }

    @Test
    public void testDeleteSuccessV0() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.deleteCallLink("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY, CallLinkRootKey.generateAdminPasskey(), result -> {
            errors.checkThat(result.isSuccess(), is(true));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.DELETE), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 200, EXAMPLE_EMPTY_JSON.getBytes("UTF-8"));
        latch.await();
    }

    @Test
    public void testDeleteSuccessV1() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.deleteCallLink("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY_V1_VALID, CallLinkRootKey.generateAdminPasskey(), result -> {
            errors.checkThat(result.isSuccess(), is(true));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.DELETE), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 200, EXAMPLE_EMPTY_JSON.getBytes("UTF-8"));
        latch.await();
    }

    @Test
    public void testPeekNoActiveCallV0() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.peekCallLinkCall("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY, result -> {
            errors.checkThat(result.getStatus(), is((short)200));
            errors.checkThat(result.getValue().getEraId(), is((String)null));
            errors.checkThat(result.getValue().getDeviceCountIncludingPendingDevices(), is(0L));
            errors.checkThat(result.getValue().getDeviceCountExcludingPendingDevices(), is(0L));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.GET), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 404, new byte[] {});
        latch.await();
    }

    @Test
    public void testPeekNoActiveCallV1() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.peekCallLinkCall("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY_V1_VALID, result -> {
            errors.checkThat(result.getStatus(), is((short)200));
            errors.checkThat(result.getValue().getEraId(), is((String)null));
            errors.checkThat(result.getValue().getDeviceCountIncludingPendingDevices(), is(0L));
            errors.checkThat(result.getValue().getDeviceCountExcludingPendingDevices(), is(0L));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.GET), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 404, new byte[] {});
        latch.await();
    }

    @Test
    public void testPeekExpiredLink() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.peekCallLinkCall("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY, result -> {
            errors.checkThat(result.getStatus(), is(PeekInfo.EXPIRED_CALL_LINK_STATUS));
            errors.checkThat(result.getValue(), is((PeekInfo)null));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.GET), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 404, "{\"reason\":\"expired\"}".getBytes("UTF-8"));
        latch.await();
    }

    @Test
    public void testPeekInvalidLink() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        CountDownLatch latch = new CountDownLatch(1);
        callManager.peekCallLinkCall("sfu.example", new byte[] { 1, 2, 3 }, EXAMPLE_KEY, result -> {
            errors.checkThat(result.getStatus(), is(PeekInfo.INVALID_CALL_LINK_STATUS));
            errors.checkThat(result.getValue(), is((PeekInfo)null));
            latch.countDown();
        });

        ArgumentCaptor<Long> requestId = ArgumentCaptor.forClass(Long.class);
        verify(observer).onSendHttpRequest(requestId.capture(), startsWith("sfu.example"), eq(CallManager.HttpMethod.GET), any(), any());

        callManager.receivedHttpResponse(requestId.getValue(), 404, "{\"reason\":\"invalid\"}".getBytes("UTF-8"));
        latch.await();
    }

    @Test
    public void testConnectWithNoResponse() throws Exception {
        CallManager.Observer observer = mock();
        CallManager callManager = CallManager.createCallManager(observer);

        GroupCall.Observer callObserver = mock();
        GroupCall call = callManager.createCallLinkCall("sfu.example", ENDORSEMENT_PUBLIC_KEY, new byte[] { 1, 2, 3 }, EXAMPLE_KEY, null, new byte[] {}, null, new AudioConfig(), callObserver);
        assertEquals(call.getKind(), GroupCall.Kind.CALL_LINK);

        call.connect();
        Thread.sleep(1000);

        verify(callObserver, never()).requestMembershipProof(any());
        verify(callObserver, never()).requestGroupMembers(any());
    }
}
