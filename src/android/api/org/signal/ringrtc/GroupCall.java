/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
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

              private long                               nativeCallManager;
    @NonNull  private PeerConnectionFactory              factory;

    @NonNull  private Observer                           observer;

                      long                               clientId;

    // State to track if RingRTC has invoked handleEnded() or not.
    // RingRTC treats this as a final state of the GroupCall.
              private boolean                            handleEndedCalled;
    // State to track if the client has invoked disconnect() or not.
    // The client currently treats this as a final state of the GroupCall.
              private boolean                            disconnectCalled;

    // Whenever the local or remote device states are updated, a new
    // object will be created to update the object value.
    @NonNull  private LocalDeviceState                   localDeviceState;
    @NonNull  private LongSparseArray<RemoteDeviceState> remoteDeviceStates;

    @Nullable private PeekInfo                           peekInfo;

    @Nullable private AudioSource                        outgoingAudioSource;
    @Nullable private AudioTrack                         outgoingAudioTrack;
    @Nullable private VideoSource                        outgoingVideoSource;
    @Nullable private VideoTrack                         outgoingVideoTrack;
    @NonNull  private ArrayList<VideoTrack>              incomingVideoTracks;

    /*
     * Creates a GroupCall object. If successful, all supporting objects
     * will be valid. Otherwise, clientId will be 0.
     *
     * Should only be accessed via the CallManager.createGroupCall().
     *
     * If clientId is 0, the caller should invoke dispose() and let the
     * object itself get GC'd.
     */
    GroupCall(          long                  nativeCallManager,
              @NonNull  byte[]                groupId,
              @NonNull  String                sfuUrl,
              @NonNull  byte[]                hkdfExtraInfo,
              @Nullable Integer               audioLevelsIntervalMs,
              @NonNull  PeerConnectionFactory factory,
              @NonNull  Observer              observer) {
        Log.i(TAG, "GroupCall():");

        this.nativeCallManager = nativeCallManager;
        this.factory = factory;
        this.observer = observer;

        this.handleEndedCalled = false;
        this.disconnectCalled = false;

        this.localDeviceState = new LocalDeviceState();
        this.remoteDeviceStates = new LongSparseArray<>();

        MediaConstraints audioConstraints = new MediaConstraints();

        this.outgoingAudioSource = factory.createAudioSource(audioConstraints);
        if (this.outgoingAudioSource == null) {
            return;
        }

        // Note: This must stay "audio1" to stay in sync with CreateSessionDescriptionForGroupCall.
        this.outgoingAudioTrack = factory.createAudioTrack("audio1", this.outgoingAudioSource);
        if (this.outgoingAudioTrack == null) {
            return;
        } else {
            this.outgoingAudioTrack.setEnabled(false);
        }

        this.outgoingVideoSource = factory.createVideoSource(false);
        if (this.outgoingVideoSource == null) {
            return;
        }

        // Note: This must stay "video1" to stay in sync with CreateSessionDescriptionForGroupCall.
        this.outgoingVideoTrack = factory.createVideoTrack("video1", this.outgoingVideoSource);
        if (this.outgoingVideoTrack == null) {
            return;
        } else {
            this.outgoingVideoTrack.setEnabled(false);
        }

        // Define maximum output video format for group calls.
        this.outgoingVideoSource.adaptOutputFormat(640, 360, 30);

        this.incomingVideoTracks = new ArrayList<>();

        int audioLevelsIntervalMillis = audioLevelsIntervalMs == null ? 0 : audioLevelsIntervalMs.intValue();
        try {
            this.clientId = ringrtcCreateGroupCallClient(
                nativeCallManager,
                groupId,
                sfuUrl,
                hkdfExtraInfo,
                audioLevelsIntervalMillis,
                // Returns a borrowed RC.
                factory.getNativePeerConnectionFactory(),
                // Returns a borrowed RC.
                this.outgoingAudioTrack.getNativeAudioTrack(),
                // Returns a borrowed RC.
                this.outgoingVideoTrack.getNativeVideoTrack());
        } catch  (CallException e) {
            Log.w(TAG, "Unable to create group call client", e);
            throw new AssertionError("Unable to create group call client");
        }
    }

    /**
     * Releases native resources belonging to the object.
     */
    public void dispose()
        throws CallException
    {
        Log.i(TAG, "dispose():");

        if (this.clientId != 0) {
            ringrtcDeleteGroupCallClient(nativeCallManager, this.clientId);
            this.clientId = 0;
        }

        if (this.outgoingAudioTrack != null) {
            this.outgoingAudioTrack.dispose();
            this.outgoingAudioTrack = null;
        }

        if (this.outgoingVideoTrack != null) {
            this.outgoingVideoTrack.dispose();
            this.outgoingVideoTrack = null;
        }

        for (VideoTrack incomingTrack : incomingVideoTracks) {
            incomingTrack.dispose();
        }
    }

    /**
     *
     * Connects the group call to an SFU. The observer can now get
     * asynchronous requests for the membership proof and group
     * members, as well as regular updates of joined members.
     *
     * @throws CallException for native code failures
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
     * Joins the group call and begins media flow.
     *
     * @throws CallException for native code failures
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
     * Leaves the group call terminating media flow.
     *
     * @throws CallException for native code failures
     *
     */
    public void leave()
        throws CallException
    {
        Log.i(TAG, "leave():");

        // When leaving, make sure outgoing media is stopped as soon as possible.
        this.outgoingAudioTrack.setEnabled(false);
        this.outgoingVideoTrack.setEnabled(false);

        ringrtcLeave(nativeCallManager, this.clientId);
    }

    /**
     *
     * Disconnects the group call from an SFU. This will also leave the
     * group call if it is joined.
     *
     * @throws CallException for native code failures
     *
     */
    public void disconnect()
        throws CallException
    {
        Log.i(TAG, "disconnect():");

        // Protect against the client invoking disconnect() multiple times.
        if (!this.disconnectCalled) {
            this.disconnectCalled = true;

            if (this.handleEndedCalled) {
                // The handleEnded() callback has been called, so this is happening
                // after RingRTC is done. Resources can now be disposed.
                this.dispose();
            } else {
                // When disconnecting, make sure outgoing media is stopped as soon as possible.
                this.outgoingAudioTrack.setEnabled(false);
                this.outgoingVideoTrack.setEnabled(false);

                // The handleEnded() callback has not been called, so we can invoke
                // the RingRTC API to handle the disconnect, and resources will be
                // disposed later when handleEnded() is called.
                ringrtcDisconnect(nativeCallManager, this.clientId);
            }
        }
    }

    /**
     * Returns the LocalDeviceState tracked for the group call.
     */
    @NonNull
    public LocalDeviceState getLocalDeviceState()
    {
        Log.i(TAG, "getLocalDevice():");

        return this.localDeviceState;
    }

    /**
     * Returns an array of RemoteDeviceState objects as updated
     * from the SFU. Keyed by the demuxId.
     */
    @NonNull
    public LongSparseArray<RemoteDeviceState> getRemoteDeviceStates()
    {
        Log.i(TAG, "getRemoteDevices():");

        return this.remoteDeviceStates;
    }

    /**
     * Returns a PeekInfo object which holds the current state of the
     * group call from the SFU, including a collection of joined members
     * and other meta data.
     */
    @Nullable
    public PeekInfo getPeekInfo()
    {
        Log.i(TAG, "getPeekInfo():");

        return this.peekInfo;
    }

    /**
     *
     * Mute (or unmute) outgoing audio. This adjusts the outgoing audio
     * track and sends the status to the SFU.
     *
     * @param muted          true to mute, false to unmute
     *
     * @throws CallException for native code failures
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
     * Mute (or unmute) outgoing video. This adjusts the outgoing video
     * track and sends the status to the SFU. The camera capture state
     * is not affected and should be set accordingly by the application.
     *
     * @param muted          true to mute, false to unmute
     *
     * @throws CallException for native code failures
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
     * Links the camera to the outgoing video track.
     *
     * @param localSink      the sink to associate with the video track
     * @param cameraControl  the camera that will be used to capture video
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
     * Sends a ring request to everyone in the group.
     *
     * @throws CallException for native code failures
     *
     */
    public void ringAll()
        throws CallException
    {
        Log.i(TAG, "ring():");

        ringrtcRing(nativeCallManager, this.clientId, null);
    }

    /**
     *
     * Forces the group call object to send the latest media keys to
     * the SFU. This is useful when the application knows that a key
     * will have changed and needs the SFU to be updated.
     *
     * @throws CallException for native code failures
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
     * Allows the application to constrain bandwidth if so configured
     * by the user.
     *
     * @param bandwidthMode  one of the BandwidthMode enumerated values
     *
     * @throws CallException for native code failures
     *
     */
    public void setBandwidthMode(CallManager.BandwidthMode bandwidthMode)
        throws CallException
    {
        Log.i(TAG, "setBandwidthMode():");

        ringrtcSetBandwidthMode(nativeCallManager, this.clientId, bandwidthMode.ordinal());
    }

    /**
     *
     * Provides a collection of VideoRequest objects to the group call
     * object which are sent to the SFU. This allows the appropriate
     * video resolution to be sent from the SFU to efficiently fit in
     * rendered resolution on the screen.
     *
     * @param resolutions         the VideoRequest objects for each user rendered on the screen
     * @param activeSpeakerHeight the height of the view for the active speaker, in pixels
     *
     * @throws CallException for native code failures
     *
     */
    public void requestVideo(@NonNull Collection<VideoRequest> resolutions, int activeSpeakerHeight)
        throws CallException
    {
        Log.i(TAG, "requestVideo():");

        ringrtcRequestVideo(nativeCallManager,
                            this.clientId,
                            new ArrayList<>(resolutions),
                            activeSpeakerHeight);
    }

    /**
     *
     * Provides a collection of GroupMemberInfo objects representing all
     * the possible members of a group.
     *
     * @param members        a GroupMemberInfo object for each member in a group
     *
     * @throws CallException for native code failures
     *
     */
    public void setGroupMembers(@NonNull Collection<GroupMemberInfo> groupMembers)
        throws CallException
    {
        Log.i(TAG, "setGroupMembers():");

        ringrtcSetGroupMembers(nativeCallManager, this.clientId, Util.serializeFromGroupMemberInfo(groupMembers));
    }

    /**
     *
     * Proves that the user is a member of the group.
     *
     * @param proof          byte array containing the proof
     *
     * @throws CallException for native code failures
     *
     */
    public void setMembershipProof(@NonNull byte[] proof)
        throws CallException
    {
        Log.i(TAG, "setMembershipProof():");

        ringrtcSetMembershipProof(nativeCallManager, this.clientId, proof);
    }

    /**
     *
     * Callback from RingRTC when the group call object needs an updated
     * membership proof. Called via the CallManager.
     *
     */
    void requestMembershipProof() {
        Log.i(TAG, "requestMembershipProof():");

        this.observer.requestMembershipProof(this);
    }

    /**
     *
     * Callback from RingRTC when the group call object needs an updated
     * list of group members. Called via the CallManager.
     *
     */
    void requestGroupMembers() {
        Log.i(TAG, "handleGroupMembers():");

        this.observer.requestGroupMembers(this);
    }

    /**
     *
     * Callback from RingRTC when the connection state changes. Called
     * via the CallManager.
     *
     */
    void handleConnectionStateChanged(ConnectionState connectionState) {
        Log.i(TAG, "handleConnectionStateChanged():");

        LocalDeviceState localDeviceState = new LocalDeviceState(this.localDeviceState);
        localDeviceState.connectionState = connectionState;

        this.localDeviceState = localDeviceState;

        this.observer.onLocalDeviceStateChanged(this);
    }

    /**
     *
     * Callback from RingRTC when the join state changes. Called via
     * the CallManager.
     *
     */
    void handleJoinStateChanged(JoinState joinState) {
        Log.i(TAG, "handleJoinStateChanged():");

        LocalDeviceState localDeviceState = new LocalDeviceState(this.localDeviceState);
        localDeviceState.joinState = joinState;

        this.localDeviceState = localDeviceState;

        this.observer.onLocalDeviceStateChanged(this);
    }

    /**
     *
     * Callback from RingRTC with details about a network route change.
     *
     */
    void handleNetworkRouteChanged(NetworkRoute networkRoute) {
        Log.i(TAG, "handleNetworkRouteChanged():");

        LocalDeviceState localDeviceState = new LocalDeviceState(this.localDeviceState);
        localDeviceState.networkRoute = networkRoute;

        this.localDeviceState = localDeviceState;

        this.observer.onLocalDeviceStateChanged(this);
    }

    /**
     *
     * Callback from RingRTC with details about audio levels.
     *
     */
    void handleAudioLevels(int capturedLevel, List<ReceivedAudioLevel> receivedLevels) {
        this.localDeviceState.audioLevel = capturedLevel;
        for (ReceivedAudioLevel received : receivedLevels) {
            RemoteDeviceState remoteDeviceState = this.remoteDeviceStates.get(received.demuxId);
            if (remoteDeviceState != null) {
                remoteDeviceState.audioLevel = received.level;
            }
        }

        this.observer.onAudioLevels(this);
    }

    /**
     *
     * Callback from RingRTC when the remote device states have changed.
     * Called via the CallManager.
     *
     */
    void handleRemoteDevicesChanged(List<RemoteDeviceState> remoteDeviceStates) {
        Log.i(TAG, "handleRemoteDevicesChanged():");

        LongSparseArray<RemoteDeviceState> remoteDeviceByDemuxId = new LongSparseArray<>();
        for (RemoteDeviceState remoteDeviceState : remoteDeviceStates) {
            // Convert each userIdByteArray to userId UUID.
            remoteDeviceState.userId = Util.getUuidFromBytes(remoteDeviceState.userIdByteArray);

            // Maintain the video track and audio level if one already exists.
            RemoteDeviceState existingDeviceState = this.remoteDeviceStates.get(remoteDeviceState.demuxId);
            if (existingDeviceState != null) {
                remoteDeviceState.videoTrack = existingDeviceState.videoTrack;
                remoteDeviceState.audioLevel = existingDeviceState.audioLevel;
            }

            // Build the mapped version of the array with demuxId as the key.
            remoteDeviceByDemuxId.put(remoteDeviceState.demuxId, remoteDeviceState);
        }

        this.remoteDeviceStates = remoteDeviceByDemuxId;

        this.observer.onRemoteDeviceStatesChanged(this);
    }

    /**
     *
     * Callback from RingRTC with details about a new video track that can be
     * rendered for a specific member (by demuxId). Called via the CallManager.
     *
     */
    void handleIncomingVideoTrack(long remoteDemuxId, long nativeVideoTrackOwnedRc) {
        Log.i(TAG, "handleIncomingVideoTrack():");

        if (nativeVideoTrackOwnedRc == 0) {
            Log.d(TAG, "nativeVideoTrackOwnedRc is null (0)");
            return;
        }

        RemoteDeviceState remoteDeviceState = this.remoteDeviceStates.get(remoteDemuxId);
        if (remoteDeviceState == null) {
            Log.d(TAG, "No remote device state found for remoteDemuxId");
            return;
        }

        remoteDeviceState.videoTrack = new VideoTrack(nativeVideoTrackOwnedRc);
        this.incomingVideoTracks.add(remoteDeviceState.videoTrack);
        this.observer.onRemoteDeviceStatesChanged(this);
    }

    /**
     *
     * Callback from RingRTC that the PeekInfo changed with new information
     * about the members in the group call. Called via the CallManager.
     *
     */
    void handlePeekChanged(PeekInfo info) {
        Log.i(TAG, "handlePeekChanged():");

        this.peekInfo = info;

        this.observer.onPeekChanged(this);
    }

    /**
     *
     * Callback from RingRTC when the group call ends. Called via the
     * CallManager.
     *
     */
    void handleEnded(GroupCallEndReason reason) {
        Log.i(TAG, "handleEnded():");

        // This check is not strictly necessary since RingRTC should only be
        // calling handleEnded() once.
        if (!this.handleEndedCalled) {
            this.handleEndedCalled = true;

            this.observer.onEnded(this, reason);

            try {
                if (this.disconnectCalled) {
                    // The disconnect() API has been called, so this is happening
                    // after the client side is done. Resources can now be disposed.
                    this.dispose();
                }
            } catch (CallException e) {
                Log.w(TAG, "Unable to delete group call clientId: " + this.clientId, e);
            }
        }
    }

    /**
     * The connection states of a device connecting to a group call.
     */
    public enum ConnectionState {

        /** connect() has not yet been called or disconnect() has been called or connect() was called but failed. */
        NOT_CONNECTED,

        /** connect() has been called but connectivity is pending. */
        CONNECTING,

        /** connect() has been called and connectivity has been established. */
        CONNECTED,

        /** connect() has been called and a connection has been established, but the connectivity is temporarily failing. */
        RECONNECTING;

        @CalledByNative
        static ConnectionState fromNativeIndex(int nativeIndex) {
            return values()[nativeIndex];
        }
    }

    /**
     * The join states of a device joining a group call.
     */
    public enum JoinState {

        /** join() has not yet been called or leave() has been called or join() was called but failed. */
        NOT_JOINED,

        /** join() has been called but a response from the SFU is pending. */
        JOINING,

        /** join() has been called and a response from the SFU has been received and a demuxId has been assigned.. */
        JOINED;

        @CalledByNative
        static JoinState fromNativeIndex(int nativeIndex) {
            return values()[nativeIndex];
        }
    }

    /**
     * A set of reasons why the group call has ended.
     */
    public enum GroupCallEndReason {

        // Normal events

        /** The client disconnected by calling the disconnect() API. */
        DEVICE_EXPLICITLY_DISCONNECTED,

        /** The server disconnected due to policy or some other controlled reason. */
        SERVER_EXPLICITLY_DISCONNECTED,

        // Things that can go wrong

        /** Another direct call or group call is currently in progress and using media resources. */
        CALL_MANAGER_IS_BUSY,

        /** Could not join the group call. */
        SFU_CLIENT_FAILED_TO_JOIN,

        /** Could not create a usable peer connection factory for media. */
        FAILED_TO_CREATE_PEER_CONNECTION_FACTORY,

        /** Could not negotiate SRTP keys with a DHE. */
        FAILED_TO_NEGOTIATE_SRTP_KEYS,

        /** Could not create a peer connection for media. */
        FAILED_TO_CREATE_PEER_CONNECTION,

        /** Could not start the peer connection for media. */
        FAILED_TO_START_PEER_CONNECTION,

        /** Could not update the peer connection for media. */
        FAILED_TO_UPDATE_PEER_CONNECTION,

        /** Could not set the requested bitrate for media. */
        FAILED_TO_SET_MAX_SEND_BITRATE,

        /** Could not connect successfully. */
        ICE_FAILED_WHILE_CONNECTING,

        /** Lost a connection and retries were unsuccessful. */
        ICE_FAILED_AFTER_CONNECTED,

        /** Unexpected change in demuxId requiring a new group call. */
        SERVER_CHANGED_DEMUXID,

        /** The SFU reported that the group call is full. */
        HAS_MAX_DEVICES;

        @CalledByNative
        static GroupCallEndReason fromNativeIndex(int nativeIndex) {
            return values()[nativeIndex];
        }
    }

    /**
     * A convenience class grouping together all the local state.
     */
    public class LocalDeviceState {
        ConnectionState connectionState;
        JoinState       joinState;
        boolean         audioMuted;
        boolean         videoMuted;
        NetworkRoute    networkRoute;
        int             audioLevel;

        public LocalDeviceState() {
            this.connectionState = ConnectionState.NOT_CONNECTED;
            this.joinState = JoinState.NOT_JOINED;
            this.audioMuted = true;
            this.videoMuted = true;
            this.networkRoute = new NetworkRoute();
            this.audioLevel = 0;
        }

        public LocalDeviceState(@NonNull LocalDeviceState localDeviceState) {
            this.connectionState = localDeviceState.connectionState;
            this.joinState = localDeviceState.joinState;
            this.audioMuted = localDeviceState.audioMuted;
            this.videoMuted = localDeviceState.videoMuted;
            this.networkRoute = localDeviceState.networkRoute;
            this.audioLevel = localDeviceState.audioLevel;
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

        public NetworkRoute getNetworkRoute() {
            return networkRoute;
        }

        // Range of 0-32767, where 0 is silence.
        public int getAudioLevel() {
            return audioLevel;
        }
    }

    /**
     * The state of each remote member in a group call.
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
        @Nullable Boolean    presenting;
        @Nullable Boolean    sharingScreen;
        long                 addedTime;   // unix millis
        long                 speakerTime; // unix millis; 0 if was never the speaker
        @Nullable Boolean    forwardingVideo;
                  boolean    isHigherResolutionPending;

        @Nullable VideoTrack videoTrack;
        @NonNull  int        audioLevel;

        public RemoteDeviceState(          long    demuxId,
                                 @NonNull  byte[]  userIdByteArray,
                                           boolean mediaKeysReceived,
                                 @Nullable Boolean audioMuted,
                                 @Nullable Boolean videoMuted,
                                 @Nullable Boolean presenting,
                                 @Nullable Boolean sharingScreen,
                                           long    addedTime,
                                           long    speakerTime,
                                 @Nullable Boolean forwardingVideo,
                                           boolean isHigherResolutionPending) {
            this.demuxId = demuxId;
            this.userIdByteArray = userIdByteArray;
            this.mediaKeysReceived = mediaKeysReceived;

            this.audioMuted = audioMuted;
            this.videoMuted = videoMuted;
            this.presenting = presenting;
            this.sharingScreen = sharingScreen;
            this.addedTime = addedTime;
            this.speakerTime = speakerTime;
            this.forwardingVideo = forwardingVideo;
            this.isHigherResolutionPending = isHigherResolutionPending;
            this.audioLevel = 0;
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

        public @Nullable Boolean getPresenting() {
            return presenting;
        }

        public @Nullable Boolean getSharingScreen() {
            return sharingScreen;
        }

        public long getAddedTime() {
            return addedTime;
        }

        public long getSpeakerTime() {
            return speakerTime;
        }

        public @Nullable Boolean getForwardingVideo() {
            return forwardingVideo;
        }

        public boolean isHigherResolutionPending() {
            return isHigherResolutionPending;
        }

        public @Nullable VideoTrack getVideoTrack() {
            return videoTrack;
        }

        // Range of 0-32767, where 0 is silence.
        public int getAudioLevel() {
            return audioLevel;
        }
    }

    /**
    *
    * A way to pass a list of (demuxId, level) through the FFI.
    *
    */
    public static class ReceivedAudioLevel {
        public long demuxId;
        public int level;  // Range of 0-32767, where 0 is silence

        public ReceivedAudioLevel(long demuxId, int level) {
            this.demuxId = demuxId;
            this.level = level;
        }
    }

    /**
     * A class grouping each member's opaque cipher text and their UUID.
     */
    public static class GroupMemberInfo {
        @NonNull  UUID   userId;
        @NonNull  byte[] userIdCipherText;

        public GroupMemberInfo(@NonNull UUID   userId,
                               @NonNull byte[] userIdCipherText) {
            this.userId = userId;
            this.userIdCipherText = userIdCipherText;
        }
    }

    /**
     * A class used to convey how each member is rendered on the screen.
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
     * The client must provide an observer for each group call object
     * which is used to convey callbacks and notifications from
     * RingRTC.
     */
    public interface Observer {

        /**
         * Notification that the group call object needs an updated membership proof.
         */
        void requestMembershipProof(GroupCall groupCall);

        /**
         * Notification that the group call object needs an updated list of group members.
         */
        void requestGroupMembers(GroupCall groupCall);

        /**
         * Notification that the local device state has changed.
         */
        void onLocalDeviceStateChanged(GroupCall groupCall);

        /**
         * Notification of audio levels.
         */
        void onAudioLevels(GroupCall groupCall);

        /**
         * Notification that the remote device states have changed.
         */
        void onRemoteDeviceStatesChanged(GroupCall groupCall);

        /**
         * Notification that the PeekInfo changed.
         */
        void onPeekChanged(GroupCall groupCall);

        /**
         * Notification that the group call has ended.
         */
        void onEnded(GroupCall groupCall, GroupCallEndReason reason);
    }

    /* Native methods below here. */

    private native
        long ringrtcCreateGroupCallClient(long nativeCallManager,
                                          byte[] groupId,
                                          String sfuUrl,
                                          byte[] hkdfExtraInfo,
                                          int audioLevelsIntervalMillis,
                                          long nativePeerConnectionFactory,
                                          long nativeAudioTrack,
                                          long nativeVideoTrack)
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
        void ringrtcRing(          long   nativeCallManager,
                                   long   clientId,
                         @Nullable byte[] recipient)
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
                                 List<VideoRequest> renderedResolutions,
                                 int activeSpeakerHeight)
        throws CallException;

    private native
        void ringrtcSetGroupMembers(long nativeCallManager,
                                    long clientId,
                                    byte[] serializedGroupMembers)
        throws CallException;

    private native
        void ringrtcSetMembershipProof(long nativeCallManager,
                                       long clientId,
                                       byte[] proof)
        throws CallException;
}
