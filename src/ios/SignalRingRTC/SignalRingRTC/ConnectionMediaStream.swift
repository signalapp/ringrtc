//
//  Copyright (c) 2020 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC
import WebRTC
import SignalCoreKit

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

func connectionMediaStreamDestroy(object: UnsafeMutableRawPointer?) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }

    Logger.debug("")

    _ = Unmanaged<ConnectionMediaStream>.fromOpaque(object).takeRetainedValue()
    // @note There should not be any retainers left for the object
    // so deinit should be called implicitly.
}

func connectionMediaStreamCreateMediaStream(object: UnsafeMutableRawPointer?, nativeStream: UnsafeMutableRawPointer?) -> UnsafeMutableRawPointer? {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return nil
    }

    let obj: ConnectionMediaStream = Unmanaged.fromOpaque(object).takeUnretainedValue()

    Logger.debug("")

    guard let nativeStream = nativeStream else {
        owsFailDebug("nativeStream was unexpectedly nil")
        return nil
    }

    let mediaStream = obj.connection.createStream(nativeStream: nativeStream)
    obj.mediaStream = mediaStream

    return UnsafeMutableRawPointer(Unmanaged.passUnretained(mediaStream).toOpaque())
}
