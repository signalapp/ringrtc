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
        let audioSender = self.peerConnection.sender(withKind: kRTCMediaStreamTrackKindAudio, streamId: "ARDAMS")
        audioSender.track = audioTrack
        self.audioSender = audioSender
    }

    func createVideoSender(videoTrack: RTCVideoTrack) {
        let videoSender = self.peerConnection.sender(withKind: kRTCMediaStreamTrackKindVideo, streamId: "ARDAMS")
        videoSender.track = videoTrack
        self.videoSender = videoSender
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
