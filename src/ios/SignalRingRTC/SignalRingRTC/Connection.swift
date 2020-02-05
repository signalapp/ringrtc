//
//  Copyright (c) 2020 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC
import WebRTC
import SignalCoreKit

public class Connection: RTCPeerConnection {
    private var audioSender: RTCRtpSender?
    private var videoSender: RTCRtpSender?

    override init() {
        super.init()

        Logger.debug("object! Connection created... \(ObjectIdentifier(self))")
    }

    deinit {
        Logger.debug("object! Connection destroyed... \(ObjectIdentifier(self))")
    }

    public override func close() {
        Logger.debug("")

        super.close()

        self.audioSender = nil
        self.videoSender = nil

        Logger.debug("done")
    }

    func getWrapper(pc: UnsafeMutableRawPointer?) -> AppConnectionInterface {
        return AppConnectionInterface(
            object: UnsafeMutableRawPointer(Unmanaged.passRetained(self).toOpaque()),
            pc: pc,
            destroy: connectionDestroy)
    }

    func createAudioSender(audioTrack: RTCAudioTrack) {
        let audioSender = self.sender(withKind: kRTCMediaStreamTrackKindAudio, streamId: "ARDAMS")
        audioSender.track = audioTrack
        self.audioSender = audioSender
    }

    func createVideoSender(videoTrack: RTCVideoTrack) {
        let videoSender = self.sender(withKind: kRTCMediaStreamTrackKindVideo, streamId: "ARDAMS")
        videoSender.track = videoTrack
        self.videoSender = videoSender
    }
}

func connectionDestroy(object: UnsafeMutableRawPointer?) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }

    Logger.debug("")

    let connection = Unmanaged<Connection>.fromOpaque(object).takeRetainedValue()
    // @note There should not be any retainers left for the object
    // so deinit should be called implicitly.

    connection.close()
}
