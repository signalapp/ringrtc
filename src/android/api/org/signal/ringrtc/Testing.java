/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

package org.signal.ringrtc;

import java.io.IOException;

import org.whispersystems.libsignal.IdentityKey;
import org.whispersystems.libsignal.ecc.Curve;
import org.whispersystems.libsignal.ecc.ECKeyPair;
import org.whispersystems.signalservice.api.crypto.UntrustedIdentityException;
import org.whispersystems.signalservice.api.push.exceptions.UnregisteredUserException;

/**
* Testing is a class for generating various exceptions for the purpose of testing.
*
* A typical use case is in the implementation of the SignalMessageRecipient interface.
*
*/
public class Testing {

  /* For testing */
  public static IOException fakeIOException() {
    return new IOException("fake network problem");
  }

  /* For testing */
  public static UnregisteredUserException fakeUnregisteredUserException() {
    return new UnregisteredUserException("+123456", new IOException("fake unregistered user problem"));
  }

  /* For testing */
  public static UntrustedIdentityException fakeUntrustedIdentityException() {
    ECKeyPair keyPair = Curve.generateKeyPair();
    IdentityKey identityKey = new IdentityKey(keyPair.getPublicKey());
    return new UntrustedIdentityException("fake identity problem", "+123456", identityKey);
  }

}
