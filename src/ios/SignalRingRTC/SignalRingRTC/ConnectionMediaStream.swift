//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import SignalRingRTC.RingRTC
import WebRTC

// See comment of IosMediaStream to understand
// where this fits in the many layers of wrappers.
@available(iOSApplicationExtension, unavailable)
public class ConnectionMediaStream {

    // Associate this application MediaStream object with a Connection.
    let connection: Connection

    // Hold on to the stream object when it is created.
    var mediaStream: RTCMediaStream?

    init(connection: Connection) {
        self.connection = connection

        Logger.debug("object! ConnectionMediaStream created... \(ObjectIdentifier(self))")
    }

    deinit {
        Logger.debug("object! ConnectionMediaStream destroyed... \(ObjectIdentifier(self))")
    }

    func getWrapper() -> AppMediaStreamInterface {
        return AppMediaStreamInterface(
            object: UnsafeMutableRawPointer(Unmanaged.passRetained(self).toOpaque()),
            destroy: connectionMediaStreamDestroy,
            createMediaStream: connectionMediaStreamCreateMediaStream)
    }
}

@available(iOSApplicationExtension, unavailable)
func connectionMediaStreamDestroy(object: UnsafeMutableRawPointer?) {
    guard let object = object else {
        failDebug("object was unexpectedly nil")
        return
    }

    Logger.debug("")

    _ = Unmanaged<ConnectionMediaStream>.fromOpaque(object).takeRetainedValue()
    // @note There should not be any retainers left for the object
    // so deinit should be called implicitly.
}

@available(iOSApplicationExtension, unavailable)
func connectionMediaStreamCreateMediaStream(object: UnsafeMutableRawPointer?, nativeStreamBorrowedRc: UnsafeMutableRawPointer?) -> UnsafeMutableRawPointer? {
    guard let object = object else {
        failDebug("object was unexpectedly nil")
        return nil
    }

    let obj: ConnectionMediaStream = Unmanaged.fromOpaque(object).takeUnretainedValue()

    Logger.debug("")

    guard let nativeStreamBorrowedRc = nativeStreamBorrowedRc else {
        failDebug("nativeStreamBorrowedRc was unexpectedly nil")
        return nil
    }

    let mediaStream = obj.connection.createStream(nativeStreamBorrowedRc: nativeStreamBorrowedRc)
    obj.mediaStream = mediaStream

    return UnsafeMutableRawPointer(Unmanaged.passUnretained(mediaStream).toOpaque())
}
