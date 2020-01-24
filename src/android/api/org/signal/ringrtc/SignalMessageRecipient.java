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

import java.io.IOException;
import java.util.List;

import org.whispersystems.signalservice.api.crypto.UntrustedIdentityException;
import org.whispersystems.signalservice.api.push.exceptions.UnregisteredUserException;
import org.whispersystems.signalservice.api.messages.calls.IceUpdateMessage;

/**
 *
 * An interface representing the remote peer of a call
 *
 * <p> This class encapsulates the sending of Signal service messages
 * to a recipient (remote peer) using existing Signal protocol data
 * structures.
 *
 * <p> The native library needs to be able to send Signal messages via
 * the service, but it does not have a native implementation to do so.
 * Instead the native code calls out to the client for sending Signal
 * messages.  To accomplish this, the client implements this interface
 * and passes an instance of that in a CallConnection.Configuration
 * object.
 *
 * @see CallConnection
 * @see CallConnection.Configuration
 */
public interface SignalMessageRecipient {

  boolean isEqual(@NonNull SignalMessageRecipient o);

  void sendOfferMessage(Context context, long callId, String description)
    throws UnregisteredUserException, UntrustedIdentityException, IOException;

  void sendAnswerMessage(Context context, long callId, String description)
    throws UnregisteredUserException, UntrustedIdentityException, IOException;

  void sendIceUpdates(Context context, List<IceUpdateMessage> iceUpdateMessages)
    throws UnregisteredUserException, UntrustedIdentityException, IOException;

  void sendHangupMessage(Context context, long callId)
    throws UnregisteredUserException, UntrustedIdentityException, IOException;

}
