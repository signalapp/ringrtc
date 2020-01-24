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

import java.io.IOException;
import java.util.List;

import org.webrtc.IceCandidate;
import org.webrtc.MediaStream;
import org.webrtc.NativePeerConnectionFactory;
import org.webrtc.PeerConnection;

import org.whispersystems.signalservice.api.SignalServiceAccountManager;
import org.whispersystems.signalservice.api.crypto.UntrustedIdentityException;
import org.whispersystems.signalservice.api.messages.calls.IceUpdateMessage;
import org.whispersystems.signalservice.api.push.exceptions.UnregisteredUserException;

/**
 *
 * Represents the call connection to a remote peer
 *
 * <p> This class inherits from org.webrtc.PeerConnection and
 * encapsulates the lifecycle of establishing, creating, and tearing
 * down a call.
 *
 * @see CallConnectionFactory
 */
public class CallConnection extends PeerConnection {

  /**
   *
   * Contains parameters for creating a CallConnection object
   */
  public static class Configuration {

    private final String TAG = CallConnection.Configuration.class.getSimpleName();
    /** SignalServiceAccountManager object used to fetch TURN server info */
    public final SignalServiceAccountManager accountManager;
    /** Set to true if user wants to hide their IP at the cost of latency */
    public final boolean   hideIp;
    /** Unique integer identifying the call */
    public final long      callId;
    /** Remote peer information */
    public final SignalMessageRecipient recipient;
    /** Set to true if the call is out bound, otherwise false for an incoming call */
    public final boolean   outBound;

    public Configuration(Long      callId,
                         boolean   outBound,
                         SignalMessageRecipient      recipient,
                         SignalServiceAccountManager accountManager,
                         boolean hideIp) {
      Log.i(TAG, "Created Configuration()");
      this.callId = callId;
      this.outBound = outBound;
      this.recipient = recipient;
      this.accountManager = accountManager;
      this.hideIp = hideIp;
    }

  }

  private static final String TAG = CallConnection.class.getSimpleName();
  private long nativeCallConnection;

  @NonNull  private final Context        context;
  @NonNull  private final Long           callId;
  @NonNull  private final SignalMessageRecipient     recipient;

  CallConnection(NativeFactory factory) {
    super(factory);
    this.nativeCallConnection = factory.nativeCallConnection;
    this.callId               = factory.callId;
    this.recipient            = factory.recipient;
    this.context              = factory.callConnectionFactory.context;
    Log.i(TAG, "create: callId: 0x" + Long.toHexString(callId));
  }

  static long createNativeCallConnectionObserver(Observer               observer,
                                                 Long                   callId,
                                                 SignalMessageRecipient recipient) {
    return nativeCreateCallConnectionObserver(observer, callId, recipient);
  }

  private void checkCallConnectionExists() {
    if (nativeCallConnection == 0) {
      throw new IllegalStateException("CallConnection has been disposed.");
    }
  }

  /**
   *
   * Close down and dispose of the CallConnection object
   *
   */
  @Override public void dispose() {
    checkCallConnectionExists();

    Log.i(TAG, "CallConnection.dispose(): closing native call connection");
    nativeClose(nativeCallConnection);

    Log.i(TAG, "CallConnection.dispose(): calling super.close()");
    super.close();
    Log.i(TAG, "CallConnection.dispose(): calling super.dispose()");
    super.dispose();

    Log.i(TAG, "CallConnection.dispose(): disposing native call connection");
    nativeDispose(nativeCallConnection);
    nativeCallConnection = 0;

  }

  /**
   *
   * Send a call offer message to the remote peer
   *
   * @throws CallException   internal native library failure
   *
   */
  public void sendOffer()
    throws CallException
  {
    Log.i(TAG, "CallConnection::sendOffer():");
    checkCallConnectionExists();
    nativeSendOffer(nativeCallConnection);
  }

  /**
   *
   * Validate the recipient and callId matches
   *
   * <p>This method validates that the recipient and callId matches
   * the recipient and callId of this CallConnection object.
   *
   * @param recipient remote peer attempting to acknowledge a call
   * @param inCallId  incoming callId
   *
   * @return true if the response matches, false otherwise.
   *
   * @throws CallException   internal native library failure
   *
   */
  public boolean validateResponse(SignalMessageRecipient recipient, @Nullable Long inCallId)
    throws CallException
  {
    Log.i(TAG, "CallConnection::validateResponse():");
    checkCallConnectionExists();
    return inCallId != null &&
      inCallId.longValue() == this.callId &&
      this.recipient.isEqual(recipient) &&
      nativeValidateResponseState(nativeCallConnection);
  }

  /**
   *
   * Processes the received offer answer message
   *
   * <p>The offer answer is sent by the remote peer to us in response
   * to our initial offer message.
   *
   * @param sessionDescription describes the remote peer's session
   *
   * @throws CallException   internal native library failure
   *
   */
  public void handleOfferAnswer(String sessionDescription)
    throws CallException
  {
    checkCallConnectionExists();
    nativeHandleOfferAnswer(nativeCallConnection, sessionDescription);
  }

  /**
   *
   * Processes the received offer message
   *
   * <p>The offer is sent by the remote peer to us to initiate a call.
   * This is the beginning of an incoming call.
   *
   * @param offer describes the remote peer's offer
   *
   * @throws CallException   internal native library failure
   *
   */
  public void acceptOffer(String offer)
    throws CallException
  {
    checkCallConnectionExists();
    nativeAcceptOffer(nativeCallConnection, offer);
  }

  /**
   *
   * Informs the remote peer that the call is being terminated
   *
   * @throws CallException   internal native library failure
   *
   */
  public void hangUp()
    throws CallException
  {
    checkCallConnectionExists();
    nativeHangUp(nativeCallConnection);
  }

  /**
   *
   * Informs the remote peer that the call is completely connected
   *
   * @throws CallException   internal native library failure
   *
   */
  public void answerCall()
    throws CallException
  {
    checkCallConnectionExists();
    nativeAnswerCall(nativeCallConnection);
  }

  /**
   *
   * Adds an IceCandidate from the remote peer to the CallConnection
   *
   * @param candidate  incoming IceCandidate from remote peer
   */
  @Override
  public boolean addIceCandidate(IceCandidate candidate)
  {
    checkCallConnectionExists();

    boolean result;
    try {
      result = nativeAddIceCandidate(nativeCallConnection, candidate.sdpMid,
                                     candidate.sdpMLineIndex, candidate.sdp);
    } catch (CallException e) {
      Log.w(TAG, "addIceCandidate() failed:", e);
      result = false;
    }
    return result;
  }

  /**
   *
   * Inform the remote peer of the state of our local video stream
   *
   * @param enabled  set true if sending video
   *
   * @throws CallException   internal native library failure
   *
   */
  public void sendVideoStatus(boolean enabled)
    throws CallException
  {
    checkCallConnectionExists();
    nativeSendVideoStatus(nativeCallConnection, enabled);
  }

  /**
   *
   * Send a SDP offer to a remote recipient
   *
   * This method is called by native code.
   *
   * @param recipient   represents the recipient (remote peer) of a call
   * @param callId      unique 64-bit number indentifying the call
   * @param description contains the SDP offer text
   *
   * @return Exception  any exceptions that happen while sending
   *
   * Any exception that happens is captured and returned to native
   * code as a result.  A null exception is an indication of success.
   */
  @Nullable
  @CalledByNative
  Exception sendSignalServiceOffer(SignalMessageRecipient recipient, long callId, String description)
  {

    Exception exception = null;

    Log.i(TAG, "CallConnection::sendSignalServiceOffer(): callId: 0x" + Long.toHexString(callId));
    try {
      recipient.sendOfferMessage(this.context, callId, description);
    } catch (UnregisteredUserException e) {
      exception = e;
      Log.w(TAG, e);
    } catch (UntrustedIdentityException e) {
      exception = e;
      Log.w(TAG, e);
    } catch (IOException e) {
      exception = e;
      Log.w(TAG, e);
    }

    return exception;

  }

  /**
   *
   * Send a SDP answer to a remote recipient
   *
   * This method is called by native code.
   *
   * @param recipient   represents the recipient (remote peer) of a call
   * @param callId      unique 64-bit number indentifying the call
   * @param description contains the SDP offer text
   *
   * @return Exception  any exceptions that happen while sending
   *
   * Any exception that happens is captured and returned to native
   * code as a result.  A null exception is an indication of success.
   */
  @Nullable
  @CalledByNative
  Exception sendSignalServiceAnswer(SignalMessageRecipient recipient, long callId, String description)
  {

    Exception exception = null;

    Log.i(TAG, "CallConnection::sendSignalServiceAnswer(): callId: 0x" + Long.toHexString(callId));
    try {
      recipient.sendAnswerMessage(this.context, callId, description);
    } catch (UnregisteredUserException e) {
      exception = e;
      Log.w(TAG, e);
    } catch (UntrustedIdentityException e) {
      exception = e;
      Log.w(TAG, e);
    } catch (IOException e) {
      exception = e;
      Log.w(TAG, e);
    }

    return exception;

  }

  /**
   *
   * Send list of ICE candidates to a remote recipient
   *
   * This method is called by native code.
   *
   * @param recipient          represents the recipient (remote peer) of a call
   * @param iceUpdateMessages  list of ICE candidates
   *
   * @return Exception  any exceptions that happen while sending
   *
   * Any exception that happens is captured and returned to native
   * code as a result.  A null exception is an indication of success.
   */
  @Nullable
  @CalledByNative
  Exception sendSignalServiceIceUpdates(SignalMessageRecipient recipient, List<IceUpdateMessage> iceUpdateMessages)
  {

    Exception exception = null;

    Log.i(TAG, "CallConnection::sendSignalServiceIceUpdates(): iceUpdates: " + iceUpdateMessages.size());
    try {
      recipient.sendIceUpdates(this.context, iceUpdateMessages);
    } catch (UnregisteredUserException e) {
      exception = e;
      Log.w(TAG, e);
    } catch (UntrustedIdentityException e) {
      exception = e;
      Log.w(TAG, e);
    } catch (IOException e) {
      exception = e;
      Log.w(TAG, e);
    }

    return exception;
  }

  /**
   *
   * Send hang-up message to a remote recipient
   *
   * This method is called by native code.
   *
   * @param recipient   represents the recipient (remote peer) of a call
   * @param callId      unique 64-bit number indentifying the call
   *
   * @return Exception  any exceptions that happen while sending
   *
   * Any exception that happens is captured and returned to native
   * code as a result.  A null exception is an indication of success.
   */
  @CalledByNative
  void sendSignalServiceHangup(SignalMessageRecipient recipient, long callId)
  {
    Log.i(TAG, "CallConnection::sendSignalServiceHangup(): callId: 0x" + Long.toHexString(callId));
    try {
      recipient.sendHangupMessage(this.context, callId);
    } catch (Exception e) {
      // Nothing we can do about it while hanging up...
      Log.w(TAG, e);
    }
  }

  /**
   *
   * Enumeration of simple call status events
   *
   */
  public enum CallEvent {

    /** The call is being established */
    RINGING,
    /** The remote peer indicates connection success */
    REMOTE_CONNECTED,
    /** The remote peer indicates its video stream is enabled */
    REMOTE_VIDEO_ENABLE,
    /** The remote peer indicates its video stream is disabled */
    REMOTE_VIDEO_DISABLE,
    /** The remote peer indicates it is terminating the call */
    REMOTE_HANGUP,
    /** The call failed to connect during the call setup phase */
    CONNECTION_FAILED,
    /** Unable to establish the call within a resonable amount of time */
    CALL_TIMEOUT,
    /** The call dropped while connected and is now reconnecting */
    CALL_RECONNECTING;

    @CalledByNative
    static CallEvent fromNativeIndex(int nativeIndex) {
      return values()[nativeIndex];
    }

  }

  /**
   *
   * Enumeration of call error events
   *
   */
  public enum CallError {

    /** Attempt to create a call to an unregistered Signal user */
    UNREGISTERED_USER,
    /** Attempt to create a call to a user with an untrusted Signal identity */
    UNTRUSTED_IDENTITY,
    /** Network problems while establishing the call */
    NETWORK_FAILURE,
    /** Library internal failure */
    INTERNAL_FAILURE,
    /** Other failure */
    FAILURE;

    @CalledByNative
    public static CallError fromNativeIndex(int nativeIndex) {
      return values()[nativeIndex];
    }

  }

  /**
   *
   * Interface for handling CallConnection events and errors
   *
   */
  public interface Observer {

    /**
     *
     * Simple status event callback
     *
     * @param recipient  remote peer
     * @param callId     callId for the call
     * @param event      status event
     *
     * @see CallEvent
     */
    @CalledByNative
    void onCallEvent(SignalMessageRecipient recipient,
                     long                   callId,
                     CallEvent              event);

    /**
     *
     * Error event callback
     *
     * @param recipient  remote peer
     * @param callId     callId for the call
     * @param error      exception describing the error
     *
     */
    @CalledByNative
    void onCallError(SignalMessageRecipient recipient,
                     long                   callId,
                     Exception              error);

    /**
     *
     * On adding a remote media stream callback
     *
     * @param recipient  remote peer
     * @param callId     callId for the call
     * @param stream     remote media stream
     *
     */
    @CalledByNative
    void onAddStream(SignalMessageRecipient recipient,
                     long                   callId,
                     MediaStream            stream);

  }

  /**
   *
   * Represents the native call connection factory
   *
   * This class is an implementation detail, used to encapsulate the
   * native call connection pointer created by the call connection
   * factory.
   *
   * One way of constructing a PeerConnection (CallConnection's super
   * class) is by passing an object that implements
   * NativePeerConnectionFactory, which is done here.
   *
   * @see CallConnectionFactory
   */
  static class NativeFactory implements NativePeerConnectionFactory {
    private long nativeCallConnection;
    private CallConnectionFactory callConnectionFactory;
    private Long callId;
    private SignalMessageRecipient recipient;

    protected NativeFactory(long nativeCallConnection,
                            @NonNull CallConnectionFactory callConnectionFactory,
                            @NonNull Long callId,
                            @NonNull SignalMessageRecipient recipient) {
      this.nativeCallConnection = nativeCallConnection;
      this.callConnectionFactory = callConnectionFactory;
      this.callId = callId;
      this.recipient = recipient;
    }

    @Override
    public long createNativePeerConnection() {
      Log.d(TAG, "Creating CallConnection() with createNativeCallConnection(): ");
      return nativeGetNativePeerConnection(nativeCallConnection);
    }

  }

  /* Native methods below here */

  private static native
    long nativeCreateCallConnectionObserver(Observer observer, long callId, SignalMessageRecipient recipient);

  private static native
    long nativeGetNativePeerConnection(long nativeCallConnection);

  private native
    void nativeClose(long nativeCallConnection);

  private native
    void nativeDispose(long nativeCallConnection);

  private native void nativeSendOffer(long nativeCallConnection)
    throws CallException;

  private native
    boolean nativeValidateResponseState(long nativeCallConnection)
    throws CallException;

  private native
    void nativeHandleOfferAnswer(long nativeCallConnection, String sessionDescription)
    throws CallException;

  private native
    void nativeAcceptOffer(long nativeCallConnection, String offer)
    throws CallException;

  private native
    void nativeHangUp(long nativeCallConnection)
    throws CallException;

  private native
    void nativeAnswerCall(long nativeCallConnection)
    throws CallException;

  private native
    void nativeSendVideoStatus(long nativeCallConnection, boolean enabled)
    throws CallException;

  private native
    boolean nativeAddIceCandidate(long nativeCallConnection, String sdpMid, int sdpMLineIndex, String iceCandidateSdp)
    throws CallException;

}
