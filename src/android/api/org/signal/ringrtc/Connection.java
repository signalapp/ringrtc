/*
 *
 *  Copyright (C) 2019 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

package org.signal.ringrtc;

import androidx.annotation.NonNull;
import androidx.annotation.Nullable;

import org.webrtc.AudioSource;
import org.webrtc.AudioTrack;
import org.webrtc.NativePeerConnectionFactory;
import org.webrtc.PeerConnection;

/**
 *
 * Represents the connection to a remote peer
 *
 * <p> This class inherits from org.webrtc.PeerConnection and
 * encapsulates the lifecycle of establishing, creating, and tearing
 * down a connection.
 *
 */
public class Connection extends PeerConnection {

  @NonNull
  private static final String        TAG = Connection.class.getSimpleName();
  @NonNull
  private        final CallId        callId;
  private              long          nativePeerConnection;
  private              int           remoteDevice;
  @Nullable
  private              AudioSource   audioSource;
  @Nullable
  private              AudioTrack    audioTrack;

  Connection(NativeFactory factory) {
    super(factory);
    this.nativePeerConnection = factory.nativePeerConnection;
    this.callId               = factory.callId;
    this.remoteDevice         = factory.remoteDevice;
    Log.i(TAG, "ctor(): connectionId: " + callId.format(remoteDevice));
  }

  @Override
  public String toString() {
    return callId.format(remoteDevice);
  }

  private void checkConnectionExists() {
    if (nativePeerConnection == 0) {
      throw new IllegalStateException("Connection has been closed.");
    }
  }

  void setAudioSource(@NonNull AudioSource   audioSource,
                      @NonNull AudioTrack    audioTrack) {
    checkConnectionExists();

    this.audioSource   = audioSource;
    this.audioTrack    = audioTrack;

  }

  void setAudioEnabled(boolean enabled) {
    // enable microphone
    audioTrack.setEnabled(enabled);
  }

  /**
   *
   * Close the Connection object and clean up.
   *
   */
  void shutdown() {
    checkConnectionExists();

    audioSource.dispose();

    try {
      Log.i(TAG, "Connection.shutdown(): calling super.close()");
      close();
      Log.i(TAG, "Connection.shutdown(): calling super.dispose()");
      dispose();
      Log.i(TAG, "Connection.shutdown(): after calling super.dispose()");
    } catch (Exception e) {
      Log.e(TAG, "Problem closing PeerConnection: ", e);
    }

    nativePeerConnection = 0;

  }

  /**
   *
   * Represents the native call connection factory
   *
   * This class is an implementation detail, used to encapsulate the
   * native call connection pointer created by the call connection
   * factory.
   *
   * One way of constructing a PeerConnection (Connection's super
   * class) is by passing an object that implements
   * NativePeerConnectionFactory, which is done here.
   *
   */
  static class NativeFactory implements NativePeerConnectionFactory {
    private long   nativePeerConnection;
    private CallId callId;
    private int    remoteDevice;

    protected NativeFactory(         long   nativePeerConnection,
                            @NonNull CallId callId,
                                     int    remoteDevice) {
      this.nativePeerConnection = nativePeerConnection;
      this.callId               = callId;
      this.remoteDevice         = remoteDevice;
    }

    @Override
    public long createNativePeerConnection() {
      return nativePeerConnection;
    }

  }

}
