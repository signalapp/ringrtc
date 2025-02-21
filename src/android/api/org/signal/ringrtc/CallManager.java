/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import android.content.Context;
import android.util.LongSparseArray;
import androidx.annotation.NonNull;
import androidx.annotation.Nullable;

import android.os.Build;

import android.media.AudioManager;

import org.webrtc.AudioSource;
import org.webrtc.AudioTrack;
import org.webrtc.ContextUtils;
import org.webrtc.DefaultVideoDecoderFactory;
import org.webrtc.DefaultVideoEncoderFactory;
import org.webrtc.SoftwareVideoDecoderFactory;
import org.webrtc.SoftwareVideoEncoderFactory;
import org.webrtc.EglBase;
import org.webrtc.Logging.Severity;
import org.webrtc.MediaConstraints;
import org.webrtc.MediaStream;
import org.webrtc.NativeLibraryLoader;
import org.webrtc.PeerConnection;
import org.webrtc.PeerConnectionFactory;
import org.webrtc.VideoDecoderFactory;
import org.webrtc.VideoEncoderFactory;
import org.webrtc.VideoSource;
import org.webrtc.VideoTrack;
import org.webrtc.VideoSink;
import org.webrtc.audio.AudioDeviceModule;
import org.webrtc.audio.JavaAudioDeviceModule;
import org.webrtc.audio.OboeAudioDeviceModule;

import java.util.ArrayList;
import java.util.Collection;
import java.util.Collections;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Map;
import java.util.Set;
import java.util.List;
import java.util.UUID;

/**
 *
 * Provides an interface to the RingRTC Call Manager.
 *
 */
public class CallManager {
  public static final  int     INVALID_AUDIO_SESSION_ID = -1;

  @NonNull
  private static final String  TAG = CallManager.class.getSimpleName();

  private static       boolean isInitialized;


  private long                                nativeCallManager;

  @NonNull
  private Observer                            observer;

  // Keep a hash/mapping of a callId to a GroupCall object. CallId is a u32
  // and will fit in to the long type.
  @NonNull
  private LongSparseArray<GroupCall>          groupCallByClientId;

  @NonNull
  private Requests<HttpResult<PeekInfo>>      peekRequests;

  @NonNull
  private Requests<HttpResult<CallLinkState>> callLinkRequests;

  @NonNull
  private Requests<HttpResult<Boolean>>       emptyRequests;

  @Nullable
  private PeerConnectionFactory               groupFactory;

  static {
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
   * @param fieldTrials         Configuration to alter WebRTC's default behavior
   */
  public static void initialize(Context applicationContext, Log.Logger logger, Map<String, String> fieldTrials) {

    try {
      Log.initialize(logger);

      PeerConnectionFactory.InitializationOptions.Builder builder = PeerConnectionFactory.InitializationOptions.builder(applicationContext)
        .setNativeLibraryLoader(new NoOpLoader());

      BuildInfo buildInfo = ringrtcGetBuildInfo();

      Map<String, String> fieldTrialsWithDefaults = new HashMap<>();
      fieldTrialsWithDefaults.put("RingRTC-PruneTurnPorts", "Enabled");
      fieldTrialsWithDefaults.put("WebRTC-Bwe-ProbingConfiguration", "skip_if_est_larger_than_fraction_of_max:0.99");
      fieldTrialsWithDefaults.put("WebRTC-IncreaseIceCandidatePriorityHostSrflx", "Enabled");
      fieldTrialsWithDefaults.putAll(fieldTrials);

      String fieldTrialsString = buildFieldTrialsString(fieldTrialsWithDefaults);

      Log.i(TAG, "CallManager.initialize(): (" + (buildInfo.debug ? "debug" : "release") + " build, field trials = " + fieldTrialsString + ")");

      if (buildInfo.debug) {
        // Show all WebRTC logs via application Logger while debugging.
        builder.setInjectableLogger(new WebRtcLogger(), Severity.LS_INFO);
      } else {
        // Show WebRTC error and warning logs via application Logger for release builds.
        builder.setInjectableLogger(new WebRtcLogger(), Severity.LS_WARNING);
      }

      builder.setFieldTrials(fieldTrialsString);

      PeerConnectionFactory.initialize(builder.createInitializationOptions());
      ringrtcInitialize();
      CallManager.isInitialized = true;
      Log.i(TAG, "CallManager.initialize() returned");
    } catch (UnsatisfiedLinkError e) {
      Log.w(TAG, "Unable to load ringrtc library", e);
      throw new AssertionError("Unable to load ringrtc library", e);
    } catch  (CallException e) {
      Log.w(TAG, "Unable to initialize ringrtc library", e);
      throw new AssertionError("Unable to initialize ringrtc library", e);
    }

  }

  private static void checkInitializeHasBeenCalled() {
    if (!CallManager.isInitialized) {
      throw new IllegalStateException("CallManager.initialize has not been called");
    }
  }

  private static String buildFieldTrialsString(Map<String, String> fieldTrials) {
    StringBuilder builder = new StringBuilder();

    for (Map.Entry<String, String> entry : fieldTrials.entrySet()) {
      builder.append(entry.getKey());
      builder.append('/');
      builder.append(entry.getValue());
      builder.append('/');
    }

    return builder.toString();
  }

  class PeerConnectionFactoryOptions extends PeerConnectionFactory.Options {
    public PeerConnectionFactoryOptions() {
      // Give the (native default) behavior of filtering out loopback addresses.
      this.networkIgnoreMask = PeerConnectionFactory.Options.ADAPTER_TYPE_LOOPBACK;
    }
  }

  /// Defines the method to use for audio processing of AEC and NS.
  public enum AudioProcessingMethod {
    Default,
    ForceHardware,
    ForceSoftwareAec3
  }

  /// Creates a PeerConnectionFactory appropriate for our use of WebRTC.
  ///
  /// If `eglBase` is present, hardware codecs will be used unless they are known to be broken
  /// in some way. Otherwise, we'll fall back to software codecs.
  private PeerConnectionFactory createPeerConnectionFactory(@Nullable EglBase               eglBase,
                                                                      AudioProcessingMethod audioProcessingMethod,
                                                                      boolean               useOboe) {
    Set<String> HARDWARE_ENCODING_BLOCKLIST = new HashSet<String>() {{
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

      // Samsung A3 with Exynos 7870 SoC
      add("SM-A320F");
      add("SM-A320FL");
      add("SM-A320F/DS");
      add("SM-A320Y/DS");
      add("SM-A320Y");

      // Samsung S22 5G with Exynos 2200 SoC
      add("SM-S901B");
    }};

    VideoEncoderFactory encoderFactory;
    if (eglBase == null || HARDWARE_ENCODING_BLOCKLIST.contains(Build.MODEL)) {
      encoderFactory = new SoftwareVideoEncoderFactory();
    } else {
      encoderFactory = new DefaultVideoEncoderFactory(eglBase.getEglBaseContext(), true, true);
    }

    VideoDecoderFactory decoderFactory;
    if (eglBase == null) {
      decoderFactory = new SoftwareVideoDecoderFactory();
    } else {
      decoderFactory = new DefaultVideoDecoderFactory(eglBase.getEglBaseContext());
    }

    // We'll set both AEC and NS equally to be either both hardware or
    // both software, assuming that they are co-tuned.
    boolean useHardware = audioProcessingMethod != AudioProcessingMethod.ForceSoftwareAec3;

    Log.i(TAG, "createPeerConnectionFactory(): useHardware: " + useHardware + " useOboe: " + useOboe);

    // ContextUtils.getApplicationContext() is deprecated;
    // we're supposed to have a Context on hand instead.
    @SuppressWarnings("deprecation")
    Context context = ContextUtils.getApplicationContext();

    if (useOboe) {
      // Use the Oboe Audio Device Module.
      OboeAudioDeviceModule adm = OboeAudioDeviceModule.builder()
        .setUseSoftwareAcousticEchoCanceler(!useHardware)
        .setUseSoftwareNoiseSuppressor(!useHardware)
        .setExclusiveSharingMode(true)
        .setAudioSessionId(INVALID_AUDIO_SESSION_ID)
        .createAudioDeviceModule();

      PeerConnectionFactory factory = PeerConnectionFactory.builder()
              .setOptions(new PeerConnectionFactoryOptions())
              .setAudioDeviceModule(adm)
              .setVideoEncoderFactory(encoderFactory)
              .setVideoDecoderFactory(decoderFactory)
              .createPeerConnectionFactory();
      adm.release();
      return factory;
    } else {
      // The legacy Java Audio Device Module is deprecated.
      JavaAudioDeviceModule adm = JavaAudioDeviceModule.builder(context)
        .setUseHardwareAcousticEchoCanceler(useHardware)
        .setUseHardwareNoiseSuppressor(useHardware)
        .createAudioDeviceModule();

      PeerConnectionFactory factory = PeerConnectionFactory.builder()
              .setOptions(new PeerConnectionFactoryOptions())
              .setAudioDeviceModule(adm)
              .setVideoEncoderFactory(encoderFactory)
              .setVideoDecoderFactory(decoderFactory)
              .createPeerConnectionFactory();
      adm.release();
      return factory;
    }
  }

  private void checkCallManagerExists() {
    if (nativeCallManager == 0) {
      throw new IllegalStateException("CallManager has been disposed");
    }
  }

  CallManager(@NonNull Observer observer) {
    Log.i(TAG, "CallManager():");

    this.observer            = observer;
    this.nativeCallManager   = 0;
    this.groupCallByClientId = new LongSparseArray<>();
    this.peekRequests        = new Requests<>();
    this.callLinkRequests    = new Requests<>();
    this.emptyRequests       = new Requests<>();
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

    if (this.groupCallByClientId != null &&
        this.groupCallByClientId.size() > 0) {
      Log.w(TAG, "Closing CallManager but groupCallByClientId still has objects");
    }

    if (this.groupFactory != null) {
      this.groupFactory.dispose();
    }

    ringrtcClose(nativeCallManager);
    nativeCallManager = 0;
  }

  /**
   * 
   * Updates the UUID used for the current user.
   * 
   * @param uuid  The new UUID to use
   *
   * @throws CallException for native code failures
   * 
   */
  public void setSelfUuid(@NonNull UUID uuid)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "setSelfUuid():");

    ringrtcSetSelfUuid(nativeCallManager, Util.getBytesFromUuid(uuid));
  }

  /**
   *
   * Indication from application to start a new outgoing call
   *
   * @param remote         remote side fo the call
   * @param callMediaType  used to specify origination as an audio or video call
   * @param localDeviceId  the local deviceId of the client
   *
   * @throws CallException for native code failures
   *
   */
  public void call(         Remote        remote,
                   @NonNull CallMediaType callMediaType,
                   @NonNull Integer       localDeviceId)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "call(): creating new call:");

    ringrtcCall(nativeCallManager, remote, callMediaType.ordinal(), localDeviceId);
  }

  /**
   *
   * Indication from application to proceed with call
   *
   * @param callId                 callId for the call
   * @param context                Call service context
   * @param eglBase                eglBase to use for this Call
   * @param audioProcessingMethod  the method to use for audio processing
   * @param useOboe                whether to use the oboe-based audio device module, otherwise use java
   * @param localSink              local video sink to use for this Call
   * @param remoteSink             remote video sink to use for this Call
   * @param camera                 camera control to use for this Call
   * @param iceServers             list of ICE servers to use for this Call
   * @param hideIp                 if true hide caller's IP by using a TURN server
   * @param dataMode               desired data mode to start the session with
   * @param audioLevelsIntervalMs  if greater than 0, enable audio levels with this interval (in milliseconds)
   * @param enableCamera           if true, enable the local camera video track when created
   *
   * @throws CallException for native code failures
   *
   */
  public void proceed(@NonNull  CallId                         callId,
                      @NonNull  Context                        context,
                      @NonNull  EglBase                        eglBase,
                                AudioProcessingMethod          audioProcessingMethod,
                                boolean                        useOboe,
                      @NonNull  VideoSink                      localSink,
                      @NonNull  VideoSink                      remoteSink,
                      @NonNull  CameraControl                  camera,
                      @NonNull  List<PeerConnection.IceServer> iceServers,
                                boolean                        hideIp,
                                DataMode                       dataMode,
                      @Nullable Integer                        audioLevelsIntervalMs,
                                boolean                        enableCamera)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "proceed(): callId: " + callId + ", hideIp: " + hideIp);
    for (PeerConnection.IceServer iceServer : iceServers) {
      for (String url : iceServer.urls) {
        Log.i(TAG, "  server: " + url);
      }
    }

    PeerConnectionFactory factory = this.createPeerConnectionFactory(eglBase, audioProcessingMethod, useOboe);

    CallContext callContext = new CallContext(callId,
                                              context,
                                              factory,
                                              localSink,
                                              remoteSink,
                                              camera,
                                              iceServers,
                                              hideIp);

    callContext.setVideoEnabled(enableCamera);

    int audioLevelsIntervalMillis = audioLevelsIntervalMs == null ? 0 : audioLevelsIntervalMs.intValue();
    ringrtcProceed(nativeCallManager,
                   callId.longValue(),
                   callContext,
                   dataMode.ordinal(),
                   audioLevelsIntervalMillis);
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
   * Notification from application of a received Offer
   *
   * This is the beginning of an incoming call.
   *
   * @param callId                   callId for the call
   * @param remote                   remote side fo the call
   * @param remoteDeviceId           deviceId of remote peer
   * @param opaque                   the opaque offer
   * @param messageAgeSec            approximate age of the offer message, in seconds
   * @param callMediaType            the origination type for the call, audio or video
   * @param localDeviceId            the local deviceId of the client
   * @param senderIdentityKey        the identity key of the remote client
   * @param receiverIdentityKey      the identity key of the local client
   *
   * @throws CallException for native code failures
   *
   */
  public void receivedOffer(         CallId        callId,
                                     Remote        remote,
                                     Integer       remoteDeviceId,
                            @NonNull byte[]        opaque,
                                     Long          messageAgeSec,
                                     CallMediaType callMediaType,
                                     Integer       localDeviceId,
                            @NonNull byte[]        senderIdentityKey,
                            @NonNull byte[]        receiverIdentityKey)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "receivedOffer(): id: " + callId.format(remoteDeviceId));

    ringrtcReceivedOffer(nativeCallManager,
                         callId.longValue(),
                         remote,
                         remoteDeviceId,
                         opaque,
                         messageAgeSec,
                         callMediaType.ordinal(),
                         localDeviceId,
                         senderIdentityKey,
                         receiverIdentityKey);
  }

  /**
   *
   * Notification from application of a received Answer
   *
   * @param callId                   callId for the call
   * @param remoteDeviceId           deviceId of remote peer
   * @param opaque                   the opaque answer
   * @param senderIdentityKey        the identity key of the remote client
   * @param receiverIdentityKey      the identity key of the local client
   *
   * @throws CallException for native code failures
   *
   */
  public void receivedAnswer(         CallId  callId,
                                      Integer remoteDeviceId,
                             @NonNull byte[]  opaque,
                             @NonNull byte[]  senderIdentityKey,
                             @NonNull byte[]  receiverIdentityKey)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "receivedAnswer(): id: " + callId.format(remoteDeviceId));

    ringrtcReceivedAnswer(nativeCallManager,
                          callId.longValue(),
                          remoteDeviceId,
                          opaque,
                          senderIdentityKey,
                          receiverIdentityKey);
  }

  /**
   *
   * Notification from application of received ICE candidates
   *
   * @param callId          callId for the call
   * @param remoteDeviceId  deviceId of remote peer
   * @param iceCandidates   list of Ice Candidates
   *
   * @throws CallException for native code failures
   *
   */
  public void receivedIceCandidates(         CallId       callId,
                                             Integer      remoteDeviceId,
                                    @NonNull List<byte[]> iceCandidates)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "receivedIceCandidates(): id: " + callId.format(remoteDeviceId) + ", count: " + iceCandidates.size());

    ringrtcReceivedIceCandidates(nativeCallManager,
                                 callId.longValue(),
                                 remoteDeviceId,
                                 iceCandidates);
  }

  /**
   *
   * Notification from application of received Hangup message
   *
   * @param callId          callId for the call
   * @param remoteDeviceId  deviceId of remote peer
   * @param hangupType      type of hangup, normal or handled elsewhere
   * @param deviceId        if not a normal hangup, the associated deviceId
   *
   * @throws CallException for native code failures
   *
   */
  public void receivedHangup(CallId     callId,
                             Integer    remoteDeviceId,
                             HangupType hangupType,
                             Integer    deviceId)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "receivedHangup(): id: " + callId.format(remoteDeviceId));

    ringrtcReceivedHangup(nativeCallManager,
                          callId.longValue(),
                          remoteDeviceId,
                          hangupType.ordinal(),
                          deviceId);
  }

  /**
   *
   * Notification from application of received Busy message
   *
   * @param callId          callId for the call
   * @param remoteDeviceId  deviceId of remote peer
   *
   * @throws CallException for native code failures
   *
   */
  public void receivedBusy(CallId callId, Integer remoteDeviceId)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "receivedBusy(): id: " + callId.format(remoteDeviceId));

    ringrtcReceivedBusy(nativeCallManager,
                        callId.longValue(),
                        remoteDeviceId);
  }

  /**
   *
   * Provides a generic call message that has been received to the
   * RingRTC Call Manager for handling.
   *
   * @param senderUuid      the UUID of the sending user
   * @param senderDeviceId  the deviceId of the sending device
   * @param localDeviceId   the local deviceId
   * @param message         the byte array of the actual message
   * @param messageAgeSec   the age of the message, in seconds
   *
   * @throws CallException for native code failures
   *
   */
  public void receivedCallMessage(@NonNull UUID    senderUuid,
                                  @NonNull Integer senderDeviceId,
                                  @NonNull Integer localDeviceId,
                                  @NonNull byte[]  message,
                                  @NonNull Long    messageAgeSec)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "receivedCallMessage():");

    ringrtcReceivedCallMessage(nativeCallManager,
                               Util.getBytesFromUuid(senderUuid),
                               senderDeviceId,
                               localDeviceId,
                               message,
                               messageAgeSec);
  }

  /**
   *
   * Provides a HTTP response that has been received for a prior request
   * to the RingRTC Call Manager for handling.
   *
   * @param requestId       the Id of the request that the response belongs to
   * @param status          the standard HTTP status value of the response
   * @param body            the body of the response
   *
   * @throws CallException for native code failures
   *
   */
  public void receivedHttpResponse(         long   requestId,
                                            int    status,
                                   @NonNull byte[] body)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "receivedHttpResponse(): requestId: " + requestId);
    ringrtcReceivedHttpResponse(nativeCallManager,
                                requestId,
                                status,
                                body);
  }

  /**
   *
   * Indicates a failure that has been detected for a HTTP request.
   *
   * @param requestId       the Id of the request that the response belongs to
   *
   * @throws CallException for native code failures
   *
   */
  public void httpRequestFailed(long requestId)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "httpRequestFailed(): requestId: " + requestId);
    ringrtcHttpRequestFailed(nativeCallManager, requestId);
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

    ringrtcSetAudioEnable(nativeCallManager, enable);
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
   * Sets a data mode, allowing the client to limit the media bandwidth used.
   *
   * @param dataMode  one of the DataMode enumerated values
   *
   * @throws CallException for native code failures
   *
   */
  public void updateDataMode(DataMode dataMode)
    throws CallException
  {
    checkCallManagerExists();

    ringrtcUpdateDataMode(nativeCallManager, dataMode.ordinal());
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

  /** Describes why a ring was cancelled. */
  public enum RingCancelReason {
    /** The user explicitly clicked "Decline". */
    DeclinedByUser,
    /** The device is busy with another call. */
    Busy
  }

  /**
   *
   * Notification from application that a group ring is being cancelled.
   * 
   * @param groupId the unique identifier for the group
   * @param ringId  identifies the ring being declined
   * @param reason  if non-null, a reason for the cancellation that should be communicated to the
   *                user's other devices
   *
   * @throws CallException for native code failures
   *
   */
  public void cancelGroupRing(@NonNull byte[] groupId, long ringId, @Nullable RingCancelReason reason)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "cancelGroupRing():");
    int rawReason = reason != null ? reason.ordinal() : -1;
    ringrtcCancelGroupRing(nativeCallManager, groupId, ringId, rawReason);
  }

  // Group Calls and Call Links

  public interface ResponseHandler<T> {
    void handleResponse(T response);
  }

  public static class HttpResult<T> {
    @Nullable
    private final T value;
    private final short status;
  
    @CalledByNative
    HttpResult(@NonNull T value) {
      this.value = value;
      this.status = 200;
    }

    @CalledByNative
    HttpResult(short status) {
      this.value = null;
      this.status = status;
    }

    @Nullable
    public T getValue() {
      return value;
    }

    /** Note that this includes "artificial" error codes in the 6xx and 7xx range used by RingRTC. */
    public short getStatus() {
      return status;
    }

    public boolean isSuccess() {
      return value != null;
    }
  }

  static class Requests<T> {
    private long nextId = 1;
    @NonNull private LongSparseArray<ResponseHandler<T>> handlerById = new LongSparseArray<>();
  
    long add(ResponseHandler<T> handler) {
      long id = this.nextId++;
      this.handlerById.put(id, handler);
      return id;
    }

    boolean resolve(long id, T response) {
      ResponseHandler<T> handler = this.handlerById.get(id);
      if (handler == null) {
        return false;
      }
      handler.handleResponse(response);
      this.handlerById.delete(id);
      return true;
    }
  }

  /**
   *
   * Asynchronous request to get information about a call link.
   *
   * @param sfuUrl                     the URL to use when accessing the SFU
   * @param authCredentialPresentation a serialized CallLinkAuthCredentialPresentation
   * @param linkRootKey                the root key for the call link
   * @param handler                    a handler function which is invoked with the room's current state, or an error status code
   *
   * Expected failure codes include:
   * <ul>
   *   <li>404: the room does not exist (or expired so long ago that it has been removed from the server)
   * </ul>
   *
   * @throws CallException for native code failures
   *
   */
  public void readCallLink(
    @NonNull String                                     sfuUrl,
    @NonNull byte[]                                     authCredentialPresentation,
    @NonNull CallLinkRootKey                            linkRootKey,
    @NonNull ResponseHandler<HttpResult<CallLinkState>> handler)
    throws CallException
  {
    checkCallManagerExists();
    Log.i(TAG, "readCallLink():");

    long requestId = this.callLinkRequests.add(handler);
    ringrtcReadCallLink(nativeCallManager, sfuUrl, authCredentialPresentation, linkRootKey.getKeyBytes(), requestId);
  }

  /**
   *
   * Asynchronous request to create a new call link.
   *
   * This request is idempotent; if it fails due to a network issue, it is safe to retry.
   *
   * <pre>
   * CallLinkRootKey linkKey = CallLinkRootKey.generate();
   * byte[] adminPasskey = CallLinkRootKey.generateAdminPasskey();
   * byte[] roomId = linkKey.deriveRoomId();
   * CreateCallLinkCredential credential = requestCreateCredentialFromChatServer(roomId); // using libsignal
   * CallLinkSecretParams secretParams = CallLinkSecretParams.deriveFromRootKey(linkKey.getKeyBytes());
   * byte[] credentialPresentation = credential.present(roomId, secretParams).serialize();
   * byte[] serializedPublicParams = secretParams.getPublicParams().serialize();
   * CallLinkState.Restrictions restrictions = CallLinkState.Restrictions.NONE;
   * callManager.createCallLink(sfuUrl, credentialPresentation, linkKey, adminPasskey, serializedPublicParams, restrictions, result -> {
   *   if (result.isSuccess()) {
   *     CallLinkState state = result.getValue();
   *     // In actuality you may not want to do this until the user clicks Done.
   *     saveToDatabase(linkKey.getKeyBytes(), adminPasskey, state);
   *     syncToOtherDevices(linkKey.getKeyBytes(), adminPasskey);
   *   } else {
   *     switch (result.getStatus()) {
   *     case 409:
   *       // The room already exists (and isn't yours), i.e. you've hit a 1-in-a-billion conflict.
   *       // Fall through to kicking the user out to try again later.
   *     default:
   *       // Unexpected error, kick the user out for now.
   *     }
   *   }
   * });
   * </pre>
   *
   * @param sfuUrl                       the URL to use when accessing the SFU
   * @param createCredentialPresentation a serialized CreateCallLinkCredentialPresentation
   * @param linkRootKey                  the root key for the call link
   * @param adminPasskey                 the arbitrary passkey to use for the new room
   * @param callLinkPublicParams         the serialized CallLinkPublicParams for the new room
   * @param handler                      a handler function which is invoked with the newly-created room's initial state, or an error status code
   *
   * @throws CallException for native code failures
   *
   */
  public void createCallLink(
    @NonNull String                                     sfuUrl,
    @NonNull byte[]                                     createCredentialPresentation,
    @NonNull CallLinkRootKey                            linkRootKey,
    @NonNull byte[]                                     adminPasskey,
    @NonNull byte[]                                     callLinkPublicParams,
    @NonNull CallLinkState.Restrictions                 restrictions,
    @NonNull ResponseHandler<HttpResult<CallLinkState>> handler)
    throws CallException
  {
    checkCallManagerExists();
    Log.i(TAG, "createCallLink():");

    long requestId = this.callLinkRequests.add(handler);
    ringrtcCreateCallLink(nativeCallManager, sfuUrl, createCredentialPresentation, linkRootKey.getKeyBytes(), adminPasskey, callLinkPublicParams, restrictions.ordinal(), requestId);
  }

  /**
   *
   * Asynchronous request to update a call link's name.
   *
   * Possible failure codes include:
   * <ul>
   *   <li>401: the room does not exist (and this is the wrong API to create a new room)
   *   <li>403: the admin passkey is incorrect
   * </ul>
   *
   * This request is idempotent; if it fails due to a network issue, it is safe to retry.
   *
   * @param sfuUrl                     the URL to use when accessing the SFU
   * @param authCredentialPresentation a serialized CallLinkAuthCredentialPresentation
   * @param linkRootKey                the root key for the call link
   * @param adminPasskey               the passkey specified when the link was created
   * @param newName                    the new name to use
   * @param handler                    a handler function which is invoked with the room's updated state, or an error status code
   *
   * @throws CallException for native code failures
   *
   */
  public void updateCallLinkName(
    @NonNull String                                     sfuUrl,
    @NonNull byte[]                                     authCredentialPresentation,
    @NonNull CallLinkRootKey                            linkRootKey,
    @NonNull byte[]                                     adminPasskey,
    @NonNull String                                     newName,
    @NonNull ResponseHandler<HttpResult<CallLinkState>> handler)
    throws CallException
  {
    checkCallManagerExists();
    Log.i(TAG, "updateCallLinkName():");

    long requestId = this.callLinkRequests.add(handler);
    ringrtcUpdateCallLink(nativeCallManager, sfuUrl, authCredentialPresentation, linkRootKey.getKeyBytes(), adminPasskey, newName, -1, -1, requestId);
  }

  /**
   *
   * Asynchronous request to update a call link's restrictions.
   *
   * Possible failure codes include:
   * <ul>
   *   <li>401: the room does not exist (and this is the wrong API to create a new room)
   *   <li>403: the admin passkey is incorrect
   *   <li>409: the room is currently in use, so restrictions cannot be changed at the moment
   * </ul>
   *
   * This request is idempotent; if it fails due to a network issue, it is safe to retry.
   *
   * @param sfuUrl                     the URL to use when accessing the SFU
   * @param authCredentialPresentation a serialized CallLinkAuthCredentialPresentation
   * @param linkRootKey                the root key for the call link
   * @param adminPasskey               the passkey specified when the link was created
   * @param restrictions               the new restrictions to use
   * @param handler                    a handler function which is invoked with the room's updated state, or an error status code
   *
   * @throws CallException for native code failures
   *
   */
  public void updateCallLinkRestrictions(
    @NonNull String                                     sfuUrl,
    @NonNull byte[]                                     authCredentialPresentation,
    @NonNull CallLinkRootKey                            linkRootKey,
    @NonNull byte[]                                     adminPasskey,
    @NonNull CallLinkState.Restrictions                 restrictions,
    @NonNull ResponseHandler<HttpResult<CallLinkState>> handler)
    throws CallException
  {
    checkCallManagerExists();
    Log.i(TAG, "updateCallLinkRestrictions():");
    if (restrictions == CallLinkState.Restrictions.UNKNOWN) {
      throw new IllegalArgumentException("cannot set a call link's restrictions to UNKNOWN");
    }

    long requestId = this.callLinkRequests.add(handler);
    ringrtcUpdateCallLink(nativeCallManager, sfuUrl, authCredentialPresentation, linkRootKey.getKeyBytes(), adminPasskey, null, restrictions.ordinal(), -1, requestId);
  }

  /**
   *
   * Asynchronous request to delete a call link.
   *
   * Possible failure codes include:
   * <ul>
   *   <li>403: the admin passkey is incorrect
   *   <li>409: there is an ongoing call using the call link
   * </ul>
   *
   * This request is idempotent; if it fails due to a network issue, it is safe to retry.
   *
   * @param sfuUrl                     the URL to use when accessing the SFU
   * @param authCredentialPresentation a serialized CallLinkAuthCredentialPresentation
   * @param linkRootKey                the root key for the call link
   * @param adminPasskey               the passkey specified when the link was created
   * @param handler                    a handler function which is invoked with a trash boolean, or an error status code
   *
   * @throws CallException for native code failures
   *
   */
  public void deleteCallLink(
    @NonNull String                                     sfuUrl,
    @NonNull byte[]                                     authCredentialPresentation,
    @NonNull CallLinkRootKey                            linkRootKey,
    @NonNull byte[]                                     adminPasskey,
    @NonNull ResponseHandler<HttpResult<Boolean>>       handler)
    throws CallException
  {
    checkCallManagerExists();
    Log.i(TAG, "deleteCallLink():");

    long requestId = this.emptyRequests.add(handler);
    ringrtcDeleteCallLink(nativeCallManager, sfuUrl, authCredentialPresentation, linkRootKey.getKeyBytes(), adminPasskey, requestId);
  }

  /**
   *
   * Asynchronous request for the group call state from the SFU for a particular
   * group. Does not require a group call object.
   *
   * @param sfuUrl           the URL to use when accessing the SFU
   * @param membershipProof  byte array containing the proof for accessing a specific group call
   * @param groupMembers     a GroupMemberInfo object for each member in a group
   * @param handler          a handler function which is invoked once the data is available
   *
   * @throws CallException for native code failures
   *
   */
  public void peekGroupCall(@NonNull String                                sfuUrl,
                            @NonNull byte[]                                membershipProof,
                            @NonNull Collection<GroupCall.GroupMemberInfo> groupMembers,
                            @NonNull ResponseHandler<PeekInfo>             handler)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "peekGroupCall():");

    long requestId = this.peekRequests.add(result -> {
      if (result.isSuccess()) {
        handler.handleResponse(result.getValue());
      } else {
        handler.handleResponse(new PeekInfo(Collections.emptyList(), null, null, null, 0, 0, Collections.emptyList(), null));
      }
    });
    ringrtcPeekGroupCall(nativeCallManager, requestId, sfuUrl, membershipProof, Util.serializeFromGroupMemberInfo(groupMembers));
  }

  /**
   *
   * Asynchronous request for the active call state from the SFU for a particular
   * call link. Does not require a group call object.
   *
   * Possible (synthetic) failure codes include:
   * <ul>
   *   <li>{@link PeekInfo#EXPIRED_CALL_LINK_STATUS}: the call link has expired or been revoked
   *   <li>{@link PeekInfo#INVALID_CALL_LINK_STATUS}: the call link is invalid; it may have expired a long time ago
   * </ul>
   *
   * Will produce an "empty" {@link PeekInfo} if the link is valid but no call is active.
   *
   * @param sfuUrl                     the URL to use when accessing the SFU
   * @param authCredentialPresentation a serialized CallLinkAuthCredentialPresentation
   * @param linkRootKey                the root key for the call link
   * @param handler                    a handler function which is invoked once the data is available
   *
   * @throws CallException for native code failures
   *
   */
  public void peekCallLinkCall(
    @NonNull String                                sfuUrl,
    @NonNull byte[]                                authCredentialPresentation,
    @NonNull CallLinkRootKey                       linkRootKey,
    @NonNull ResponseHandler<HttpResult<PeekInfo>> handler)
    throws CallException
  {
    checkCallManagerExists();

    Log.i(TAG, "peekCallLink():");

    long requestId = this.peekRequests.add(handler);
    ringrtcPeekCallLinkCall(nativeCallManager, requestId, sfuUrl, authCredentialPresentation, linkRootKey.getKeyBytes());
  }

  /**
   *
   * Creates and returns a GroupCall object.
   *
   * If there is any error when allocating resources for the object,
   * null is returned.
   *
   * @param groupId                the unique identifier for the group
   * @param sfuUrl                 the URL to use when accessing the SFU
   * @param hkdfExtraInfo          additional entropy to use for the connection with the SFU (it's okay if this is empty)
   * @param audioLevelsIntervalMs  if provided, the observer will receive audio level callbacks at this interval
   * @param audioProcessingMethod  the method to use for audio processing
   * @param useOboe                whether to use the oboe-based audio device module, otherwise use java
   * @param observer               the observer that the group call object will use for callback notifications
   *
   */
  @Nullable
  public GroupCall createGroupCall(@NonNull  byte[]                groupId,
                                   @NonNull  String                sfuUrl,
                                   @NonNull  byte[]                hkdfExtraInfo,
                                   @Nullable Integer               audioLevelsIntervalMs,
                                             AudioProcessingMethod audioProcessingMethod,
                                             boolean               useOboe,
                                   @NonNull  GroupCall.Observer    observer)
  {
    checkCallManagerExists();

    if (this.groupFactory == null) {
      // The first GroupCall object will create a factory that will be re-used.
      this.groupFactory = this.createPeerConnectionFactory(null, audioProcessingMethod, useOboe);
      if (this.groupFactory == null) {
        Log.e(TAG, "createPeerConnectionFactory failed");
        return null;
      }
    }

    GroupCall groupCall = GroupCall.create(nativeCallManager, groupId, sfuUrl, hkdfExtraInfo, audioLevelsIntervalMs, this.groupFactory, observer);

    if (groupCall != null) {
      // Add the groupCall to the map.
      this.groupCallByClientId.append(groupCall.clientId, groupCall);
    }

    return groupCall;
  }

  /**
   *
   * Creates and returns a GroupCall object for a call link call.
   *
   * If there is any error when allocating resources for the object,
   * null is returned.
   *
   * @param sfuUrl                     the URL to use when accessing the SFU
   * @param authCredentialPresentation a serialized CallLinkAuthCredentialPresentation
   * @param linkRootKey                the root key for the call link
   * @param adminPasskey               if present, the opaque passkey authorizing this user as an admin for the call link
   * @param hkdfExtraInfo              additional entropy to use for the connection with the SFU (it's okay if this is empty)
   * @param audioLevelsIntervalMs      if provided, the observer will receive audio level callbacks at this interval
   * @param audioProcessingMethod      the method to use for audio processing
   * @param useOboe                    whether to use the oboe-based audio device module, otherwise use java
   * @param observer                   the observer that the group call object will use for callback notifications
   *
   * @throws CallException for native code failures
   *
   */
  @Nullable
  public GroupCall createCallLinkCall(@NonNull  String                sfuUrl,
                                      @NonNull  byte[]                authCredentialPresentation,
                                      @NonNull  CallLinkRootKey       linkRootKey,
                                      @Nullable byte[]                adminPasskey,
                                      @NonNull  byte[]                hkdfExtraInfo,
                                      @Nullable Integer               audioLevelsIntervalMs,
                                                AudioProcessingMethod audioProcessingMethod,
                                                boolean               useOboe,
                                      @NonNull  GroupCall.Observer    observer)
  {
    checkCallManagerExists();

    if (this.groupFactory == null) {
      // The first GroupCall object will create a factory that will be re-used.
      this.groupFactory = this.createPeerConnectionFactory(null, audioProcessingMethod, useOboe);
      if (this.groupFactory == null) {
        Log.e(TAG, "createPeerConnectionFactory failed");
        return null;
      }
    }

    GroupCall groupCall = GroupCall.create(nativeCallManager, sfuUrl, authCredentialPresentation, linkRootKey, adminPasskey, hkdfExtraInfo, audioLevelsIntervalMs, this.groupFactory, observer);

    if (groupCall != null) {
      // Add the groupCall to the map.
      this.groupCallByClientId.append(groupCall.clientId, groupCall);
    }

    return groupCall;
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
  private Connection createConnection(long        nativeConnectionBorrowed,
                                      long        nativeCallId,
                                      int         remoteDeviceId,
                                      CallContext callContext,
                                      int         audioJitterBufferMaxPackets,
                                      int         audioJitterBufferMaxTargetDelayMs) {

    CallId callId = new CallId(nativeCallId);

    Log.i(TAG, "createConnection(): connectionId: " + callId.format(remoteDeviceId));

    MediaConstraints                constraints   = new MediaConstraints();
    PeerConnection.RTCConfiguration configuration = new PeerConnection.RTCConfiguration(callContext.iceServers);

    configuration.sdpSemantics  = PeerConnection.SdpSemantics.UNIFIED_PLAN;
    configuration.bundlePolicy  = PeerConnection.BundlePolicy.MAXBUNDLE;
    configuration.rtcpMuxPolicy = PeerConnection.RtcpMuxPolicy.REQUIRE;
    configuration.tcpCandidatePolicy = PeerConnection.TcpCandidatePolicy.DISABLED;
    configuration.continualGatheringPolicy = PeerConnection.ContinualGatheringPolicy.GATHER_CONTINUALLY;

    if (callContext.hideIp) {
      configuration.iceTransportsType = PeerConnection.IceTransportsType.RELAY;
    }

    configuration.audioJitterBufferMaxPackets       = audioJitterBufferMaxPackets;
    configuration.audioJitterBufferMaxTargetDelayMs = audioJitterBufferMaxTargetDelayMs;

    PeerConnectionFactory factory       = callContext.factory;
    CameraControl         cameraControl = callContext.cameraControl;
    try {
      long nativePeerConnection = ringrtcCreatePeerConnection(factory.getNativeOwnedFactoryAndThreads(),
                                                              nativeConnectionBorrowed,
                                                              configuration,
                                                              constraints);
      if (nativePeerConnection == 0) {
        Log.w(TAG, "Unable to create native PeerConnection.");
        return null;
      }

      Connection connection = new Connection(new Connection.NativeFactory(nativePeerConnection,
                                                                          callId,
                                                                          remoteDeviceId));

      connection.setAudioPlayout(false);
      connection.setAudioRecording(false);

      MediaConstraints audioConstraints = new MediaConstraints();

      AudioSource audioSource = factory.createAudioSource(audioConstraints);
      // Note: This must stay "audio1" to stay in sync with V4 signaling.
      AudioTrack  audioTrack  = factory.createAudioTrack("audio1", audioSource);
      audioTrack.setEnabled(false);

      connection.addTrack(audioTrack, Collections.singletonList("s"));
      if (callContext.videoTrack != null) {
        connection.addTrack(callContext.videoTrack, Collections.singletonList("s"));
      }

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
    observer.onStartCall(remote, new CallId(callId), isOutgoing, callMediaType);
  }

  @CalledByNative
  private void onEvent(Remote remote, CallEvent event) {
    Log.i(TAG, "onEvent():");
    observer.onCallEvent(remote, event);
  }

  @CalledByNative
  private void onNetworkRouteChanged(Remote remote, int localNetworkAdapterType) {
    Log.i(TAG, "onNetworkRouteChange():");

    NetworkRoute networkRoute = new NetworkRoute(NetworkAdapterTypeFromRawValue(localNetworkAdapterType));

    observer.onNetworkRouteChanged(remote, networkRoute);
  }

  @CalledByNative
  private void onAudioLevels(Remote remote, int capturedLevel, int receivedLevel) {
    observer.onAudioLevels(remote, capturedLevel, receivedLevel);
  }

  @CalledByNative
  private void onLowBandwidthForVideo(Remote remote, boolean recovered) {
    observer.onLowBandwidthForVideo(remote, recovered);
  }

  // A faster version of PeerConnection.AdapterType.fromNativeIndex.
  // It also won't return null.
  @NonNull
  private PeerConnection.AdapterType NetworkAdapterTypeFromRawValue(int localNetworkAdapterType) {
    switch(localNetworkAdapterType) {
      case 0:
        return PeerConnection.AdapterType.UNKNOWN;
      case 1:
        return PeerConnection.AdapterType.ETHERNET;
      case 2:
        return PeerConnection.AdapterType.WIFI;
      case 4:
        return PeerConnection.AdapterType.CELLULAR;
      case 8:
        return PeerConnection.AdapterType.VPN;
      case 16:
        return PeerConnection.AdapterType.LOOPBACK;
      case 32:
        return PeerConnection.AdapterType.ADAPTER_TYPE_ANY;
      case 64:
        return PeerConnection.AdapterType.CELLULAR_2G;
      case 128:
        return PeerConnection.AdapterType.CELLULAR_3G;
      case 256:
        return PeerConnection.AdapterType.CELLULAR_4G;
      case 512:
        return PeerConnection.AdapterType.CELLULAR_5G;
    }
    return PeerConnection.AdapterType.UNKNOWN;
  }

  @CalledByNative
  private void onCallConcluded(Remote remote) {
    Log.i(TAG, "onCallConcluded():");
    observer.onCallConcluded(remote);
  }

  @CalledByNative
  private void onSendOffer(long callId, Remote remote, int remoteDeviceId, boolean broadcast, @NonNull byte[] opaque, CallMediaType callMediaType) {
    Log.i(TAG, "onSendOffer():");
    observer.onSendOffer(new CallId(callId), remote, remoteDeviceId, broadcast, opaque, callMediaType);
  }

  @CalledByNative
  private void onSendAnswer(long callId, Remote remote, int remoteDeviceId, boolean broadcast, @NonNull byte[] opaque) {
    Log.i(TAG, "onSendAnswer():");
    observer.onSendAnswer(new CallId(callId), remote, remoteDeviceId, broadcast, opaque);
  }

  @CalledByNative
  private void onSendIceCandidates(long callId, Remote remote, int remoteDeviceId, boolean broadcast, List<byte[]> iceCandidates) {
    Log.i(TAG, "onSendIceCandidates():");
    observer.onSendIceCandidates(new CallId(callId), remote, remoteDeviceId, broadcast, iceCandidates);
  }

  @CalledByNative
  private void onSendHangup(long callId, Remote remote, int remoteDeviceId, boolean broadcast, HangupType hangupType, int deviceId) {
    Log.i(TAG, "onSendHangup():");
    observer.onSendHangup(new CallId(callId), remote, remoteDeviceId, broadcast, hangupType, deviceId);
  }

  @CalledByNative
  private void onSendBusy(long callId, Remote remote, int remoteDeviceId, boolean broadcast) {
    Log.i(TAG, "onSendBusy():");
    observer.onSendBusy(new CallId(callId), remote, remoteDeviceId, broadcast);
  }

  @CalledByNative
  private void sendCallMessage(@NonNull byte[] recipientUuid, @NonNull byte[] message, int urgency) {
    Log.i(TAG, "sendCallMessage():");
    observer.onSendCallMessage(Util.getUuidFromBytes(recipientUuid), message, CallMessageUrgency.values()[urgency]);
  }

  @CalledByNative
  private void sendCallMessageToGroup(@NonNull byte[] groupId, @NonNull byte[] message, int urgency, @NonNull List<byte[]> overrideRecipients) {
    Log.i(TAG, "sendCallMessageToGroup():");

    List<UUID> finalOverrideRecipients = new ArrayList<UUID>();
    for (byte[] recipient : overrideRecipients) {
      finalOverrideRecipients.add(Util.getUuidFromBytes(recipient));
    }

    observer.onSendCallMessageToGroup(groupId, message, CallMessageUrgency.values()[urgency], finalOverrideRecipients);
  }

  @CalledByNative
  private void sendHttpRequest(long requestId, String url, HttpMethod method, List<HttpHeader> headers, @Nullable byte[] body) {
    Log.i(TAG, "sendHttpRequest():");
    observer.onSendHttpRequest(requestId, url, method, headers, body);
  }

  @CalledByNative
  private boolean compareRemotes(Remote remote1, Remote remote2) {
    Log.i(TAG, "compareRemotes():");
    if (remote1 != null) {
      return remote1.recipientEquals(remote2);
    }
    return false;
  }

  // Group Calls

  @CalledByNative
  private void groupCallRingUpdate(@NonNull byte[] groupId, long ringId, @NonNull byte[] sender, int state) {
    Log.i(TAG, "groupCallRingUpdate():");
    observer.onGroupCallRingUpdate(groupId, ringId, Util.getUuidFromBytes(sender), RingUpdate.values()[state]);
  }

  @CalledByNative
  private void handlePeekResponse(long requestId, HttpResult<PeekInfo> info) {
    if (!this.peekRequests.resolve(requestId, info)) {
      Log.w(TAG, "Invalid requestId for handlePeekResponse: " + requestId);
    }
  }

  @CalledByNative
  private void handleCallLinkResponse(long requestId, HttpResult<CallLinkState> response) {
    if (!this.callLinkRequests.resolve(requestId, response)) {
      Log.w(TAG, "Invalid requestId for handleCallLinkResponse: " + requestId);
    }
  }

  // HttpResult's success value cannot be null, so we use a Boolean. The value of the boolean is ignored
  @CalledByNative
  private void handleEmptyResponse(long requestId, HttpResult<Boolean> response) {
    if (!this.emptyRequests.resolve(requestId, response)) {
      Log.w(TAG, "Invalid requestId for handleEmptyResponse: " + requestId);
    }
  }

  @CalledByNative
  private void requestMembershipProof(long clientId) {
    Log.i(TAG, "requestMembershipProof():");

    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }

    groupCall.requestMembershipProof();
  }

  @CalledByNative
  private void requestGroupMembers(long clientId) {
    Log.i(TAG, "requestGroupMembers():");

    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }

    groupCall.requestGroupMembers();
  }

  @CalledByNative
  private void handleConnectionStateChanged(long clientId, GroupCall.ConnectionState connectionState) {
    Log.i(TAG, "handleConnectionStateChanged():");

    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }

    groupCall.handleConnectionStateChanged(connectionState);
  }

  @CalledByNative
  private void handleNetworkRouteChanged(long clientId, int localNetworkAdapterType) {
    Log.i(TAG, "handleNetworkRouteChanged():");

    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }

    NetworkRoute networkRoute = new NetworkRoute(NetworkAdapterTypeFromRawValue(localNetworkAdapterType));
    groupCall.handleNetworkRouteChanged(networkRoute);
  }

  @CalledByNative
  private void handleAudioLevels(long clientId, int capturedLevel, List<GroupCall.ReceivedAudioLevel> receivedLevels) {
    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }

    groupCall.handleAudioLevels(capturedLevel, receivedLevels);
  }

  @CalledByNative
  private void handleLowBandwidthForVideo(long clientId, boolean recovered) {
    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }

    groupCall.handleLowBandwidthForVideo(recovered);
  }

  @CalledByNative
  private void handleReactions(long clientId, List<GroupCall.Reaction> reactions) {
    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }

    groupCall.handleReactions(reactions);
  }

  @CalledByNative
  private void handleRaisedHands(long clientId, List<Long> raisedHands) {
    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }

    groupCall.handleRaisedHands(raisedHands);
  }

  @CalledByNative
  private void handleJoinStateChanged(long clientId, GroupCall.JoinState joinState, Long demuxId) {
    Log.i(TAG, "handleJoinStateChanged():");

    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }

    groupCall.handleJoinStateChanged(joinState, demuxId);
  }

  @CalledByNative
  private void handleRemoteDevicesChanged(long clientId, List<GroupCall.RemoteDeviceState> remoteDeviceStates) {
    if (remoteDeviceStates != null) {
      Log.i(TAG, "handleRemoteDevicesChanged(): remoteDeviceStates.size = " + remoteDeviceStates.size());
    } else {
      Log.i(TAG, "handleRemoteDevicesChanged(): remoteDeviceStates is null!");
    }

    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }

    groupCall.handleRemoteDevicesChanged(remoteDeviceStates);
  }

  @CalledByNative
  private void handleIncomingVideoTrack(long clientId, long remoteDemuxId, long nativeVideoTrackBorrowedRc) {
    Log.i(TAG, "handleIncomingVideoTrack():");

    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }

    groupCall.handleIncomingVideoTrack(remoteDemuxId, nativeVideoTrackBorrowedRc);
  }

  @CalledByNative
  private void handlePeekChanged(long clientId, PeekInfo info) {
    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }
    groupCall.handlePeekChanged(info);
  }

  @CalledByNative
  private void handleEnded(long clientId, GroupCall.GroupCallEndReason reason) {
    Log.i(TAG, "handleEnded():");

    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }

    this.groupCallByClientId.delete(clientId);

    groupCall.handleEnded(reason);
  }

  @CalledByNative
  private void handleSpeakingNotification(long clientId, GroupCall.SpeechEvent event) {
    Log.i(TAG, "handleSpeakingNotification():");

    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }

    groupCall.handleSpeakingNotification(event);
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
    @NonNull  public final  PeerConnectionFactory          factory;
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

    public CallContext(@NonNull CallId                         callId,
                       @NonNull Context                        context,
                       @NonNull PeerConnectionFactory          factory,
                       @NonNull VideoSink                      localSink,
                       @NonNull VideoSink                      remoteSink,
                       @NonNull CameraControl                  camera,
                       @NonNull List<PeerConnection.IceServer> iceServers,
                                boolean                        hideIp) {

      Log.i(TAG, "ctor(): " + callId);

      this.callId        = callId;
      this.factory       = factory;
      this.remoteSink    = remoteSink;
      this.cameraControl = camera;
      this.iceServers    = iceServers;
      this.hideIp        = hideIp;

      // Create a video track that will be shared across all
      // connection objects.  It must be disposed manually.
      if (cameraControl.hasCapturer()) {
        this.videoSource = factory.createVideoSource(false);
        // Note: This must stay "video1" to stay in sync with V4 signaling.
        this.videoTrack  = factory.createVideoTrack("video1", videoSource);
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

      factory.dispose();
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

    /** The call ended because of glare, receiving an offer from same remote
        while calling them. */
    ENDED_REMOTE_GLARE,

    /** The call ended because of recall, receiving an offer from same remote
        while still in an existing call with them. */
    ENDED_REMOTE_RECALL,

    /** The call ended because it timed out during setup. */
    ENDED_TIMEOUT,

    /** The call ended because of an internal error condition. */
    ENDED_INTERNAL_FAILURE,

    /** The call ended because a signaling message couldn't be sent. */
    ENDED_SIGNALING_FAILURE,

    /** The call ended because there was a failure during glare handling. */
    ENDED_GLARE_HANDLING_FAILURE,

    /** The call ended because setting up the connection failed. */
    ENDED_CONNECTION_FAILURE,

    /** The call ended because the application wanted to drop the call. */
    ENDED_APP_DROPPED_CALL,

    /** The remote peer indicates its audio stream is enabled. */
    REMOTE_AUDIO_ENABLE,

    /** The remote peer indicates its audio stream is disabled. */
    REMOTE_AUDIO_DISABLE,

    /** The remote peer indicates its video stream is enabled. */
    REMOTE_VIDEO_ENABLE,

    /** The remote peer indicates its video stream is disabled. */
    REMOTE_VIDEO_DISABLE,

    /** The remote peer is sharing its screen. */
    REMOTE_SHARING_SCREEN_ENABLE,

    /** The remote peer is not (no longer) sharing its screen. */
    REMOTE_SHARING_SCREEN_DISABLE,

    /** The call dropped while connected and is now reconnecting. */
    RECONNECTING,

    /** The call dropped while connected and is now reconnected. */
    RECONNECTED,

    /** The received offer is expired. */
    RECEIVED_OFFER_EXPIRED,

    /** Received an offer while already handling an active call. */
    RECEIVED_OFFER_WHILE_ACTIVE,

    /** Received an offer while already handling an active call and glare was detected. */
    RECEIVED_OFFER_WITH_GLARE;

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
   * The data mode allows the client to limit the media bandwidth used.
   */
  public enum DataMode {

    /**
     * Intended for low bitrate video calls. Useful to reduce
     * bandwidth costs, especially on mobile data networks.
     */
    LOW,

    /**
     * (Default) No specific constraints, but keep a relatively
     * high bitrate to ensure good quality.
     */
    NORMAL;

    @CalledByNative
    static DataMode fromNativeIndex(int nativeIndex) {
        return values()[nativeIndex];
    }
  }

  /**
   *
   * The HTTP method to use when making a request
   *
   */
  public enum HttpMethod {

    /**  */
    GET,

    /**  */
    PUT,

    /**  */
    POST,

    /**  */
    DELETE;

    @CalledByNative
    static HttpMethod fromNativeIndex(int nativeIndex) {
      return values()[nativeIndex];
    }
  }

  public enum CallMessageUrgency {
    DROPPABLE,
    HANDLE_IMMEDIATELY,
  }

  public enum RingUpdate {
    /** The sender is trying to ring this user. */
    REQUESTED,
    /** The sender tried to ring this user, but it's been too long. */
    EXPIRED_REQUEST,
    /** Call was accepted elsewhere by a different device. */
    ACCEPTED_ON_ANOTHER_DEVICE,
    /** Call was declined elsewhere by a different device. */
    DECLINED_ON_ANOTHER_DEVICE,
    /** This device is currently on a different call. */
    BUSY_LOCALLY,
    /** A different device is currently on a different call. */
    BUSY_ON_ANOTHER_DEVICE,
    /** The sender cancelled the ring request. */
    CANCELLED_BY_RINGER,
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
     * Notification that the network route changed
     *
     * @param remote        remote peer of the call
     * @param networkRoute  the current network route
     */
    void onNetworkRouteChanged(Remote remote, NetworkRoute networkRoute);

    /**
     *
     * Notification of audio levels
     *
     * @param remote        remote peer of the call
     * @param capturedLevel the audio level captured locally.  Range of 0-32767, where 0 is silence.
     * @param receivedLevel the audio level received from the remote peer.  Range of 0-32767, where 0 is silence.
     */
    void onAudioLevels(Remote remote, int capturedLevel, int receivedLevel);

    /**
     *
     * Notification of low upload bandwidth for sending video.
     *
     * When this is first called, recovered will be false. The second call (if
     * any) will have recovered set to true and will be called when the upload
     * bandwidth is high enough to send video.
     *
     * @param remote     remote peer of the call
     * @param recovered  whether there is enough bandwidth to send video
     *                   reliably
     */
    void onLowBandwidthForVideo(Remote remote, boolean recovered);

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
     * Notification that an offer is ready to be sent
     *
     * @param callId          callId for the call
     * @param remote          remote peer of the outgoing call
     * @param remoteDeviceId  deviceId of remote peer
     * @param broadcast       if true, send broadcast message
     * @param opaque          the opaque offer
     * @param callMediaType   the origination type for the call, audio or video
     *
     */
    void onSendOffer(CallId callId, Remote remote, Integer remoteDeviceId, Boolean broadcast, @NonNull byte[] opaque, CallMediaType callMediaType);

    /**
     *
     * Notification that an answer is ready to be sent
     *
     * @param callId          callId for the call
     * @param remote          remote peer of the outgoing call
     * @param remoteDeviceId  deviceId of remote peer
     * @param broadcast       if true, send broadcast message
     * @param opaque          the opaque answer
     *
     */
    void onSendAnswer(CallId callId, Remote remote, Integer remoteDeviceId, Boolean broadcast, @NonNull byte[] opaque);

    /**
     *
     * Notification that ICE candidates are ready to be sent
     *
     * @param callId          callId for the call
     * @param remote          remote peer of the outgoing call
     * @param remoteDeviceId  deviceId of remote peer
     * @param broadcast       if true, send broadcast message
     * @param iceCandidates   ICE candidates
     *
     */
    void onSendIceCandidates(CallId callId, Remote remote, Integer remoteDeviceId, Boolean broadcast, List<byte[]> iceCandidates);

    /**
     *
     * Notification that hangup message is ready to be sent
     *
     * @param callId                  callId for the call
     * @param remote                  remote peer of the call
     * @param remoteDeviceId          deviceId of remote peer
     * @param broadcast               if true, send broadcast message
     * @param hangupType              type of hangup, normal or handled elsewhere
     * @param deviceId                if not a normal hangup, the associated deviceId
     *
     */
    void onSendHangup(CallId callId, Remote remote, Integer remoteDeviceId, Boolean broadcast, HangupType hangupType, Integer deviceId);

    /**
     *
     * Notification that busy message is ready to be sent
     *
     * @param callId          callId for the call
     * @param remote          remote peer of the incoming busy call
     * @param remoteDeviceId  deviceId of remote peer
     * @param broadcast       if true, send broadcast message
     *
     */
    void onSendBusy(CallId callId, Remote remote, Integer remoteDeviceId, Boolean broadcast);

    /**
     *
     * Send a generic call message to the given remote recipient.
     *
     * @param recipientUuid  UUID for the user to send the message to
     * @param message        the opaque bytes to send
     * @param urgency        controls whether recipients should immediately handle this message
     */
    void onSendCallMessage(@NonNull UUID recipientUuid, @NonNull byte[] message, @NonNull CallMessageUrgency urgency);

    /**
     *
     * Send a generic call message to a group. Send to all members of the group
     * or, if overrideRecipients is not empty, send to the given subset of members
     * using multi-recipient sealed sender. If the sealed sender request fails,
     * clients should provide a fallback mechanism.
     *
     * @param groupId             the ID of the group in question
     * @param message             the opaque bytes to send
     * @param urgency             controls whether recipients should immediately handle this message
     * @param overrideRecipients  a subset of group members to send to; if empty, send to all
     */
    void onSendCallMessageToGroup(@NonNull byte[] groupId, @NonNull byte[] message, @NonNull CallMessageUrgency urgency, @NonNull List<UUID> overrideRecipients);

    /**
     *
     * A HTTP request should be sent to the given url.
     *
     * @param requestId
     * @param url
     * @param method
     * @param headers
     * @param body
     *
     */
    void onSendHttpRequest(long requestId, @NonNull String url, @NonNull HttpMethod method, @Nullable List<HttpHeader> headers, @Nullable byte[] body);

    /**
     *
     * A group ring request or cancellation should be handled.
     *
     * @param groupId the ID of the group
     * @param ringId  uniquely identifies the original ring request
     * @param sender  the user responsible for the update (which may be the local user)
     * @param update  the updated state to handle
     */
    void onGroupCallRingUpdate(@NonNull byte[] groupId, long ringId, @NonNull UUID sender, RingUpdate update);
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
    void ringrtcSetSelfUuid(long nativeCallManager, byte[] uuid)
    throws CallException;

  private native
    long ringrtcCreatePeerConnection(long                            nativePeerConnectionFactory,
                                     long                            nativeConnection,
                                     PeerConnection.RTCConfiguration rtcConfig,
                                     MediaConstraints                constraints)
    throws CallException;

  private native
    void ringrtcCall(long nativeCallManager, Remote remote, int callMediaType, int localDeviceId)
    throws CallException;

  private native
    void ringrtcProceed(long        nativeCallManager,
                        long        callId,
                        CallContext callContext,
                        int         dataMode,
                        int         audioLevelsIntervalMillis)
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
    void ringrtcCancelGroupRing(long nativeCallManager, byte[] groupId, long ringId, int reason)
    throws CallException;

  private native
    void ringrtcReceivedAnswer(long    nativeCallManager,
                               long    callId,
                               int     remoteDeviceId,
                               byte[]  opaque,
                               byte[]  senderIdentityKey,
                               byte[]  receiverIdentityKey)
    throws CallException;

  private native
    void ringrtcReceivedOffer(long    nativeCallManager,
                              long    callId,
                              Remote  remote,
                              int     remoteDeviceId,
                              byte[]  opaque,
                              long    messageAgeSec,
                              int     callMediaType,
                              int     localDeviceId,
                              byte[]  senderIdentityKey,
                              byte[]  receiverIdentityKey)
    throws CallException;

  private native
    void ringrtcReceivedIceCandidates(long         nativeCallManager,
                                      long         callId,
                                      int          remoteDeviceId,
                                      List<byte[]> iceCandidates)
    throws CallException;

  private native
    void ringrtcReceivedHangup(long nativeCallManager,
                               long callId,
                               int  remoteDeviceId,
                               int  hangupType,
                               int  deviceId)
    throws CallException;

  private native
    void ringrtcReceivedBusy(long nativeCallManager,
                             long callId,
                             int  remoteDeviceId)
    throws CallException;

  private native
    void ringrtcReceivedCallMessage(long   nativeCallManager,
                                    byte[] senderUuid,
                                    int    senderDeviceId,
                                    int    localDeviceId,
                                    byte[] message,
                                    long   messageAgeSec)
    throws CallException;

  private native
    void ringrtcReceivedHttpResponse(long   nativeCallManager,
                                     long   requestId,
                                     int    status,
                                     byte[] body)
    throws CallException;

  private native
    void ringrtcHttpRequestFailed(long nativeCallManager,
                                  long requestId)
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
    void ringrtcSetAudioEnable(long nativeCallManager, boolean enable)
    throws CallException;

  private native
    void ringrtcSetVideoEnable(long nativeCallManager, boolean enable)
    throws CallException;

  private native
    void ringrtcUpdateDataMode(long nativeCallManager, int dataMode)
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

  private native
    void ringrtcPeekGroupCall(long   nativeCallManager,
                              long   requestId,
                              String sfuUrl,
                              byte[] membershipProof,
                              byte[] serializedGroupMembers)
    throws CallException;

  private native
    void ringrtcReadCallLink(long   nativeCallManager,
                             String sfuUrl,
                             byte[] authCredentialPresentation,
                             byte[] rootKeyBytes,
                             long   requestId)
    throws CallException;

  private native
    void ringrtcCreateCallLink(long   nativeCallManager,
                               String sfuUrl,
                               byte[] createCredentialPresentation,
                               byte[] rootKeyBytes,
                               byte[] adminPasskey,
                               byte[] callLinkPublicParams,
                               int    restrictions,
                               long   requestId)
    throws CallException;

  private native
    void ringrtcUpdateCallLink(long   nativeCallManager,
                               String sfuUrl,
                               byte[] authCredentialPresentation,
                               byte[] rootKeyBytes,
                               byte[] adminPasskey,
                               String newName,
                               int    newRestrictions,
                               int    newRevoked,
                               long   requestId)
    throws CallException;
  
  private native
    void ringrtcDeleteCallLink(long   nativeCallManager,
                               String sfuUrl,
                               byte[] authCredentialPresentation,
                               byte[] rootKeyBytes,
                               byte[] adminPasskey,
                               long   requestId)
    throws CallException;

  private native
    void ringrtcPeekCallLinkCall(long   nativeCallManager,
                                 long   requestId,
                                 String sfuUrl,
                                 byte[] authCredentialPresentation,
                                 byte[] rootKeyBytes)
    throws CallException;
}
