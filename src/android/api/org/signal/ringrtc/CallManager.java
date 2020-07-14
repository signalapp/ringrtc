/*
 *
 *  Copyright (C) 2019, 2020 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

package org.signal.ringrtc;

import android.content.Context;
import android.os.Build;
import androidx.annotation.NonNull;
import androidx.annotation.Nullable;

import org.webrtc.AudioSource;
import org.webrtc.AudioTrack;
import org.webrtc.DefaultVideoDecoderFactory;
import org.webrtc.DefaultVideoEncoderFactory;
import org.webrtc.SoftwareVideoEncoderFactory;
import org.webrtc.EglBase;
import org.webrtc.Logging.Severity;
import org.webrtc.MediaConstraints;
import org.webrtc.MediaStream;
import org.webrtc.NativeLibraryLoader;
import org.webrtc.PeerConnection;
import org.webrtc.PeerConnectionFactory;
import org.webrtc.RtcCertificatePem;
import org.webrtc.VideoDecoderFactory;
import org.webrtc.VideoEncoderFactory;
import org.webrtc.VideoSource;
import org.webrtc.VideoTrack;
import org.webrtc.VideoSink;

import java.util.HashSet;
import java.util.Set;
import java.util.List;

/**
 *
 * Provides an interface to the RingRTC Call Manager.
 *
 */
public class CallManager {

  @NonNull
  private static final String   TAG = CallManager.class.getSimpleName();
  private static       boolean  isInitialized;

  private              long     nativeCallManager;
  @NonNull
  private              Observer observer;

  static {
    if (Build.VERSION.SDK_INT < 21) {
      Log.i(TAG, "Preloading ringrtc_rffi library for SDK: " + Build.VERSION.SDK_INT);
      System.loadLibrary("ringrtc_rffi");
    }

    Log.d(TAG, "Loading ringrtc library");
    System.loadLibrary("ringrtc");
  }

  /**
   *
   * Global one time system initialization
   *
   * <p>This method is called once from an application's initialization code.
   *
   * @param applicationContext  The global application context
   * @param logger              An instance of the package specific logger class
   */
  public static void initialize(Context applicationContext, Log.Logger logger) {

    try {
      Log.initialize(logger);

      PeerConnectionFactory.InitializationOptions.Builder builder = PeerConnectionFactory.InitializationOptions.builder(applicationContext)
        .setNativeLibraryLoader(new NoOpLoader());

      BuildInfo buildInfo = ringrtcGetBuildInfo();
      Log.i(TAG, "CallManager.initialize(): (" + (buildInfo.debug ? "debug" : "release") + " build)");

      if (buildInfo.debug) {
        // Show WebRTC logs via application Logger while debugging.
        builder.setInjectableLogger(new WebRtcLogger(), Severity.LS_INFO);
      }

      PeerConnectionFactory.initialize(builder.createInitializationOptions());
      ringrtcInitialize();
      CallManager.isInitialized = true;
      Log.i(TAG, "CallManager.initialize() returned");
    } catch (UnsatisfiedLinkError e) {
      Log.w(TAG, "Unable to load ringrtc library", e);
      throw new AssertionError("Unable to load ringrtc library");
    } catch  (CallException e) {
      Log.w(TAG, "Unable to initialize ringrtc library", e);
      throw new AssertionError("Unable to initialize ringrtc library");
    }

  }

  private static void checkInitializeHasBeenCalled() {
    if (!CallManager.isInitialized) {
      throw new IllegalStateException("CallManager.initialize has not been called");
    }
  }

  private void checkCallManagerExists() {
    if (nativeCallManager == 0) {
      throw new IllegalStateException("CallManager has been disposed.");
    }
  }

  CallManager(@NonNull Observer observer) {
    Log.i(TAG, "CallManager():");

    this.observer          = observer;
    this.nativeCallManager = 0;
  }

  @Nullable
  public static CallManager createCallManager(@NonNull Observer observer)
    throws CallException
  {
    Log.i(TAG, "createCallManager():");
    checkInitializeHasBeenCalled();

    CallManager callManager = new CallManager(observer);

    long nativeCallManager = ringrtcCreateCallManager(callManager);
    if (nativeCallManager != 0) {
      callManager.nativeCallManager = nativeCallManager;
      return callManager;
    } else {
      Log.w(TAG, "Unable to create Call Manager");
      return null;
    }

  }

  /**
   *
   * Notification from application to close down the call manager.
   *
   */
  public void close()
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "close():");
    ringrtcClose(nativeCallManager);
    nativeCallManager = 0;
  }

  /**
   *
   * Indication from application to start a new outgoing call
   *
   * @param remote         remote side fo the call
   * @param callMediaType  used to specify origination as an audio or video call
   * @param localDevice     the local deviceId of the client
   *
   * @throws CallException for native code failures
   *
   */
  public void call(         Remote        remote,
                   @NonNull CallMediaType callMediaType,
                   @NonNull Integer       localDevice)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "call(): creating new call:");

    ringrtcCall(nativeCallManager, remote, callMediaType.ordinal(), localDevice);
  }

  /**
   *
   * Indication from application to proceed with call
   *
   * @param callId        callId for the call
   * @param context       Call service context
   * @param eglBase       eglBase to use for this Call
   * @param localSink     local video sink to use for this Call
   * @param remoteSink    remote video sink to use for this Call
   * @param camera        camera control to use for this Call
   * @param iceServers    list of ICE servers to use for this Call
   * @param hideIp        if true hide caller's IP by using a TURN server
   * @param enableCamera  if true, enable the local camera video track when created
   *
   * @throws CallException for native code failures
   *
   */
  public void proceed(@NonNull CallId                         callId,
                      @NonNull Context                        context,
                      @NonNull EglBase                        eglBase,
                      @NonNull VideoSink		                  localSink,
                      @NonNull VideoSink                      remoteSink,
                      @NonNull CameraControl                  camera,
                      @NonNull List<PeerConnection.IceServer> iceServers,
                               boolean                        hideIp,
                               boolean                        enableCamera)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "proceed(): callId: " + callId + ", hideIp: " + hideIp);

    // Defaults to ECDSA, which should be fast.
    RtcCertificatePem certificate = RtcCertificatePem.generateCertificate();
    CallContext callContext = new CallContext(callId,
                                              context,
                                              eglBase,
                                              localSink,
                                              remoteSink,
                                              camera,
                                              iceServers,
                                              hideIp,
                                              certificate);

    callContext.setVideoEnabled(enableCamera);
    ringrtcProceed(nativeCallManager,
                   callId.longValue(),
                   callContext);

  }

  /**
   *
   * Indication from application to drop the active call, without
   * proceeding.
   *
   * @param callId   callId for the call
   *
   * @throws CallException for native code failures
   *
   */
  public void drop(@NonNull CallId callId)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "drop(): " + callId);
    ringrtcDrop(nativeCallManager, callId.longValue());
  }

  /**
   *
   * Indication from application to completely reset the call manager.
   * This will close out any outstanding calls and return the Call
   * Manager to a freshly initialized state.
   *
   * @throws CallException for native code failures
   *
   */
  public void reset()
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "reset():");
    ringrtcReset(nativeCallManager);
  }

  /**
   *
   * Indication from application that signaling message was sent successfully
   *
   * @param callId  callId for the call
   *
   * @throws CallException for native code failures
   *
   */
  public void messageSent(@NonNull CallId callId)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "messageSent(): " + callId);
    ringrtcMessageSent(nativeCallManager, callId.longValue());
  }

  /**
   *
   * Indication from application that signaling message was not sent successfully
   *
   * @param callId  callId for the call
   *
   * @throws CallException for native code failures
   *
   */
  public void messageSendFailure(@NonNull CallId callId)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "messageSendFailure(): " + callId);
    ringrtcMessageSendFailure(nativeCallManager, callId.longValue());
  }

  /**
   *
   * Notification from application of a received SDP Offer
   *
   * This is the beginning of an incoming call.
   *
   * @param callId                   callId for the call
   * @param remote                   remote side fo the call
   * @param remoteDevice             deviceId of remote peer
   * @param opaque                   the opaque offer
   * @param sdp                      the SDP offer (depreacated/legacy)
   * @param messageAgeSec            approximate age of the offer message, in seconds
   * @param callMediaType            the origination type for the call, audio or video
   * @param localDevice              the local deviceId of the client
   * @param remoteSupportsMultiRing  if true, the remote device supports the multi-ring feature
   * @param isLocalDevicePrimary     if true, the local device is considered a primary device
   *
   * @throws CallException for native code failures
   *
   */
  public void receivedOffer(CallId           callId,
                            Remote           remote,
                            Integer          remoteDevice,
                            @Nullable byte[] opaque,
                            @Nullable String sdp,
                            Long             messageAgeSec,
                            CallMediaType    callMediaType,
                            Integer          localDevice,
                            boolean          remoteSupportsMultiRing,
                            boolean          isLocalDevicePrimary)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "receivedOffer(): id: " + callId.format(remoteDevice));

    ringrtcReceivedOffer(nativeCallManager,
                         callId.longValue(),
                         remote,
                         remoteDevice.intValue(),
                         opaque,
                         sdp,
                         messageAgeSec.longValue(),
                         callMediaType.ordinal(),
                         localDevice,
                         remoteSupportsMultiRing,
                         isLocalDevicePrimary);
  }

  /**
   *
   * Notification from application of a received SDP Answer
   *
   * @param callId                   callId for the call
   * @param remoteDevice             deviceId of remote peer
   * @param opaque                   the opaque answer
   * @param sdp                      the SDP answer (depreacated/legacy)
   * @param remoteSupportsMultiRing  if true, the remote device supports the multi-ring feature
   *
   * @throws CallException for native code failures
   *
   */
  public void receivedAnswer(CallId           callId,
                             Integer          remoteDevice,
                             @Nullable byte[] opaque,
                             @Nullable String sdp,
                             boolean          remoteSupportsMultiRing)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "receivedAnswer(): id: " + callId.format(remoteDevice));
    ringrtcReceivedAnswer(nativeCallManager,
                          callId.longValue(),
                          remoteDevice.intValue(),
                          opaque,
                          sdp,
                          remoteSupportsMultiRing);
  }

  /**
   *
   * Notification from application of received ICE candidates
   *
   * @param callId         callId for the call
   * @param remoteDevice   deviceId of remote peer
   * @param iceCandidates  list of Ice Candidates
   *
   * @throws CallException for native code failures
   *
   */
  public void receivedIceCandidates(CallId callId, Integer remoteDevice, List<IceCandidate> iceCandidates)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "receivedIceCandidates(): id: " + callId.format(remoteDevice) + ", count: " + iceCandidates.size());
    ringrtcReceivedIceCandidates(nativeCallManager,
                                 callId.longValue(),
                                 remoteDevice.intValue(),
                                 iceCandidates);
  }

  /**
   *
   * Notification from application of received Hangup message
   *
   * @param callId        callId for the call
   * @param remoteDevice  deviceId of remote peer
   * @param hangupType    type of hangup, normal or handled elsewhere
   * @param deviceId      if not a normal hangup, the associated deviceId
   *
   * @throws CallException for native code failures
   *
   */
  public void receivedHangup(CallId     callId,
                             Integer    remoteDevice,
                             HangupType hangupType,
                             Integer    deviceId)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "receivedHangup(): id: " + callId.format(remoteDevice));
    ringrtcReceivedHangup(nativeCallManager,
                          callId.longValue(),
                          remoteDevice.intValue(),
                          hangupType.ordinal(),
                          deviceId.intValue());
  }

  /**
   *
   * Notification from application of received Busy message
   *
   * @param callId        callId for the call
   * @param remoteDevice  deviceId of remote peer
   *
   * @throws CallException for native code failures
   *
   */
  public void receivedBusy(CallId callId, Integer remoteDevice)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "receivedBusy(): id: " + callId.format(remoteDevice));
    ringrtcReceivedBusy(nativeCallManager,
                        callId.longValue(),
                        remoteDevice.intValue());
  }

  /**
   *
   * Indication from application to accept the active call.
   *
   * @param callId  callId for the call
   *
   * @throws CallException for native code failures
   *
   */
  public void acceptCall(@NonNull CallId callId)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "accept(): " + callId);
    ringrtcAcceptCall(nativeCallManager, callId.longValue());
  }

  /**
   *
   * Notification from application to enable audio playback of remote
   * audio stream and enable recording of local audio stream.
   *
   * @throws CallException for native code failures
   *
   */
  public void setCommunicationMode()
    throws CallException
  {
    checkCallManagerExists();

    Connection connection = ringrtcGetActiveConnection(nativeCallManager);
    connection.setAudioPlayout(true);
    connection.setAudioRecording(true);

  }

  /**
   *
   * Notification from application to enable/disable local audio
   * recording and transmission.
   *
   * @param enable       if true, then enable local audio recording and transmission
   *
   * @throws CallException for native code failures
   *
   */
  public void setAudioEnable(boolean enable)
    throws CallException
  {
    checkCallManagerExists();

    Connection connection = ringrtcGetActiveConnection(nativeCallManager);
    connection.setAudioEnabled(enable);
  }

  /**
   *
   * Notification from application to enable/disable local video
   * recording and transmission.
   *
   * @param enable   if true, then enable local video recording and transmission
   *
   * @throws CallException for native code failures
   *
   */
  public void setVideoEnable(boolean enable)
    throws CallException
  {
    checkCallManagerExists();

    CallContext callContext = ringrtcGetActiveCallContext(nativeCallManager);
    callContext.setVideoEnabled(enable);

    ringrtcSetVideoEnable(nativeCallManager, enable);
  }

  /**
   *
   * Notification from application to enable/disable the low bandwidth
   * mode for the session.
   *
   * @param enabled  if true, then enable low bandwidth mode
   *
   * @throws CallException for native code failures
   *
   */
  public void setLowBandwidthMode(boolean enabled)
    throws CallException
  {
    checkCallManagerExists();

    ringrtcSetLowBandwidthMode(nativeCallManager, enabled);
  }

  /**
   *
   * Notification from application to hangup the active call.
   *
   * @throws CallException for native code failures
   *
   */
  public void hangup()
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "hangup():");
    ringrtcHangup(nativeCallManager);
  }

  /****************************************************************************
   *
   * Below are only called by the native CM
   *
   */

  /**
   *
   * Envoked by native CallManager to create a Connection object.
   *
   */
  @CalledByNative
  @Nullable
  private Connection createConnection(long        nativeConnection,
                                      long        nativeCallId,
                                      int         remoteDevice,
                                      CallContext callContext,
                                      boolean     enableDtls,
                                      boolean     enableRtpDataChannel) {

    CallId callId = new CallId(nativeCallId);

    Log.i(TAG, "createConnection(): connectionId: " + callId.format(remoteDevice));

    MediaConstraints                constraints   = new MediaConstraints();
    PeerConnection.RTCConfiguration configuration = new PeerConnection.RTCConfiguration(callContext.iceServers);

    configuration.bundlePolicy  = PeerConnection.BundlePolicy.MAXBUNDLE;
    configuration.rtcpMuxPolicy = PeerConnection.RtcpMuxPolicy.REQUIRE;
    configuration.tcpCandidatePolicy = PeerConnection.TcpCandidatePolicy.DISABLED;

    if (callContext.hideIp) {
      configuration.iceTransportsType = PeerConnection.IceTransportsType.RELAY;
    }
    configuration.certificate = callContext.certificate;

    configuration.enableDtlsSrtp = enableDtls;
    configuration.enableRtpDataChannel = enableRtpDataChannel;

    constraints.optional.add(new MediaConstraints.KeyValuePair("DtlsSrtpKeyAgreement", "true"));

    PeerConnectionFactory factory       = callContext.peerConnectionFactory;
    CameraControl         cameraControl = callContext.cameraControl;
    try {
      long nativePeerConnection = ringrtcCreatePeerConnection(factory.getNativeOwnedFactoryAndThreads(),
                                                              nativeConnection,
                                                              configuration,
                                                              constraints);
      if (nativePeerConnection == 0) {
        Log.w(TAG, "Unable to create native PeerConnection.");
        return null;
      }

      Connection connection = new Connection(new Connection.NativeFactory(nativePeerConnection,
                                                                          callId,
                                                                          remoteDevice));

      connection.setAudioPlayout(false);
      connection.setAudioRecording(false);

      MediaStream      mediaStream      = factory.createLocalMediaStream("ARDAMS");
      MediaConstraints audioConstraints = new MediaConstraints();

      audioConstraints.optional.add(new MediaConstraints.KeyValuePair("DtlsSrtpKeyAgreement", "true"));
      AudioSource audioSource = factory.createAudioSource(audioConstraints);
      AudioTrack  audioTrack  = factory.createAudioTrack("ARDAMSa0", audioSource);
      audioTrack.setEnabled(false);
      mediaStream.addTrack(audioTrack);

      if (callContext.videoTrack != null) {
        // We are sharing a single videoTrack with all the media
        // streams.  As such, use addPreservedTrack() here so that
        // when this MediaStream is disposed() the VideoTrack remains.
        // We need to explicitly dispose() the VideoTrack at call
        // termination.
        mediaStream.addPreservedTrack(callContext.videoTrack);
      }

      connection.addStream(mediaStream);

      connection.setAudioSource(audioSource,
                                audioTrack);

      return connection;

    } catch  (CallException e) {
      Log.w(TAG, "Unable to create Peer Connection with native call", e);
      return null;
    }
  }

  /**
   *
   * Envoked by native CallManager when a remote "accepts" on a
   * Connection, allowing MediaStream of connection to attach to
   * application renderers and play back devices.
   *
   */
  @CalledByNative
  private void onConnectMedia(@NonNull CallContext callContext,
                              @NonNull MediaStream mediaStream)
  {
    Log.i(TAG, "onConnectMedia(): mediaStream: " + mediaStream);

    if (mediaStream == null) {
      Log.w(TAG, "Remote media stream unavailable");
      return;
    }

    if (mediaStream.audioTracks == null) {
      Log.w(TAG, "Remote media stream contains no audio tracks");
      return;
    }

    for (AudioTrack remoteAudioTrack : mediaStream.audioTracks) {
      Log.i(TAG, "onConnectMedia(): enabling audioTrack");
      remoteAudioTrack.setEnabled(true);
    }

    if (mediaStream.videoTracks == null) {
      Log.w(TAG, "Remote media stream contains no video tracks");
      return;
    }

    if (mediaStream.videoTracks.size() == 1) {
      Log.i(TAG, "onConnectMedia(): enabling videoTrack(0)");
      VideoTrack remoteVideoTrack = mediaStream.videoTracks.get(0);
      remoteVideoTrack.setEnabled(true);
      remoteVideoTrack.addSink(callContext.remoteSink);
    } else {
      Log.w(TAG, "onConnectMedia(): Media stream contains unexpected number of video tracks: " + mediaStream.videoTracks.size());
    }

  }

  /**
   *
   * Envoked by native CallManager when closing down a call to
   * shutdown media.
   *
   */
  @CalledByNative
  private void onCloseMedia(@NonNull CallContext callContext)
  {
    Log.i(TAG, "onCloseMedia():");
    callContext.setVideoEnabled(false);
  }

  @CalledByNative
  private void closeConnection(Connection connection) {
    Log.i(TAG, "closeConnection(): " + connection);
    connection.shutdown();
  }

  @CalledByNative
  private void closeCall(@NonNull CallContext callContext) {
    Log.i(TAG, "closeCall():");
    callContext.dispose();
  }

  @CalledByNative
  private void onStartCall(Remote remote, long callId, boolean isOutgoing, CallMediaType callMediaType) {
    Log.i(TAG, "onStartCall():");
    observer.onStartCall(remote, new CallId(callId), Boolean.valueOf(isOutgoing), callMediaType);
  }

  @CalledByNative
  private void onEvent(Remote remote, CallEvent event) {
    Log.i(TAG, "onEvent():");
    observer.onCallEvent(remote, event);
  }

  @CalledByNative
  private void onCallConcluded(Remote remote) {
    Log.i(TAG, "onCallConcluded():");
    observer.onCallConcluded(remote);
  }

  @CalledByNative
  private void onSendOffer(long callId, Remote remote, int remoteDevice, boolean broadcast, @Nullable byte[] opaque, @Nullable String sdp, CallMediaType callMediaType) {
    Log.i(TAG, "onSendOffer():");
    observer.onSendOffer(new CallId(callId), remote, Integer.valueOf(remoteDevice), Boolean.valueOf(broadcast), opaque, sdp, callMediaType);
  }

  @CalledByNative
  private void onSendAnswer(long callId, Remote remote, int remoteDevice, boolean broadcast, @Nullable byte[] opaque, @Nullable String sdp) {
    Log.i(TAG, "onSendAnswer():");
    observer.onSendAnswer(new CallId(callId), remote, Integer.valueOf(remoteDevice), Boolean.valueOf(broadcast), opaque, sdp);
  }

  @CalledByNative
  private void onSendIceCandidates(long callId, Remote remote, int remoteDevice, boolean broadcast, List<IceCandidate> iceCandidates) {
    Log.i(TAG, "onSendIceCandidates():");
    observer.onSendIceCandidates(new CallId(callId), remote, Integer.valueOf(remoteDevice), Boolean.valueOf(broadcast), iceCandidates);
  }

  @CalledByNative
  private void onSendHangup(long callId, Remote remote, int remoteDevice, boolean broadcast, HangupType hangupType, int deviceId, boolean useLegacyHangupMessage) {
    Log.i(TAG, "onSendHangup():");
    observer.onSendHangup(new CallId(callId), remote, Integer.valueOf(remoteDevice), Boolean.valueOf(broadcast), hangupType, Integer.valueOf(deviceId), Boolean.valueOf(useLegacyHangupMessage));
  }

  @CalledByNative
  private void onSendBusy(long callId, Remote remote, int remoteDevice, boolean broadcast) {
    Log.i(TAG, "onSendBusy():");
    observer.onSendBusy(new CallId(callId), remote, Integer.valueOf(remoteDevice), Boolean.valueOf(broadcast));
  }

  @CalledByNative
  private boolean compareRemotes(Remote remote1, Remote remote2) {
    Log.i(TAG, "compareRemotes():");
    if (remote1 != null) {
      return remote1.recipientEquals(remote2);
    }
    return false;
  }

  /**
   *
   * Contains parameters for creating Connection objects
   */
  static class CallContext {

    @NonNull  private final String TAG = CallManager.CallContext.class.getSimpleName();
    /** CallId */
    @NonNull  public final  CallId                         callId;
    /** Connection factory */
    @NonNull  public final  PeerConnectionFactory          peerConnectionFactory;
    /** Remote camera surface renderer */
    @NonNull  public final  VideoSink                      remoteSink;
    /** Camera controller */
    @NonNull  public final  CameraControl                  cameraControl;
    /** ICE server list */
    @NonNull  public final  List<PeerConnection.IceServer> iceServers;
    /** If true, use TURN servers */
              public final  boolean                        hideIp;
    @Nullable public final  VideoSource                    videoSource;
    @Nullable public final  VideoTrack                     videoTrack;
    @NonNull  public final  RtcCertificatePem              certificate;

    public CallContext(@NonNull CallId                         callId,
                       @NonNull Context                        context,
                       @NonNull EglBase                        eglBase,
                       @NonNull VideoSink                      localSink,
                       @NonNull VideoSink                      remoteSink,
                       @NonNull CameraControl                  camera,
                       @NonNull List<PeerConnection.IceServer> iceServers,
                                boolean                        hideIp,
                       @NonNull RtcCertificatePem              certificate) {

      Log.i(TAG, "ctor(): " + callId);

      this.callId         = callId;
      this.remoteSink     = remoteSink;
      this.cameraControl  = camera;
      this.iceServers     = iceServers;
      this.hideIp         = hideIp;
      this.certificate    = certificate;

      Set<String> HARDWARE_ENCODING_BLACKLIST = new HashSet<String>() {{
         // Samsung S6 with Exynos 7420 SoC
         add("SM-G920F");
         add("SM-G920FD");
         add("SM-G920FQ");
         add("SM-G920I");
         add("SM-G920A");
         add("SM-G920T");

         // Samsung S7 with Exynos 8890 SoC
         add("SM-G930F");
         add("SM-G930FD");
         add("SM-G930W8");
         add("SM-G930S");
         add("SM-G930K");
         add("SM-G930L");

         // Samsung S7 Edge with Exynos 8890 SoC
         add("SM-G935F");
         add("SM-G935FD");
         add("SM-G935W8");
         add("SM-G935S");
         add("SM-G935K");
         add("SM-G935L");
      }};

      VideoEncoderFactory encoderFactory;

      if (HARDWARE_ENCODING_BLACKLIST.contains(Build.MODEL)) {
        encoderFactory = new SoftwareVideoEncoderFactory();
      } else {
        encoderFactory = new DefaultVideoEncoderFactory(eglBase.getEglBaseContext(), true, true);
      }

      VideoDecoderFactory decoderFactory = new DefaultVideoDecoderFactory(eglBase.getEglBaseContext());

      this.peerConnectionFactory = PeerConnectionFactory.builder()
        .setOptions(new PeerConnectionFactoryOptions())
        .setVideoEncoderFactory(encoderFactory)
        .setVideoDecoderFactory(decoderFactory)
        .createPeerConnectionFactory();

      // Create a video track that will be shared across all
      // connection objects.  It must be disposed manually.
      if (cameraControl.hasCapturer()) {
        this.videoSource = peerConnectionFactory.createVideoSource(false);
        this.videoTrack  = peerConnectionFactory.createVideoTrack("ARDAMSv0", videoSource);
        videoTrack.setEnabled(false);

        // Connect camera as the local video source.
        cameraControl.initCapturer(videoSource.getCapturerObserver());
        videoTrack.addSink(localSink);
      } else {
        this.videoSource = null;
        this.videoTrack  = null;
      }

    }

    void setVideoEnabled(boolean enable) {
      Log.i(TAG, "setVideoEnabled(): " + callId);
      if (videoTrack != null) {
        videoTrack.setEnabled(enable);
        cameraControl.setEnabled(enable);
      }
    }

    void dispose() {
      Log.i(TAG, "dispose(): " + callId);

      if (cameraControl != null) {
        cameraControl.setEnabled(false);
      }

      if (videoSource != null) {
        videoSource.dispose();
      }

      if (videoTrack != null) {
        videoTrack.dispose();
      }

      peerConnectionFactory.dispose();
    }

    class PeerConnectionFactoryOptions extends PeerConnectionFactory.Options {
      public PeerConnectionFactoryOptions() {
        this.networkIgnoreMask = 1 << 4;
      }
    }

  }

  /**
   *
   * Enumeration of simple call status events
   *
   */
  public enum CallEvent {

    /** Inbound call only: The call signaling (ICE) is complete. */
    LOCAL_RINGING,

    /** Outbound call only: The call signaling (ICE) is complete. */
    REMOTE_RINGING,

    /** The local side has accepted and connected the call. */
    LOCAL_CONNECTED,

    /** The remote side has accepted and connected the call. */
    REMOTE_CONNECTED,

    /** The call ended because of a local hangup. */
    ENDED_LOCAL_HANGUP,

    /** The call ended because of a remote hangup. */
    ENDED_REMOTE_HANGUP,

    /** The call ended because the remote needs permission. */
    ENDED_REMOTE_HANGUP_NEED_PERMISSION,

    /** The call ended because the call was accepted by a different device. */
    ENDED_REMOTE_HANGUP_ACCEPTED,

    /** The call ended because the call was declined by a different device. */
    ENDED_REMOTE_HANGUP_DECLINED,

    /** The call ended because the call was declared busy by a different device. */
    ENDED_REMOTE_HANGUP_BUSY,

    /** The call ended because of a remote busy message. */
    ENDED_REMOTE_BUSY,

    /** The call ended because of glare (received offer from same remote). */
    ENDED_REMOTE_GLARE,

    /** The call ended because it timed out during setup. */
    ENDED_TIMEOUT,

    /** The call ended because of an internal error condition. */
    ENDED_INTERNAL_FAILURE,

    /** The call ended because a signaling message couldn't be sent. */
    ENDED_SIGNALING_FAILURE,

    /** The call ended because setting up the connection failed. */
    ENDED_CONNECTION_FAILURE,

    /** The call ended because the application wanted to drop the call. */
    ENDED_APP_DROPPED_CALL,

    /** The remote peer indicates its video stream is enabled. */
    REMOTE_VIDEO_ENABLE,

    /** The remote peer indicates its video stream is disabled. */
    REMOTE_VIDEO_DISABLE,

    /** The call dropped while connected and is now reconnecting. */
    RECONNECTING,

    /** The call dropped while connected and is now reconnected. */
    RECONNECTED,

    /** The received offer is expired. */
    ENDED_RECEIVED_OFFER_EXPIRED,

    /** Received an offer while already handling an active call. */
    ENDED_RECEIVED_OFFER_WHILE_ACTIVE,

    /** Received an offer on a linked device from one that doesn't support multi-ring. */
    ENDED_IGNORE_CALLS_FROM_NON_MULTIRING_CALLERS;

    @CalledByNative
    static CallEvent fromNativeIndex(int nativeIndex) {
      return values()[nativeIndex];
    }

  }

  /**
   *
   * Enumeration of the type of media for a call at time of origination
   *
   */
  public enum CallMediaType {

    /** Call should start as audio only. */
    AUDIO_CALL,

    /** Call should start as audio/video. */
    VIDEO_CALL;

    @CalledByNative
    static CallMediaType fromNativeIndex(int nativeIndex) {
      return values()[nativeIndex];
    }

  }

  /**
   *
   * Enumeration of the type of hangup messages
   *
   */
  public enum HangupType {

    /** Normal hangup, typically remote user initiated. */
    NORMAL,

    /** Call was accepted elsewhere by a different device. */
    ACCEPTED,

    /** Call was declined elsewhere by a different device. */
    DECLINED,

    /** Call was declared busy elsewhere by a different device. */
    BUSY,

    /** Call needed permission on a different device. */
    NEED_PERMISSION;

    @CalledByNative
    static HangupType fromNativeIndex(int nativeIndex) {
      return values()[nativeIndex];
    }

  }

  /**
   *
   * Interface for handling CallManager events and errors
   *
   */
  public interface Observer {

    /**
     *
     * Notification to start a call
     *
     * @param remote         remote peer of the call
     * @param callId         callId for the call
     * @param isOutgoing     true for an outgoing call, false for incoming
     * @param callMediaType  the origination type for the call, audio or video
     *
     */
    void onStartCall(Remote remote, CallId callId, Boolean isOutgoing, CallMediaType callMediaType);

    /**
     *
     * Notification of an event for the active call sent to the UI
     *
     * @param remote  remote peer of the call
     * @param event   event to be notified of
     *
     */
    void onCallEvent(Remote remote, CallEvent event);

    /**
     *
     * Notification of that the call is completely concluded
     *
     * @param remote  remote peer of the call
     *
     */
    void onCallConcluded(Remote remote);

    /**
     *
     * Notification that an SDP offer is ready to be sent
     *
     * @param callId         callId for the call
     * @param remote         remote peer of the outgoing call
     * @param remoteDevice   deviceId of remote peer
     * @param broadcast      if true, send broadcast message
     * @param opaque         the opaque offer
     * @param sdp            the SDP offer (deprecated/legacy)
     * @param callMediaType  the origination type for the call, audio or video
     *
     */
    void onSendOffer(CallId callId, Remote remote, Integer remoteDevice, Boolean broadcast, @Nullable byte[] opaque, @Nullable String sdp, CallMediaType callMediaType);

    /**
     *
     * Notification that an SDP answer is ready to be sent
     *
     * @param callId        callId for the call
     * @param remote        remote peer of the outgoing call
     * @param remoteDevice  deviceId of remote peer
     * @param broadcast     if true, send broadcast message
     * @param opaque        the opaque answer
     * @param sdp           the SDP answer (deprecated/legacy)
     *
     */
    void onSendAnswer(CallId callId, Remote remote, Integer remoteDevice, Boolean broadcast, @Nullable byte[] opaque, @Nullable String sdp);

    /**
     *
     * Notification that ICE candidates are ready to be sent
     *
     * @param callId         callId for the call
     * @param remote         remote peer of the outgoing call
     * @param remoteDevice   deviceId of remote peer
     * @param broadcast      if true, send broadcast message
     * @param iceCandidates  ICE candidates
     *
     */
    void onSendIceCandidates(CallId callId, Remote remote, Integer remoteDevice, Boolean broadcast, List<IceCandidate> iceCandidates);

    /**
     *
     * Notification that hangup message is ready to be sent
     *
     * @param callId                  callId for the call
     * @param remote                  remote peer of the call
     * @param remoteDevice            deviceId of remote peer
     * @param broadcast               if true, send broadcast message
     * @param hangupType              type of hangup, normal or handled elsewhere
     * @param deviceId                if not a normal hangup, the associated deviceId
     * @param useLegacyHangupMessage  if true, use legacyHangup as opposed to hangup in protocol
     *
     */
    void onSendHangup(CallId callId, Remote remote, Integer remoteDevice, Boolean broadcast, HangupType hangupType, Integer deviceId, Boolean useLegacyHangupMessage);

    /**
     *
     * Notification that busy message is ready to be sent
     *
     * @param callId        callId for the call
     * @param remote        remote peer of the incoming busy call
     * @param remoteDevice  deviceId of remote peer
     * @param broadcast     if true, send broadcast message
     *
     */
    void onSendBusy(CallId callId, Remote remote, Integer remoteDevice, Boolean broadcast);

  }

  /**
   * A custom, NO-OP library loader for jingle_peerconnection_so, as
   * our rust shared library already loads it.
   */
  static class NoOpLoader implements NativeLibraryLoader {
    public NoOpLoader() {
    }

    @Override public boolean load(String name) {
      return true;
    }
  }

  /* Native methods below here */

  private static native
    BuildInfo ringrtcGetBuildInfo()
    throws CallException;

  private static native
    void ringrtcInitialize()
    throws CallException;

  private static native
    long ringrtcCreateCallManager(CallManager callManager)
    throws CallException;

  private native
    long ringrtcCreatePeerConnection(long                            nativePeerConnectionFactory,
                                     long                            nativeConnection,
                                     PeerConnection.RTCConfiguration rtcConfig,
                                     MediaConstraints                constraints)
    throws CallException;

  private native
    void ringrtcCall(long nativeCallManager, Remote remote, int callMediaType, int localDevice)
    throws CallException;

  private native
    void ringrtcProceed(long        nativeCallManager,
                        long        callId,
                        CallContext callContext)
    throws CallException;

  private native
    void ringrtcMessageSent(long nativeCallManager, long callId)
    throws CallException;

  private native
    void ringrtcMessageSendFailure(long nativeCallManager, long callId)
    throws CallException;

  private native
    void ringrtcHangup(long nativeCallManager)
    throws CallException;

  private native
    void ringrtcReceivedAnswer(long             nativeCallManager,
                               long             callId,
                               int              remoteDevice,
                               @Nullable byte[] opaque,
                               @Nullable String sdp,
                               boolean          remoteSupportsMultiRing)
    throws CallException;

  private native
    void ringrtcReceivedOffer(long             nativeCallManager,
                              long             callId,
                              Remote           remote,
                              int              remoteDevice,
                              @Nullable byte[] opaque,
                              @Nullable String sdp,
                              long             messageAgeSec,
                              int              callMediaType,
                              int              localDevice,
                              boolean          remoteSupportsMultiRing,
                              boolean          isLocalDevicePrimary)
    throws CallException;

  private native
    void ringrtcReceivedIceCandidates(long               nativeCallManager,
                                      long               callId,
                                      int                remoteDevice,
                                      List<IceCandidate> iceCandidates)
    throws CallException;

  private native
    void ringrtcReceivedHangup(long nativeCallManager,
                               long callId,
                               int  remoteDevice,
                               int  hangupType,
                               int  deviceId)
    throws CallException;

  private native
    void ringrtcReceivedBusy(long nativeCallManager,
                             long callId,
                             int  remoteDevice)
    throws CallException;

  private native
    void ringrtcAcceptCall(long nativeCallManager, long callId)
    throws CallException;

  private native
    Connection ringrtcGetActiveConnection(long nativeCallManager)
    throws CallException;

  private native
    CallContext ringrtcGetActiveCallContext(long nativeCallManager)
    throws CallException;

  private native
    void ringrtcSetVideoEnable(long nativeCallManager, boolean enable)
    throws CallException;

  private native
    void ringrtcSetLowBandwidthMode(long nativeCallManager, boolean enabled)
    throws CallException;

  private native
    void ringrtcDrop(long nativeCallManager, long callId)
    throws CallException;

  private native
    void ringrtcReset(long nativeCallManager)
    throws CallException;

  private native
    void ringrtcClose(long nativeCallManager)
    throws CallException;
}
