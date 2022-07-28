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

import java.util.ArrayList;
import java.util.Collection;
import java.util.HashSet;
import java.util.Set;
import java.util.List;
import java.util.UUID;

/**
 *
 * Provides an interface to the RingRTC Call Manager.
 *
 */
public class CallManager {

  @NonNull
  private static final String                     TAG = CallManager.class.getSimpleName();

  private static       boolean                    isInitialized;

  private              long                       nativeCallManager;

  @NonNull
  private              Observer                   observer;

  // Keep a hash/mapping of a callId to a GroupCall object. CallId is a u32
  // and will fit in to the long type.
  @NonNull
  private              LongSparseArray<GroupCall> groupCallByClientId;

  @NonNull
  private              Requests<PeekInfo>         peekInfoRequests;

  @Nullable
  private              PeerConnectionFactory      groupFactory;

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
        // Show all WebRTC logs via application Logger while debugging.
        builder.setInjectableLogger(new WebRtcLogger(), Severity.LS_INFO);
      } else {
        // Show WebRTC error and warning logs via application Logger for release builds.
        builder.setInjectableLogger(new WebRtcLogger(), Severity.LS_WARNING);
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

  class PeerConnectionFactoryOptions extends PeerConnectionFactory.Options {
    public PeerConnectionFactoryOptions() {
      // Give the (native default) behavior of filtering out loopback addresses.
      // See https://source.chromium.org/chromium/chromium/src/+/master:third_party/webrtc/rtc_base/network.h;l=47?q=.networkIgnoreMask&ss=chromium
      this.networkIgnoreMask = 1 << 4;
    }
  }

  /// Creates a PeerConnectionFactory appropriate for our use of WebRTC.
  ///
  /// If `eglBase` is present, hardware codecs will be used unless they are known to be broken
  /// in some way. Otherwise, we'll fall back to software codecs.
  private PeerConnectionFactory createPeerConnectionFactory(@Nullable EglBase               eglBase,
                                                                      AudioProcessingMethod audioProcessingMethod) {
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

    // This is a workaround to what appears to a bug in WebRTC. If you don't call
    // setAudioDeviceModule, then the default ADM created by WebRTC will not have
    // .release() called, and the ADM will be leaked. This also lets us control the
    // use of hardware or software voice processing.
    JavaAudioDeviceModule adm = createAudioDeviceModule(audioProcessingMethod);
    PeerConnectionFactory factory = PeerConnectionFactory.builder()
            .setOptions(new PeerConnectionFactoryOptions())
            .setAudioDeviceModule(adm)
            .setVideoEncoderFactory(encoderFactory)
            .setVideoDecoderFactory(decoderFactory)
            .createPeerConnectionFactory();
    adm.release();
    return factory;
  }

  /// Defines the method to use for audio processing of AEC and NS.
  public enum AudioProcessingMethod {
    Default,
    ForceHardware,
    ForceSoftwareAec3,
    ForceSoftwareAecM
  }

  static JavaAudioDeviceModule createAudioDeviceModule(AudioProcessingMethod audioProcessingMethod) {
    // We'll set both AEC and NS equally to be either both hardware or
    // both software, assuming that they are co-tuned.
    boolean useHardware;
    boolean useAecM;

    switch(audioProcessingMethod) {
      case ForceSoftwareAecM:
        useHardware = false;
        useAecM = true;
        break;
      case ForceSoftwareAec3:
        useHardware = false;
        useAecM = false;
        break;
      default:
        useHardware = true;
        useAecM = false;
        break;
    }

    Log.i(TAG, "createAudioDeviceModule(): useHardware: " + useHardware + " useAecM: " + useAecM);

    return JavaAudioDeviceModule.builder(ContextUtils.getApplicationContext())
      .setUseHardwareAcousticEchoCanceler(useHardware)
      .setUseHardwareNoiseSuppressor(useHardware)
      .setUseAecm(useAecM)
      .createAudioDeviceModule();
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
    this.peekInfoRequests    = new Requests<>();
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
   * @param localSink              local video sink to use for this Call
   * @param remoteSink             remote video sink to use for this Call
   * @param camera                 camera control to use for this Call
   * @param iceServers             list of ICE servers to use for this Call
   * @param hideIp                 if true hide caller's IP by using a TURN server
   * @param bandwidthMode          desired bandwidth mode to start the session with
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
                      @NonNull  VideoSink                      localSink,
                      @NonNull  VideoSink                      remoteSink,
                      @NonNull  CameraControl                  camera,
                      @NonNull  List<PeerConnection.IceServer> iceServers,
                                boolean                        hideIp,
                                BandwidthMode                  bandwidthMode,
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

    PeerConnectionFactory factory = this.createPeerConnectionFactory(eglBase, audioProcessingMethod);

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
                   bandwidthMode.ordinal(),
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
   * @param isLocalDevicePrimary     if true, the local device is considered a primary device
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
                                     boolean       isLocalDevicePrimary,
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
                         isLocalDevicePrimary,
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
   * Allows the application to constrain bandwidth if so configured
   * by the user.
   *
   * @param bandwidthMode  one of the BandwidthMode enumerated values
   *
   * @throws CallException for native code failures
   *
   */
  public void updateBandwidthMode(BandwidthMode bandwidthMode)
    throws CallException
  {
    checkCallManagerExists();

    ringrtcUpdateBandwidthMode(nativeCallManager, bandwidthMode.ordinal());
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

  // Group Calls

  public interface ResponseHandler<T> {
    void handleResponse(T response);
  }

  static class Requests<T> {
    private long nextId = 1;
    @NonNull private LongSparseArray<ResponseHandler<T>> handlerById = new LongSparseArray();
  
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

    long requestId = this.peekInfoRequests.add(handler);
    ringrtcPeekGroupCall(nativeCallManager, requestId, sfuUrl, membershipProof, Util.serializeFromGroupMemberInfo(groupMembers));
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
   * @param audioProcessingMethod  the method to use for audio processing
   * @param observer               the observer that the group call object will use for callback notifications
   *
   */
  public GroupCall createGroupCall(@NonNull  byte[]                groupId,
                                   @NonNull  String                sfuUrl,
                                   @NonNull  byte[]                hkdfExtraInfo,
                                   @Nullable Integer               audioLevelsIntervalMs,
                                             AudioProcessingMethod audioProcessingMethod,
                                   @NonNull  GroupCall.Observer    observer)
  {
    checkCallManagerExists();

    if (this.groupFactory == null) {
      // The first GroupCall object will create a factory that will be re-used.
      this.groupFactory = this.createPeerConnectionFactory(null, audioProcessingMethod);
      if (this.groupFactory == null) {
        Log.e(TAG, "createPeerConnectionFactory failed");
        return null;
      }
    }

    GroupCall groupCall = new GroupCall(nativeCallManager, groupId, sfuUrl, hkdfExtraInfo, audioLevelsIntervalMs, this.groupFactory, observer);

    if (groupCall.clientId != 0) {
      // Add the groupCall to the map.
      this.groupCallByClientId.append(groupCall.clientId, groupCall);

      return groupCall;
    } else {
      try {
        groupCall.dispose();
      } catch (CallException e) {
        Log.e(TAG, "Unable to properly dispose of GroupCall", e);
      }

      return null;
    }
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
                                      CallContext callContext) {

    CallId callId = new CallId(nativeCallId);

    Log.i(TAG, "createConnection(): connectionId: " + callId.format(remoteDeviceId));

    MediaConstraints                constraints   = new MediaConstraints();
    PeerConnection.RTCConfiguration configuration = new PeerConnection.RTCConfiguration(callContext.iceServers);

    configuration.sdpSemantics  = PeerConnection.SdpSemantics.PLAN_B;
    configuration.bundlePolicy  = PeerConnection.BundlePolicy.MAXBUNDLE;
    configuration.rtcpMuxPolicy = PeerConnection.RtcpMuxPolicy.REQUIRE;
    configuration.tcpCandidatePolicy = PeerConnection.TcpCandidatePolicy.DISABLED;
    configuration.continualGatheringPolicy = PeerConnection.ContinualGatheringPolicy.GATHER_CONTINUALLY;

    if (callContext.hideIp) {
      configuration.iceTransportsType = PeerConnection.IceTransportsType.RELAY;
    }

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

      MediaStream      mediaStream      = factory.createLocalMediaStream("ARDAMS");
      MediaConstraints audioConstraints = new MediaConstraints();

      AudioSource audioSource = factory.createAudioSource(audioConstraints);
      // Note: This must stay "audio1" to stay in sync with V4 signaling.
      AudioTrack  audioTrack  = factory.createAudioTrack("audio1", audioSource);
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
  private void sendCallMessageToGroup(@NonNull byte[] groupId, @NonNull byte[] message, int urgency) {
    Log.i(TAG, "sendCallMessageToGroup():");
    observer.onSendCallMessageToGroup(groupId, message, CallMessageUrgency.values()[urgency]);
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
  private void handlePeekResponse(long requestId, List<byte[]> joinedMembers, @Nullable byte[] creator, @Nullable String eraId, @Nullable Long maxDevices, long deviceCount) {
    if (joinedMembers != null) {
      Log.i(TAG, "handlePeekResponse(): joinedMembers.size = " + joinedMembers.size());
    } else {
      Log.i(TAG, "handlePeekResponse(): joinedMembers is null");
    }

    // Create the collection, converting each provided byte[] to a UUID.
    Collection<UUID> joinedGroupMembers = new ArrayList<UUID>();
    for (byte[] joinedMember : joinedMembers) {
        joinedGroupMembers.add(Util.getUuidFromBytes(joinedMember));
    }

    PeekInfo info = new PeekInfo(joinedGroupMembers, creator == null ? null : Util.getUuidFromBytes(creator), eraId, maxDevices, deviceCount);

    if (!this.peekInfoRequests.resolve(requestId, info)) {
      Log.w(TAG, "Invalid requestId for handlePeekResponse: " + requestId);
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
  private void handleJoinStateChanged(long clientId, GroupCall.JoinState joinState) {
    Log.i(TAG, "handleJoinStateChanged():");

    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }

    groupCall.handleJoinStateChanged(joinState);
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
  private void handlePeekChanged(long clientId, List<byte[]> joinedMembers, @Nullable byte[] creator, @Nullable String eraId, @Nullable Long maxDevices, long deviceCount) {
    if (joinedMembers != null) {
      Log.i(TAG, "handlePeekChanged(): joinedMembers.size = " + joinedMembers.size());
    } else {
      Log.i(TAG, "handlePeekChanged(): joinedMembers is null");
    }

    GroupCall groupCall = this.groupCallByClientId.get(clientId);
    if (groupCall == null) {
      Log.w(TAG, "groupCall not found by clientId: " + clientId);
      return;
    }

    // Create the collection, converting each provided byte[] to a UUID.
    Collection<UUID> joinedGroupMembers = new ArrayList<UUID>();
    for (byte[] joinedMember : joinedMembers) {
        joinedGroupMembers.add(Util.getUuidFromBytes(joinedMember));
    }

    PeekInfo info = new PeekInfo(joinedGroupMembers, creator == null ? null : Util.getUuidFromBytes(creator), eraId, maxDevices, deviceCount);

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
   * Modes of operation when working with different bandwidth environments.
   */
  public enum BandwidthMode {

    /**
     * Intended for audio-only, to help ensure reliable audio over
     * severely constrained networks.
     */
    VERY_LOW,

    /**
     * Intended for low bitrate video calls. Useful to reduce
     * bandwidth costs, especially on mobile networks.
     */
    LOW,

    /**
     * (Default) No specific constraints, but keep a relatively
     * high bitrate to ensure good quality.
     */
    NORMAL;

    @CalledByNative
    static BandwidthMode fromNativeIndex(int nativeIndex) {
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
     * @param remote        remote peer of the incoming busy call
     * @param networkRoute  the current network route
     */
    void onNetworkRouteChanged(Remote remote, NetworkRoute networkRoute);

    /**
     *
     * Notification of audio levels
     *
     * @param remote        remote peer of the incoming busy call
     * @param capturedLevel the audio level captured locally.  Range of 0-32767, where 0 is silence.
     * @param receivedLevel the audio level received from the remote peer.  Range of 0-32767, where 0 is silence.
     */
    void onAudioLevels(Remote remote, int capturedLevel, int receivedLevel);

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
     * A message that should be sent to the given user as a CallMessage.
     *
     * @param recipientUuid  UUID for the user to send the message to
     * @param message        the opaque bytes to send
     * @param urgency        controls whether recipients should immediately handle this message.
     *                       Affects out-of-app message processing.
     */
    void onSendCallMessage(@NonNull UUID recipientUuid, @NonNull byte[] message, @NonNull CallMessageUrgency urgency);

    /**
     *
     * A message that should be sent to all members of the given group as a CallMessage.
     *
     * @param groupId                  the ID of the group in question
     * @param message                  the opaque bytes to send
     * @param urgency        controls whether recipients should immediately handle this message.
     *                       Affects out-of-app message processing.
     */
    void onSendCallMessageToGroup(@NonNull byte[] groupId, @NonNull byte[] message, @NonNull CallMessageUrgency urgency);

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
                        int         bandwidthMode,
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
                              boolean isLocalDevicePrimary,
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
    void ringrtcSetVideoEnable(long nativeCallManager, boolean enable)
    throws CallException;

  private native
    void ringrtcUpdateBandwidthMode(long nativeCallManager, int bandwidthMode)
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
}
