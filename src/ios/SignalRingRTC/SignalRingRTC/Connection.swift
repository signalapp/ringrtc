//
//  Copyright (c) 2020 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC
import WebRTC
import SignalCoreKit

public class Connection {
    private var audioSender: RTCRtpSender?
    private var videoSender: RTCRtpSender?

    private var peerConnection: RTCPeerConnection
    private var nativePeerConnection: UnsafeMutableRawPointer

    init(pcObserver: UnsafeMutableRawPointer, factory: RTCPeerConnectionFactory, configuration: RTCConfiguration, constraints: RTCMediaConstraints) {
        self.peerConnection = factory.peerConnection(with: configuration, constraints: constraints, observer: pcObserver)
        self.nativePeerConnection = self.peerConnection.getRawPeerConnection()

        Logger.debug("object! Connection created... \(ObjectIdentifier(self))")
    }

    deinit {
        Logger.debug("object! Connection destroyed... \(ObjectIdentifier(self))")
    }

    func close() {
        // Give the native pointer back...
        Logger.debug("Releasing PeerConnection")
        self.peerConnection.releaseRawPeerConnection(self.nativePeerConnection)

        Logger.debug("Closing PeerConnection")
        self.peerConnection.close()

        Logger.debug("Done")
    }

    func getRawPeerConnection() -> UnsafeMutableRawPointer? {
        return self.nativePeerConnection
    }

    func getWrapper(pc: UnsafeMutableRawPointer?) -> AppConnectionInterface {
        return AppConnectionInterface(
            object: UnsafeMutableRawPointer(Unmanaged.passRetained(self).toOpaque()),
            pc: pc,
            destroy: connectionDestroy)
    }

    func createStream(nativeStream: UnsafeMutableRawPointer) -> RTCMediaStream {
        return self.peerConnection.createStream(fromNative: nativeStream)
    }

    func createAudioSender(audioTrack: RTCAudioTrack) {
        // We use addTrack instead of createSender or addTransceiver because it uses
        // the track's ID instead of a random ID in the SDP, which is important
        // for call forking.
        self.audioSender = self.peerConnection.add(audioTrack, streamIds: ["ARDAMS"])
    }

    func createVideoSender(videoTrack: RTCVideoTrack) {
        // We use addTrack instead of createSender or addTransceiver because it uses
        // the track's ID instead of a random ID in the SDP, which is important
        // for call forking.
        self.videoSender = self.peerConnection.add(videoTrack, streamIds: ["ARDAMS"])
    }
}

func connectionDestroy(object: UnsafeMutableRawPointer?) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }

    let connection = Unmanaged<Connection>.fromOpaque(object).takeRetainedValue()
    // @note There should not be any retainers left for the object
    // so deinit should be called implicitly.

    connection.close()
}
