//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import SignalRingRTC.RingRTC
import WebRTC

@available(iOSApplicationExtension, unavailable)
public class Connection {
    private var audioSender: RTCRtpSender?
    private var videoSender: RTCRtpSender?

    private var peerConnection: RTCPeerConnection
    private var nativePeerConnection: UnsafeMutableRawPointer

    init(pcObserverOwned: UnsafeMutableRawPointer, factory: RTCPeerConnectionFactory, configuration: RTCConfiguration, constraints: RTCMediaConstraints) {
        // Takes an owned pointer to the observer.
        // See "std::unique_ptr<webrtc::PeerConnectionObserver> _customObserver"
        // in webrtc/src/sdk/objc/api/peerconnection/RTCPeerConnection.mm
        // which is modified in RingRTC's fork of WebRTC.
        self.peerConnection = factory.peerConnection(with: configuration, constraints: constraints, observer: pcObserverOwned)
        self.nativePeerConnection = self.peerConnection.getNativePeerConnectionPointer()

        Logger.debug("object! Connection created... \(ObjectIdentifier(self))")
    }

    deinit {
        Logger.debug("Closing PeerConnection")
        self.peerConnection.close()

        Logger.debug("object! Connection destroyed... \(ObjectIdentifier(self))")
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

    func createStream(nativeStreamBorrowedRc: UnsafeMutableRawPointer) -> RTCMediaStream {
        // This gets converted into a rtc::scoped_refptr<webrtc::MediaStreamInterface>.
        // In other words, the ref count gets incremented,
        // so what's passed in is a borrowed RC.
        return self.peerConnection.createStream(fromNative: nativeStreamBorrowedRc)
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

@available(iOSApplicationExtension, unavailable)
func connectionDestroy(object: UnsafeMutableRawPointer?) {
    guard let object = object else {
        failDebug("object was unexpectedly nil")
        return
    }

    let _ = Unmanaged<Connection>.fromOpaque(object).takeRetainedValue()
    // @note There should not be any retainers left for the object
    // so deinit should be called implicitly.
}
