/*
 *
 *  Copyright (C) 2019, 2020 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

package org.signal.ringrtc;

import android.util.LongSparseArray;
import androidx.annotation.NonNull;
import androidx.annotation.Nullable;

import org.webrtc.AudioSource;
import org.webrtc.AudioTrack;
import org.webrtc.DefaultVideoDecoderFactory;
import org.webrtc.DefaultVideoEncoderFactory;
import org.webrtc.EglBase;
import org.webrtc.MediaConstraints;
import org.webrtc.PeerConnectionFactory;
import org.webrtc.VideoDecoderFactory;
import org.webrtc.VideoEncoderFactory;
import org.webrtc.VideoSink;
import org.webrtc.VideoSource;
import org.webrtc.VideoTrack;

import java.util.ArrayList;
import java.util.Collection;
import java.util.List;
import java.util.UUID;

/**
 *
 * Provides an interface to the RingRTC Group Call implementation.
 *
 */
public final class GroupCall {
    @NonNull private static final String TAG = GroupCall.class.getSimpleName();

    @NonNull  private long                               nativeCallManager;
    @NonNull  private PeerConnectionFactory              factory;

    @NonNull  private Observer                           observer;

    @NonNull          long                               clientId;

    // Whenever the local or remote device states are updated, a new
    // object will be created to update the object value.
    @NonNull  private LocalDeviceState                   localDeviceState;
    @Nullable private LongSparseArray<RemoteDeviceState> remoteDeviceStates;

    @Nullable private PeekInfo                           peekInfo;

    @NonNull  private AudioSource                        outgoingAudioSource;
    @NonNull  private AudioTrack                         outgoingAudioTrack;
    @NonNull  private VideoSource                        outgoingVideoSource;
    @NonNull  private VideoTrack                         outgoingVideoTrack;

    class PeerConnectionFactoryOptions extends PeerConnectionFactory.Options {
        public PeerConnectionFactoryOptions() {
            // Give the (native default) behavior of filtering out loopback addresses.
            // See https://source.chromium.org/chromium/chromium/src/+/master:third_party/webrtc/rtc_base/network.h;l=47?q=.networkIgnoreMask&ss=chromium
            this.networkIgnoreMask = 1 << 4;
        }
    }

    public GroupCall(@NonNull long     nativeCallManager,
                     @NonNull byte[]   groupId,
                     @NonNull String   sfuUrl,
                     @NonNull EglBase  eglBase,
                     @NonNull Observer observer) {
        Log.i(TAG, "GroupCall():");

        this.nativeCallManager = nativeCallManager;
        this.observer = observer;

        this.localDeviceState = new LocalDeviceState();

        VideoEncoderFactory encoderFactory = new DefaultVideoEncoderFactory(eglBase.getEglBaseContext(), true, true);
        VideoDecoderFactory decoderFactory = new DefaultVideoDecoderFactory(eglBase.getEglBaseContext());

        this.factory = PeerConnectionFactory.builder()
            .setOptions(new PeerConnectionFactoryOptions())
            .setVideoEncoderFactory(encoderFactory)
            .setVideoDecoderFactory(decoderFactory)
            .createPeerConnectionFactory();

        MediaConstraints audioConstraints = new MediaConstraints();

        this.outgoingAudioSource = factory.createAudioSource(audioConstraints);
        // Note: This must stay "audio1" to stay in sync with CreateSessionDescriptionForGroupCall.
        this.outgoingAudioTrack = factory.createAudioTrack("audio1", this.outgoingAudioSource);
        this.outgoingAudioTrack.setEnabled(!this.localDeviceState.audioMuted);

        this.outgoingVideoSource = factory.createVideoSource(false);
        // Note: This must stay "video1" to stay in sync with CreateSessionDescriptionForGroupCall.
        this.outgoingVideoTrack = factory.createVideoTrack("video1", this.outgoingVideoSource);
        this.outgoingVideoTrack.setEnabled(!this.localDeviceState.videoMuted);

        // Define maximum output video format for group calls.
        this.outgoingVideoSource.adaptOutputFormat(640, 360, 30);

        try {
            this.clientId = ringrtcCreateGroupCallClient(
                nativeCallManager,
                groupId,
                sfuUrl,
                this.outgoingAudioTrack.getNativeAudioTrack(),
                this.outgoingVideoTrack.getNativeVideoTrack());
            if (this.clientId == 0) {
                // TODO
            }
        } catch  (CallException e) {
            Log.w(TAG, "Unable to create group call client", e);
            throw new AssertionError("Unable to create group call client");
        }
    }

    /**
     *
     */
    public void connect()
        throws CallException
    {
        Log.i(TAG, "connect():");

        ringrtcConnect(nativeCallManager, this.clientId);
    }

    /**
     *
     */
    public void join()
        throws CallException
    {
        Log.i(TAG, "join():");

        ringrtcJoin(nativeCallManager, this.clientId);
    }

    /**
     *
     */
    public void leave()
        throws CallException
    {
        Log.i(TAG, "leave():");

        ringrtcLeave(nativeCallManager, this.clientId);
    }

    /**
     *
     */
    public void disconnect()
        throws CallException
    {
        Log.i(TAG, "disconnect():");

        ringrtcDisconnect(nativeCallManager, this.clientId);
    }

    /**
     *
     */
    public LocalDeviceState getLocalDeviceState()
    {
        Log.i(TAG, "getLocalDevice():");

        return this.localDeviceState;
    }

    /**
     *
     */
    public LongSparseArray<RemoteDeviceState> getRemoteDeviceStates()
    {
        Log.i(TAG, "getRemoteDevices():");

        return this.remoteDeviceStates;
    }

    /**
     *
     */
    public PeekInfo getPeekInfo()
    {
        Log.i(TAG, "getPeekInfo():");

        return this.peekInfo;
    }

    /**
     *
     */
    public void setOutgoingAudioMuted(boolean muted)
        throws CallException
    {
        Log.i(TAG, "setOutgoingAudioMuted():");

        this.localDeviceState.audioMuted = muted;
        this.outgoingAudioTrack.setEnabled(!this.localDeviceState.audioMuted);

        ringrtcSetOutgoingAudioMuted(nativeCallManager, this.clientId, muted);
    }

    /**
     *
     */
    public void setOutgoingVideoMuted(boolean muted)
        throws CallException
    {
        Log.i(TAG, "setOutgoingVideoMuted():");

        this.localDeviceState.videoMuted = muted;
        this.outgoingVideoTrack.setEnabled(!this.localDeviceState.videoMuted);

        ringrtcSetOutgoingVideoMuted(nativeCallManager, this.clientId, muted);
    }

    /**	
     *	
     */	
    public void setOutgoingVideoSource(@NonNull VideoSink     localSink,
                                       @NonNull CameraControl cameraControl)
    {	
        Log.i(TAG, "setOutgoingVideoSource():");	

        if (cameraControl.hasCapturer()) {
            // Connect camera as the local video source.
            cameraControl.initCapturer(this.outgoingVideoSource.getCapturerObserver());
            this.outgoingVideoTrack.addSink(localSink);
        }
    }

    /**
     *
     */
    public void resendMediaKeys()
        throws CallException
    {
        Log.i(TAG, "resendMediaKeys():");

        ringrtcResendMediaKeys(nativeCallManager, this.clientId);
    }

    /**
     *
     */
    public void setBandwidthMode(@NonNull BandwidthMode bandwidthMode)
        throws CallException
    {
        Log.i(TAG, "setBandwidthMode():");

        ringrtcSetBandwidthMode(nativeCallManager, this.clientId, bandwidthMode.ordinal());
    }

    /**
     *
     */
    public void requestVideo(@NonNull Collection<VideoRequest> resolutions)
        throws CallException
    {
        Log.i(TAG, "requestVideo():");

        ringrtcRequestVideo(nativeCallManager, this.clientId, new ArrayList<>(resolutions));
    }

    /**
     *
     */
    public void setGroupMembers(@NonNull Collection<GroupMemberInfo> members)
        throws CallException
    {
        Log.i(TAG, "setGroupMembers():");

        // Convert each userId UUID to a userIdByteArray.
        for (GroupMemberInfo member : members) {
            member.userIdByteArray = Util.getBytesFromUuid(member.userId);
        }

        ringrtcSetGroupMembers(nativeCallManager, this.clientId, new ArrayList<>(members));
    }

    /**
     *
     */
    public void setMembershipProof(@NonNull byte[] proof)
        throws CallException
    {
        Log.i(TAG, "setMembershipProof():");

        ringrtcSetMembershipProof(nativeCallManager, this.clientId, proof);
    }

    /*
     * Called by the CallManager.
     */
    void requestMembershipProof() {
        Log.i(TAG, "requestMembershipProof():");

        this.observer.requestMembershipProof(this);
    }

    /*
     * Called by the CallManager.
     */
    void requestGroupMembers() {
        Log.i(TAG, "handleGroupMembers():");

        this.observer.requestGroupMembers(this);
    }

    /*
     * Called by the CallManager.
     */
    void handleConnectionStateChanged(ConnectionState connectionState) {
        Log.i(TAG, "handleConnectionStateChanged():");

        LocalDeviceState localDeviceState = new LocalDeviceState(this.localDeviceState);
        localDeviceState.connectionState = connectionState;

        this.localDeviceState = localDeviceState;

        this.observer.onLocalDeviceStateChanged(this);
    }

    /*
     * Called by the CallManager.
     */
    void handleJoinStateChanged(JoinState joinState) {
        Log.i(TAG, "handleJoinStateChanged():");

        LocalDeviceState localDeviceState = new LocalDeviceState(this.localDeviceState);
        localDeviceState.joinState = joinState;

        this.localDeviceState = localDeviceState;

        this.observer.onLocalDeviceStateChanged(this);
    }

    /*
     * Called by the CallManager.
     */
    void handleRemoteDevicesChanged(List<RemoteDeviceState> remoteDeviceStates) {
        Log.i(TAG, "handleRemoteDevicesChanged():");

        LongSparseArray<RemoteDeviceState> remoteDeviceByDemuxId = new LongSparseArray<>();
        for (RemoteDeviceState remoteDeviceState : remoteDeviceStates) {
            // Convert each userIdByteArray to userId UUID.
            remoteDeviceState.userId = Util.getUuidFromBytes(remoteDeviceState.userIdByteArray);

            // Maintain the video track if one already exists.
            if (this.remoteDeviceStates != null) {
                RemoteDeviceState existingDeviceState = this.remoteDeviceStates.get(remoteDeviceState.demuxId);
                if (existingDeviceState != null) {
                    remoteDeviceState.videoTrack = existingDeviceState.videoTrack;
                }
            }

            // Build the mapped version of the array with demuxId as the key.
            remoteDeviceByDemuxId.put(remoteDeviceState.demuxId, remoteDeviceState);
        }

        this.remoteDeviceStates = remoteDeviceByDemuxId;

        this.observer.onRemoteDeviceStatesChanged(this);
    }

    /*
     * Called by the CallManager.
     */
    void handleIncomingVideoTrack(long remoteDemuxId, long nativeVideoTrack) {
        Log.i(TAG, "handleIncomingVideoTrack():");

        if (nativeVideoTrack == 0) {
            Log.d(TAG, "nativeVideoTrack is null (0)");
            return;
        }

        RemoteDeviceState remoteDeviceState = this.remoteDeviceStates.get(remoteDemuxId);
        if (remoteDeviceState == null) {
            Log.d(TAG, "No remote device state found for remoteDemuxId");
            return;
        }

        remoteDeviceState.videoTrack = new VideoTrack(nativeVideoTrack);

        this.observer.onRemoteDeviceStatesChanged(this);
    }

    /*
     * Called by the CallManager.
     */
    void handlePeekChanged(PeekInfo info) {
        Log.i(TAG, "handlePeekChanged():");

        this.peekInfo = info;

        this.observer.onPeekChanged(this);
    }

    /*
     * Called by the CallManager.
     */
    void handleEnded(GroupCallEndReason reason) {
        Log.i(TAG, "handleEnded():");

        this.observer.onEnded(this, reason);

        try {
            ringrtcDeleteGroupCallClient(nativeCallManager, this.clientId);
        } catch  (CallException e) {
            Log.w(TAG, "Unable to delete group call client: ", e);
        }
    }

    /**
     *
     */
    public enum ConnectionState {

        /** */
        NOT_CONNECTED,

        /** */
        CONNECTING,

        /** */
        CONNECTED,

        /** */
        RECONNECTING;

        @CalledByNative
        static ConnectionState fromNativeIndex(int nativeIndex) {
            return values()[nativeIndex];
        }
    }

    /**
     *
     */
    public enum JoinState {

        /** */
        NOT_JOINED,

        /** */
        JOINING,

        /** */
        JOINED;

        @CalledByNative
        static JoinState fromNativeIndex(int nativeIndex) {
            return values()[nativeIndex];
        }
    }

    /**
     *
     */
    public enum BandwidthMode {

        /** */
        LOW,

        /** */
        NORMAL;

        @CalledByNative
        static BandwidthMode fromNativeIndex(int nativeIndex) {
            return values()[nativeIndex];
        }
    }

    /**
     *
     */
    public enum GroupCallEndReason {

        // Normal events

        /** */
        DEVICE_EXPLICITLY_DISCONNECTED,

        /** */
        SERVER_EXPLICITLY_DISCONNECTED,

        // Things that can go wrong

        /** */
        CALL_MANAGER_IS_BUSY,

        /** */
        SFU_CLIENT_FAILED_TO_JOIN,

        /** */
        FAILED_TO_CREATE_PEER_CONNECTION_FACTORY,

        /** */
        FAILED_TO_GENERATE_CERTIFICATE,

        /** */
        FAILED_TO_CREATE_PEER_CONNECTION,

        /** */
        FAILED_TO_CREATE_DATA_CHANNEL,

        /** */
        FAILED_TO_START_PEER_CONNECTION,

        /** */
        FAILED_TO_UPDATE_PEER_CONNECTION,

        /** */
        FAILED_TO_SET_MAX_SEND_BITRATE,

        /** */
        ICE_FAILED_WHILE_CONNECTING,

        /** */
        ICE_FAILED_AFTER_CONNECTED,

        /** */
        SERVER_CHANGED_DEMUXID,

        /** */
        HAS_MAX_DEVICES;

        @CalledByNative
        static GroupCallEndReason fromNativeIndex(int nativeIndex) {
            return values()[nativeIndex];
        }
    }

    /**
     *
     */
    public class LocalDeviceState {
        ConnectionState connectionState;
        JoinState       joinState;
        boolean         audioMuted;
        boolean         videoMuted;

        public LocalDeviceState() {
            this.connectionState = ConnectionState.NOT_CONNECTED;
            this.joinState = JoinState.NOT_JOINED;
            this.audioMuted = true;
            this.videoMuted = true;
        }

        public LocalDeviceState(@NonNull LocalDeviceState localDeviceState) {
            this.connectionState = localDeviceState.connectionState;
            this.joinState = localDeviceState.joinState;
            this.audioMuted = localDeviceState.audioMuted;
            this.videoMuted = localDeviceState.videoMuted;
        }

        public ConnectionState getConnectionState() {
            return connectionState;
        }

        public JoinState getJoinState() {
            return joinState;
        }

        public boolean getAudioMuted() {
            return audioMuted;
        }

        public boolean getVideoMuted() {
            return videoMuted;
        }
    }

    /**
     *
     */
    public static class RemoteDeviceState {
                  long       demuxId;          // UInt32

        // The userId is set by the GroupCall object for delivery to the
        // application, after conversion from the RingRTC-provided byte
        // array.
        @Nullable UUID       userId;
        @NonNull  byte[]     userIdByteArray;

                  boolean    mediaKeysReceived;

        @Nullable Boolean    audioMuted;
        @Nullable Boolean    videoMuted;
        long                 addedTime;   // unix millis
        long                 speakerTime; // unix millis; 0 if was never the speaker

        @Nullable VideoTrack videoTrack;

        public RemoteDeviceState(          long    demuxId,
                                 @NonNull  byte[]  userIdByteArray,
                                           boolean mediaKeysReceived,
                                 @Nullable Boolean audioMuted,
                                 @Nullable Boolean videoMuted,
                                           long    addedTime,
                                           long    speakerTime) {
            this.demuxId = demuxId;
            this.userIdByteArray = userIdByteArray;
            this.mediaKeysReceived = mediaKeysReceived;

            this.audioMuted = audioMuted;
            this.videoMuted = videoMuted;
            this.addedTime = addedTime;
            this.speakerTime = speakerTime;
        }

        public long getDemuxId() {
            return demuxId;
        }

        // Marking as nullable although it should never be null when accessed
        // from the application.
        public @Nullable UUID getUserId() {
            return userId;
        }

        public boolean getMediaKeysReceived() {
            return mediaKeysReceived;
        }

        public @Nullable Boolean getAudioMuted() {
            return audioMuted;
        }

        public @Nullable Boolean getVideoMuted() {
            return videoMuted;
        }

        public long getAddedTime() {
            return addedTime;
        }

        public long getSpeakerTime() {
            return speakerTime;
        }

        public @Nullable VideoTrack getVideoTrack() {
            return videoTrack;
        }
    }

    /**
     *
     */
    public static class GroupMemberInfo {
        @NonNull  UUID   userId;
        // The userIdByteArray is set by the GroupCall object for delivery
        // to RingRTC, after conversion from the app-provided userId UUID.
        @Nullable byte[] userIdByteArray;
        @NonNull  byte[] userIdCipherText;

        public GroupMemberInfo(@NonNull UUID   userId,
                               @NonNull byte[] userIdCipherText) {
            this.userId = userId;
            this.userIdCipherText = userIdCipherText;
        }
    }

    /**
     *
     */
    public static class VideoRequest {
                  long    demuxId;   // UInt32
                  int     width;     // UInt16
                  int     height;    // UInt16
        @Nullable Integer framerate; // UInt16

        public VideoRequest(          long    demuxId,
                                            int     width,
                                            int     height,
                                  @Nullable Integer framerate) {
            this.demuxId = demuxId;
            this.width = width;
            this.height = height;
            this.framerate = framerate;
        }
    }

    /**
     *
     */
    public interface Observer {

        /**
         *
         */
        void requestMembershipProof(GroupCall groupCall);

        /**
         *
         */
        void requestGroupMembers(GroupCall groupCall);

        /**
         *
         */
        void onLocalDeviceStateChanged(GroupCall groupCall);

        /**
         *
         */
        void onRemoteDeviceStatesChanged(GroupCall groupCall);

        /**
         *
         */
        void onPeekChanged(GroupCall groupCall);

        /**
         *
         */
        void onEnded(GroupCall groupCall, GroupCallEndReason reason);
    }

    /* Native methods below here */

    private native
        long ringrtcCreateGroupCallClient(long   nativeCallManager,
                                          byte[] groupId,
                                          String sfuUrl,
                                          long   nativeAudioTrack,
                                          long   nativeVideoTrack)
        throws CallException;

    private native
        void ringrtcDeleteGroupCallClient(long nativeCallManager,
                                          long clientId)
        throws CallException;

    private native
        void ringrtcConnect(long nativeCallManager,
                            long clientId)
        throws CallException;

    private native
        void ringrtcJoin(long nativeCallManager,
                         long clientId)
        throws CallException;

    private native
        void ringrtcLeave(long nativeCallManager,
                          long clientId)
        throws CallException;

    private native
        void ringrtcDisconnect(long nativeCallManager,
                               long clientId)
        throws CallException;

    private native
        void ringrtcSetOutgoingAudioMuted(long nativeCallManager,
                                          long clientId,
                                          boolean muted)
        throws CallException;

    private native
        void ringrtcSetOutgoingVideoMuted(long nativeCallManager,
                                          long clientId,
                                          boolean muted)
        throws CallException;

    private native
        void ringrtcResendMediaKeys(long nativeCallManager,
                                    long clientId)
        throws CallException;

    private native
        void ringrtcSetBandwidthMode(long nativeCallManager,
                                     long clientId,
                                     int bandwidthMode)
        throws CallException;

    private native
        void ringrtcRequestVideo(long nativeCallManager,
                                           long clientId,
                                           List<VideoRequest> renderedResolutions)
        throws CallException;

    private native
        void ringrtcSetGroupMembers(long nativeCallManager,
                                    long clientId,
                                    List<GroupMemberInfo> members)
        throws CallException;

    private native
        void ringrtcSetMembershipProof(long nativeCallManager,
                                       long clientId,
                                       byte[] proof)
        throws CallException;
}
