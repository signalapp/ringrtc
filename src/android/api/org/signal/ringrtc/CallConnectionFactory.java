/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

package org.signal.ringrtc;

import android.content.Context;
import android.support.annotation.NonNull;
import android.support.annotation.Nullable;
import android.os.Build;

import java.io.IOException;
import java.util.LinkedList;
import java.util.List;

import org.webrtc.AudioSource;
import org.webrtc.AudioTrack;
import org.webrtc.Logging.Severity;
import org.webrtc.MediaConstraints;
import org.webrtc.MediaStream;
import org.webrtc.NativeLibraryLoader;
import org.webrtc.PeerConnection;
import org.webrtc.PeerConnectionFactory;
import org.webrtc.SSLCertificateVerifier;
import org.webrtc.VideoDecoderFactory;
import org.webrtc.VideoEncoderFactory;
import org.webrtc.VideoSource;
import org.webrtc.VideoTrack;


/**
 *
 * CallConnection object factory
 *
 * <ul>
 *      <li> Global one time initialization of the underlying system
 *      <li> Creating CallConnection instances.
 * </ul>
 * @see CallConnection
 */
public class CallConnectionFactory {

  private static final String TAG = CallConnectionFactory.class.getSimpleName();
  private static boolean      isInitialized;
  private long                nativeCallConnectionFactory;

  @NonNull  protected Context               context;
  @Nullable private   PeerConnectionFactory peerConnectionFactory;

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
      if (buildInfo.debug) {
        // Log WebRtc internals via application Logger.
        builder.setInjectableLogger(new WebRtcLogger(), Severity.LS_INFO);
      }
      Log.i(TAG, "CallManager.initialize(): (" + (buildInfo.debug ? "debug" : "release") + " build)");

      PeerConnectionFactory.initialize(builder.createInitializationOptions());
      ringrtcInitialize();
      CallConnectionFactory.isInitialized = true;
      Log.i(TAG, "CallConnectionFactory.initialize() returned");
    } catch (UnsatisfiedLinkError e) {
      Log.w(TAG, "Unable to load ringrtc library", e);
    } catch  (CallException e) {
      Log.w(TAG, "ringrtc library initialization failure", e);
    }

  }

  private static void checkInitializeHasBeenCalled() {
    if (!CallConnectionFactory.isInitialized) {
      throw new IllegalStateException(
          "CallConnectionFactory.initialize was not called before creating a "
          + "CallConnectionFactory.");
    }
  }

  private void checkCallConnectionFactoryExists() {
    if ((peerConnectionFactory == null) || (nativeCallConnectionFactory == 0)) {
      throw new IllegalStateException("CallConnectionFactory has been disposed.");
    }
  }

  CallConnectionFactory(@NonNull Context context,
                        @NonNull VideoEncoderFactory encoderFactory,
                        @NonNull VideoDecoderFactory decoderFactory) {

    Log.i(TAG, "CallConnectionFactory() called");
    checkInitializeHasBeenCalled();

    this.context = context;
    this.peerConnectionFactory = PeerConnectionFactory.builder()
      .setOptions(new PeerConnectionFactoryOptions())
      .setVideoEncoderFactory(encoderFactory)
      .setVideoDecoderFactory(decoderFactory)
      .createPeerConnectionFactory();

  }

  /**
   *
   * Static method for creating the CallConnectionFactory object
   *
   * @param context          The context where the CallConnectionFactory object is created
   * @param encoderFactory   Video encoder factory to use
   * @param decoderFactory   Video decoder factory to use
   *
   * @return Newly created CallConnectionFactory object
   */
  @Nullable
  public static CallConnectionFactory createCallConnectionFactory(@NonNull Context context,
                                                                  @NonNull VideoEncoderFactory encoderFactory,
                                                                  @NonNull VideoDecoderFactory decoderFactory) {
    Log.i(TAG, "createCallConnectionFactory() called");

    CallConnectionFactory callConnectionFactory = new CallConnectionFactory(context,
                                                                            encoderFactory,
                                                                            decoderFactory);

    long nativeFactory = callConnectionFactory.peerConnectionFactory.getNativeOwnedFactoryAndThreads();
    long nativeCallConnectionFactory = ringrtcCreateCallConnectionFactory(nativeFactory);

    if (nativeCallConnectionFactory != 0) {
      callConnectionFactory.nativeCallConnectionFactory = nativeCallConnectionFactory;
      return callConnectionFactory;
    } else {
      return null;
    }

  }

  /**
   *
   * Close down and dispose of the CallConnectionFactory object
   *
   */
  public void dispose() {
    checkCallConnectionFactoryExists();

    Log.i(TAG, "CallConnectionFactory.dispose(): calling peerConnectionFactory.dispose()");
    this.peerConnectionFactory.dispose();

    this.peerConnectionFactory = null;

    Log.i(TAG, "CallConnectionFactory.dispose(): calling nativeFreeFactory()");
    ringrtcFreeFactory(this.nativeCallConnectionFactory);
    this.nativeCallConnectionFactory = 0;

  }

  @Nullable
  CallConnection createCallConnectionInternal(@NonNull CallConnection.Configuration callConfiguration,
                                              @NonNull CallConnection.Observer observer,
                                              @NonNull PeerConnection.RTCConfiguration rtcConfig,
                                              @NonNull MediaConstraints constraints,
                                              @NonNull SSLCertificateVerifier sslCertificateVerifier)
    throws IOException, CallException
  {

    Log.i(TAG, "createCallConnectionInternal()");
    checkCallConnectionFactoryExists();

    long nativeObserver = CallConnection.createNativeCallConnectionObserver(observer,
                                                                            callConfiguration.callId,
                                                                            callConfiguration.recipient);
    if (nativeObserver == 0) {
      return null;
    }
    long nativeCallConnection = ringrtcCreateCallConnection(this.nativeCallConnectionFactory,
                                                            callConfiguration,
                                                            nativeObserver,
                                                            rtcConfig,
                                                            constraints,
                                                            sslCertificateVerifier);
    if (nativeCallConnection == 0) {
      Log.w(TAG, "Unable to create native CallConnection()");
      return null;
    }

    CallConnection callConnection = new CallConnection(new CallConnection.NativeFactory(nativeCallConnection,
                                                                                        this,
                                                                                        callConfiguration.callId,
                                                                                        callConfiguration.recipient));

    return callConnection;
  }

  /**
   *
   * Factory method for creating CallConnection objects
   *
   * @param callConfiguration    Parameters that parameterize the call
   * @param observer             Observer object to handle CallConnection callbacks
   *
   * @throws IOException    possible network failure fetching TURN servers
   * @throws CallException  internal native library failure
   *
   * @return Newly created CallConnection object
   */
  @Nullable
  public CallConnection createCallConnection(@NonNull CallConnection.Configuration callConfiguration,
                                             @NonNull CallConnection.Observer observer)
    throws IOException, CallException
  {
    List<PeerConnection.IceServer> iceServers = new LinkedList<>();
    MediaConstraints                constraints      = new MediaConstraints();
    PeerConnection.RTCConfiguration configuration    = new PeerConnection.RTCConfiguration(iceServers);

    configuration.bundlePolicy  = PeerConnection.BundlePolicy.MAXBUNDLE;
    configuration.rtcpMuxPolicy = PeerConnection.RtcpMuxPolicy.REQUIRE;

    if (callConfiguration.hideIp) {
      configuration.iceTransportsType = PeerConnection.IceTransportsType.RELAY;
    }

    constraints.optional.add(new MediaConstraints.KeyValuePair("DtlsSrtpKeyAgreement", "true"));

    return createCallConnectionInternal(callConfiguration, observer,
                                        configuration, constraints, null /* sslCertificateVerifier */);

    }

  /**
   *
   * Simple pass through to underlying PeerConnectionFactory object
   *
   * @param label     Media stream label
   *
   * @return Newly created MediaStream object
   */
  public MediaStream createLocalMediaStream(String label) {
    return this.peerConnectionFactory.createLocalMediaStream(label);
  }

  /**
   *
   * Simple pass through to underlying PeerConnectionFactory object
   *
   * @param constraints    Media stream constraints
   *
   * @return Newly created AudioSource object
   */
  public AudioSource createAudioSource(MediaConstraints constraints) {
    return this.peerConnectionFactory.createAudioSource(constraints);
  }

  /**
   *
   * Simple pass through to underlying PeerConnectionFactory object
   *
   * @param id      Audio track id
   * @param source  Audio source
   *
   * @return Newly created AudioTrack object
   */
  public AudioTrack createAudioTrack(String id, AudioSource source) {
    return this.peerConnectionFactory.createAudioTrack(id, source);
  }

  /**
   *
   * Simple pass through to underlying PeerConnectionFactory object
   *
   * @param isScreencast  true if video source is a screencast
   *
   * @return Newly created VideoSource object
   */
  public VideoSource createVideoSource(boolean isScreencast) {
    return this.peerConnectionFactory.createVideoSource(isScreencast);
  }

  /**
   *
   * Simple pass through to underlying PeerConnectionFactory object
   *
   * @param id      Video track id
   * @param source  Video source
   *
   * @return Newly created VideoTrack object
   */
  public VideoTrack createVideoTrack(String id, VideoSource source) {
    return this.peerConnectionFactory.createVideoTrack(id, source);
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

  class PeerConnectionFactoryOptions extends PeerConnectionFactory.Options {
    public PeerConnectionFactoryOptions() {
      this.networkIgnoreMask = 1 << 4;
    }
  }

  /* Native methods below here */

  private static native
    BuildInfo ringrtcGetBuildInfo()
    throws CallException;

  private static native
    void ringrtcInitialize() throws CallException;

  private static native
    long ringrtcCreateCallConnectionFactory(long nativePeerConnectionFactory);

  private static native
    void ringrtcFreeFactory(long factory);

  private static native
    long ringrtcCreateCallConnection(long nativeFactory, CallConnection.Configuration callConfiguration,
                                     long nativeObserver, PeerConnection.RTCConfiguration rtcConfig,
                                     MediaConstraints constraints,
                                     SSLCertificateVerifier sslCertificateVerifier)
    throws IOException, CallException;

}
