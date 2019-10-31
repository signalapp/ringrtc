//
//  Copyright (c) 2019 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC
import WebRTC
import SignalCoreKit

public enum CallConnectionError: Error {

    /**
     * Failure during createCallConnection.
     */
    case ringRtcCreateFailure(description: String)

    /**
     * Bad state, session should be cleaned up.
     */
    case ringRtcNotInitialized

    /**
     * API failure, session should be terminated.
     */
    case ringRtcFailure(description: String)

    /**
     * Media invalid, session should be terminated.
     */
    case invalidAudioTrack

    /**
     * Media invalid, session should be terminated.
     */
    case invalidVideoTrack

    /**
     * Media invalid, session should be terminated.
     */
    case invalidVideoCaptureController
}

public protocol CallConnectionDelegate: class {
    // Observer notifications

    /**
     * Fired for various asynchronous RingRTC events. See CallConnection.CallEvent for more information.
     */
    func callConnection(_ callConnection: CallConnection, onCallEvent event: CallEvent, callId: UInt64)

    /**
     * Fired whenever RingRTC encounters an error. Should always be considered fatal and end the session.
     */
    func callConnection(_ callConnection: CallConnection, onCallError error: String, callId: UInt64)

    /**
     * Fired whenever the remote video track becomes active or inactive.
     */
    func callConnection(_ callConnection: CallConnection, onAddRemoteVideoTrack track: RTCVideoTrack, callId: UInt64)

    // Internal notifications

    /**
     * Fired whenever the local video track becomes active or inactive.
     */
    func callConnection(_ callConnection: CallConnection, onUpdateLocalVideoSession session: AVCaptureSession?, callId: UInt64)

    // Signaling notifications

    /**
     * Fired when an offer message should be sent over the signaling channel.
     */
    func callConnection(_ callConnection: CallConnection, shouldSendOffer sdp: String, callId: UInt64)

    /**
     * Fired when an answer message should be sent over the signaling channel.
     */
    func callConnection(_ callConnection: CallConnection, shouldSendAnswer sdp: String, callId: UInt64)

    /**
     * Fired when there are one or more local Ice Candidates to be sent over the signaling channel.
     */
    func callConnection(_ callConnection: CallConnection, shouldSendIceCandidates candidates: [RTCIceCandidate], callId: UInt64)

    /**
     * Fired when a hangup message should be sent over the signaling channel.
     */
    func callConnection(_ callConnection: CallConnection, shouldSendHangup callId: UInt64)
}

@objc public class CallConnection: RTCPeerConnection, CallConnectionObserverDelegate, CallConnectionRecipientDelegate, VideoCaptureSettingsDelegate {
    // MARK: General Variables

    // A camera queue on which to perform camera operations.
    private static let cameraQueue = DispatchQueue(label: "CallConnectionCameraQueue")

    private weak var callConnectionDelegate: CallConnectionDelegate?

    // The factory that was used to create this instance of the CallConnection.
    let factory: CallConnectionFactory

    internal var ringRtcCallConnection: UnsafeMutableRawPointer?

    // MARK: WebRTC Variables

    private var audioSender: RTCRtpSender?
    private var audioTrack: RTCAudioTrack?

    private var videoCaptureController: VideoCaptureController?
    private var videoSender: RTCRtpSender?
    private var videoTrack: RTCVideoTrack?

    // MARK: Call Control/State Variables

    let callId: UInt64
    let isOutgoing: Bool

    // MARK: Object Lifetime

    // We have enabled the init() method in RTCPeerConnection so that we
    // can access it for object instantiation.
    init(delegate: CallConnectionDelegate, factory: CallConnectionFactory, callId: UInt64, isOutgoing: Bool) {
        self.callConnectionDelegate = delegate
        self.factory = factory

        self.callId = callId
        self.isOutgoing = isOutgoing

        super.init()

        Logger.debug("object! CallConnection created... \(ObjectIdentifier(self))")
    }

    deinit {
        Logger.debug("object! CallConnection destroyed... \(ObjectIdentifier(self))")
    }

    // MARK: Utility Functions

    // @note Meant to be called from factory during creation.
    func createAudioSender() {
        let audioConstraints = RTCMediaConstraints(mandatoryConstraints: nil, optionalConstraints: nil)
        let audioSource = self.factory.audioSource(with: audioConstraints)

        let audioTrack = self.factory.audioTrack(with: audioSource, trackId: "ARDAMSa0")
        self.audioTrack = audioTrack

        // Disable until the call is connected.
        audioTrack.isEnabled = false

        // @note There may be an iOS9 issue with the definition 'Kind' definition below.
        let audioSender = self.sender(withKind: kRTCMediaStreamTrackKindAudio, streamId: "ARDAMS")
        audioSender.track = audioTrack
        self.audioSender = audioSender
    }

    // @note Meant to be called from factory during creation.
    func createVideoSender() {
        let videoSource = self.factory.videoSource()

        let videoTrack = self.factory.videoTrack(with: videoSource, trackId: "ARDAMSv0")
        self.videoTrack = videoTrack

        // Disable until call is connected.
        videoTrack.isEnabled = false

        let capturer = RTCCameraVideoCapturer(delegate: videoSource)
        self.videoCaptureController = VideoCaptureController(capturer: capturer, settingsDelegate: self)

        // @note There may be an iOS9 issue with the 'Kind' definition below.
        let videoSender = self.sender(withKind: kRTCMediaStreamTrackKindVideo, streamId: "ARDAMS")
        videoSender.track = videoTrack
        self.videoSender = videoSender
    }

    // MARK: API Functions

    // (*) Stop and close any call session and release resources.
    // @note This function blocks and should be called off the
    // main thread.
    public override func close() {
        Logger.debug("")

        // Clear the delegate immediately so that we can guarantee that
        // no delegate methods are called after terminate() returns.
        callConnectionDelegate = nil

        var retPtr = ringRtcClose(self.ringRtcCallConnection)
        if retPtr == nil {
            owsFailDebug("ringRtcClose() failed")
        }

        super.close()

        self.audioSender = nil
        self.audioTrack = nil
        self.videoSender = nil
        self.videoTrack = nil
        self.videoCaptureController = nil

        retPtr = ringRtcDispose(self.ringRtcCallConnection)
        if retPtr == nil {
            owsFailDebug("ringRtcDispose() failed")
        }

        self.ringRtcCallConnection = nil
        
        Logger.debug("done")
    }

    // (caller) Start a call connection by sending an offer.
    public func sendOffer() throws {
        AssertIsOnMainThread()

        Logger.debug("sendOffer() \(ObjectIdentifier(self))")

        guard ringRtcCallConnection != nil else {
            throw CallConnectionError.ringRtcNotInitialized
        }

        let retPtr = ringRtcSendOffer(ringRtcCallConnection, Unmanaged.passUnretained(self).toOpaque())
        guard retPtr != nil else {
            throw CallConnectionError.ringRtcFailure(description: "ringRtcSendOffer failed")
        }
    }

    // (callee) Start a call connection if we have received an offer.
    public func receivedOffer(sdp: String) throws {
        AssertIsOnMainThread()

        Logger.debug("receivedOffer() \(ObjectIdentifier(self))")

        guard ringRtcCallConnection != nil else {
            throw CallConnectionError.ringRtcNotInitialized
        }

        // We pass strings to RingRTC as data buffers with an
        // associated byte length.
        let bytes = Array(sdp.utf8)
        let retPtr = ringRtcReceivedOffer(ringRtcCallConnection, Unmanaged.passUnretained(self).toOpaque(), bytes, bytes.count)
        guard retPtr != nil else {
            throw CallConnectionError.ringRtcFailure(description: "ringRtcReceivedOffer failed")
        }
     }

    // (caller) An answer has been received.
    public func receivedAnswer(sdp: String) throws {
        AssertIsOnMainThread()

        Logger.debug("receivedAnswer() \(ObjectIdentifier(self))")

        guard ringRtcCallConnection != nil else {
            throw CallConnectionError.ringRtcNotInitialized
        }

        // We pass strings to RingRTC as data buffers with an
        // associated byte length.
        let bytes = Array(sdp.utf8)
        let retPtr = ringRtcReceivedAnswer(ringRtcCallConnection, bytes, bytes.count)
        guard retPtr != nil else {
            throw CallConnectionError.ringRtcFailure(description: "ringRtcReceivedAnswer failed")
        }
    }

    // (*) An Ice Candidate has been received.
    public func receivedIceCandidate(sdp: String, lineIndex: Int32, sdpMid: String) throws {
        AssertIsOnMainThread()

        Logger.debug("receivedIceCandidate() \(ObjectIdentifier(self))")

        guard ringRtcCallConnection != nil else {
            throw CallConnectionError.ringRtcNotInitialized
        }

        // We pass strings to RingRTC as data buffers with an
        // associated byte length.
        let sdpBytes = Array(sdp.utf8)
        let sdpMidBytes = Array(sdpMid.utf8)
        let retPtr = ringRtcReceivedIceCandidate(ringRtcCallConnection,
                                                 sdpBytes,
                                                 sdpBytes.count,
                                                 lineIndex,
                                                 sdpMidBytes,
                                                 sdpMidBytes.count)
        guard retPtr != nil else {
            throw CallConnectionError.ringRtcFailure(description: "ringRtcReceivedIceCandidate failed")
        }
    }

    // (callee) Accept an incoming call.
    // @note Media can flow as soon as possible.
    public func accept() throws {
        AssertIsOnMainThread()

        Logger.debug("accept() \(ObjectIdentifier(self))")

        guard ringRtcCallConnection != nil else {
            throw CallConnectionError.ringRtcNotInitialized
        }

        let retPtr = ringRtcAccept(ringRtcCallConnection)
        guard retPtr != nil else {
            throw CallConnectionError.ringRtcFailure(description: "ringRtcAccept failed")
        }
    }

    // (*) Hangup/End a call session.
    // @note This also will end a pending call.
    public func hangup() throws {
        AssertIsOnMainThread()

        Logger.debug("hangup() \(ObjectIdentifier(self))")

        guard ringRtcCallConnection != nil else {
            throw CallConnectionError.ringRtcNotInitialized
        }

        let retPtr = ringRtcHangup(ringRtcCallConnection)
        guard retPtr != nil else {
            throw CallConnectionError.ringRtcFailure(description: "ringRtcHangup failed")
        }
    }

    // (*) Enable audio to flow through stream.
    public func setLocalAudioEnabled(enabled: Bool) throws {
        AssertIsOnMainThread()

        Logger.debug("setAudioEnabled(\(enabled)) \(ObjectIdentifier(self))")

        guard let audioTrack = self.audioTrack else {
            throw CallConnectionError.invalidAudioTrack
        }

        // Let the audio flow through the stream, or not.
        audioTrack.isEnabled = enabled
    }

    // (*) Enable video to flow through stream.
    public func setLocalVideoEnabled(enabled: Bool) throws {
        AssertIsOnMainThread()

        Logger.debug("setLocalVideoEnabled(\(enabled)) \(ObjectIdentifier(self))")

        guard let videoTrack = self.videoTrack else {
            throw CallConnectionError.invalidVideoTrack
        }

        guard let videoCaptureController = self.videoCaptureController else {
            throw CallConnectionError.invalidVideoCaptureController
        }

        // Let the video flow through the stream, or not.
        videoTrack.isEnabled = enabled

        if enabled {
            Logger.debug("starting video capture \(ObjectIdentifier(self))")
            videoCaptureController.startCapture()
        } else {
            Logger.debug("stopping video capture \(ObjectIdentifier(self))")
            videoCaptureController.stopCapture()
        }

        // We are already on the main thread but dispatch the notification
        // of the session anyway for handling.
        DispatchQueue.main.async {
            Logger.debug("setLocalVideoEnabled - main thread")

            let strongSelf = self
            guard let strongDelegate = strongSelf.callConnectionDelegate else { return }

            if enabled {
                strongDelegate.callConnection(strongSelf, onUpdateLocalVideoSession: videoCaptureController.captureSession, callId: strongSelf.callId)
            } else {
                // Pass nil if we are disabled...
                strongDelegate.callConnection(strongSelf, onUpdateLocalVideoSession: nil, callId: strongSelf.callId)
            }
        }
    }

    public func sendLocalVideoStatus(enabled: Bool) throws {
        AssertIsOnMainThread()

        Logger.debug("sendVideoStatus(\(enabled)) \(ObjectIdentifier(self))")

        guard ringRtcCallConnection != nil else {
            throw CallConnectionError.ringRtcNotInitialized
        }

        let retPtr = ringRtcSendVideoStatus(ringRtcCallConnection, enabled)
        guard retPtr != nil else {
            throw CallConnectionError.ringRtcFailure(description: "ringRtcSendVideoStatus failed")
        }
    }

    public func setCameraSource(isUsingFrontCamera: Bool) throws {
        AssertIsOnMainThread()

        Logger.debug("setCameraSource(\(isUsingFrontCamera)) \(ObjectIdentifier(self))")

        guard self.videoCaptureController != nil else {
            throw CallConnectionError.invalidVideoCaptureController
        }

        CallConnection.cameraQueue.async {
            let strongSelf = self

            guard let captureController = strongSelf.videoCaptureController else {
                owsFailDebug("missing videoCaptureController")
                return
            }

            captureController.switchCamera(isUsingFrontCamera: isUsingFrontCamera)
        }
    }

    // (callee) Decline call due to being busy (already in another call).
    // The gist here is that we will direct the busy indication to the
    // correct mechanism to send the busy message...
    // @note Not currently supported on iOS.
    func sendBusy(callId: UInt64) throws {
        AssertIsOnMainThread()

        Logger.debug("sendBusy() \(ObjectIdentifier(self))")

        guard ringRtcCallConnection != nil else {
            throw CallConnectionError.ringRtcNotInitialized
        }

        let retPtr = ringRtcSendBusy(self.ringRtcCallConnection, callId)
        guard retPtr != nil else {
            throw CallConnectionError.ringRtcFailure(description: "ringRtcSendBusy failed")
        }
    }

    // MARK: Recipient (Signaling) Handlers

    internal func onSendOffer(_ callConnectionRecipient: CallConnectionRecipient, callId: UInt64, offer: String) {
        Logger.debug("onSendOffer")

        DispatchQueue.main.async {
            Logger.debug("onSendOffer - main thread")

            let strongSelf = self
            guard let strongDelegate = strongSelf.callConnectionDelegate else { return }

            strongDelegate.callConnection(strongSelf, shouldSendOffer: offer, callId: callId)
        }
    }

    internal func onSendAnswer(_ callConnectionRecipient: CallConnectionRecipient, callId: UInt64, answer: String) {
        Logger.debug("onSendAnswer")

        DispatchQueue.main.async {
            Logger.debug("onSendAnswer - main thread")

            let strongSelf = self
            guard let strongDelegate = strongSelf.callConnectionDelegate else { return }

            strongDelegate.callConnection(strongSelf, shouldSendAnswer: answer, callId: callId)
        }
    }

    internal func onSendIceCandidates(_ callConnectionRecipient: CallConnectionRecipient, callId: UInt64, candidates: [RTCIceCandidate]) {
        Logger.debug("onSendIceCandidates")

        DispatchQueue.main.async {
            Logger.debug("onSendIceCandidates - main thread")

            let strongSelf = self
            guard let strongDelegate = strongSelf.callConnectionDelegate else { return }

            strongDelegate.callConnection(strongSelf, shouldSendIceCandidates: candidates, callId: callId)
        }
    }

    internal func onSendHangup(_ callConnectionRecipient: CallConnectionRecipient, callId: UInt64) {
        Logger.debug("onSendHangup")

        DispatchQueue.main.async {
            Logger.debug("onSendHangup - main thread")

            let strongSelf = self
            guard let strongDelegate = strongSelf.callConnectionDelegate else { return }

            strongDelegate.callConnection(strongSelf, shouldSendHangup: callId)
        }
    }

    // MARK: Observer Handlers

    internal func onCallEvent(_ callConnectionObserver: CallConnectionObserver, callId: UInt64, callEvent: CallEvent) {
        Logger.debug("onCallEvent")

        DispatchQueue.main.async {
            Logger.debug("onCallEvent - main thread")

            let strongSelf = self
            guard let strongDelegate = strongSelf.callConnectionDelegate else { return }

            strongDelegate.callConnection(strongSelf, onCallEvent: callEvent, callId: callId)
        }
    }

    internal func onCallError(_ callConnectionObserver: CallConnectionObserver, callId: UInt64, errorString: String) {
        Logger.debug("onCallError")

        DispatchQueue.main.async {
            Logger.debug("onCallError - main thread")

            let strongSelf = self
            guard let strongDelegate = strongSelf.callConnectionDelegate else { return }

            strongDelegate.callConnection(strongSelf, onCallError: errorString, callId: callId)
        }
    }

    internal func onAddStream(_ callConnectionObserver: CallConnectionObserver, callId: UInt64, stream: RTCMediaStream) {
        Logger.debug("onAddStream")

        DispatchQueue.main.async {
            Logger.debug("onAddStream - main thread")

            let strongSelf = self
            guard let strongDelegate = strongSelf.callConnectionDelegate else { return }

            guard stream.videoTracks.count > 0 else {
                owsFailDebug("missing video stream")
                return
            }

            let remoteVideoTrack = stream.videoTracks[0]

            strongDelegate.callConnection(strongSelf, onAddRemoteVideoTrack: remoteVideoTrack, callId: callId)
        }
    }

    // MARK: VideoCaptureSettingsDelegate

    var videoWidth: Int32 {
        return 400
    }

    var videoHeight: Int32 {
        return 400
    }
}

// @note Bringing the following over from PeerConnectionClient.

protocol VideoCaptureSettingsDelegate: class {
    var videoWidth: Int32 { get }
    var videoHeight: Int32 { get }
}

class VideoCaptureController {
    private let capturer: RTCCameraVideoCapturer
    private weak var settingsDelegate: VideoCaptureSettingsDelegate?
    private let serialQueue = DispatchQueue(label: "org.signal.videoCaptureController")
    private var isUsingFrontCamera: Bool = true

    public var captureSession: AVCaptureSession {
        return capturer.captureSession
    }

    public init(capturer: RTCCameraVideoCapturer, settingsDelegate: VideoCaptureSettingsDelegate) {
        self.capturer = capturer
        self.settingsDelegate = settingsDelegate
    }

    public func startCapture() {
        serialQueue.sync { [weak self] in
            guard let strongSelf = self else {
                return
            }

            strongSelf.startCaptureSync()
        }
    }

    public func stopCapture() {
        serialQueue.sync { [weak self] in
            guard let strongSelf = self else {
                return
            }

            strongSelf.capturer.stopCapture()
        }
    }

    public func switchCamera(isUsingFrontCamera: Bool) {
        serialQueue.sync { [weak self] in
            guard let strongSelf = self else {
                return
            }

            strongSelf.isUsingFrontCamera = isUsingFrontCamera
            strongSelf.startCaptureSync()
        }
    }

    private func assertIsOnSerialQueue() {
        if _isDebugAssertConfiguration(), #available(iOS 10.0, *) {
            assertOnQueue(serialQueue)
        }
    }

    private func startCaptureSync() {
        assertIsOnSerialQueue()

        let position: AVCaptureDevice.Position = isUsingFrontCamera ? .front : .back
        guard let device: AVCaptureDevice = self.device(position: position) else {
            owsFailDebug("unable to find captureDevice")
            return
        }

        guard let format: AVCaptureDevice.Format = self.format(device: device) else {
            owsFailDebug("unable to find captureDevice")
            return
        }

        let fps = self.framesPerSecond(format: format)
        capturer.startCapture(with: device, format: format, fps: fps)
    }

    private func device(position: AVCaptureDevice.Position) -> AVCaptureDevice? {
        let captureDevices = RTCCameraVideoCapturer.captureDevices()
        guard let device = (captureDevices.first { $0.position == position }) else {
            Logger.debug("unable to find desired position: \(position)")
            return captureDevices.first
        }

        return device
    }

    private func format(device: AVCaptureDevice) -> AVCaptureDevice.Format? {
        let formats = RTCCameraVideoCapturer.supportedFormats(for: device)
        let targetWidth = settingsDelegate?.videoWidth ?? 0
        let targetHeight = settingsDelegate?.videoHeight ?? 0

        var selectedFormat: AVCaptureDevice.Format?
        var currentDiff: Int32 = Int32.max

        for format in formats {
            let dimension = CMVideoFormatDescriptionGetDimensions(format.formatDescription)
            let diff = abs(targetWidth - dimension.width) + abs(targetHeight - dimension.height)
            if diff < currentDiff {
                selectedFormat = format
                currentDiff = diff
            }
        }

        if _isDebugAssertConfiguration(), let selectedFormat = selectedFormat {
            let dimension = CMVideoFormatDescriptionGetDimensions(selectedFormat.formatDescription)
            Logger.debug("selected format width: \(dimension.width) height: \(dimension.height)")
        }

        assert(selectedFormat != nil)

        return selectedFormat
    }

    private func framesPerSecond(format: AVCaptureDevice.Format) -> Int {
        var maxFrameRate: Float64 = 0
        for range in format.videoSupportedFrameRateRanges {
            maxFrameRate = max(maxFrameRate, range.maxFrameRate)
        }

        return Int(maxFrameRate)
    }
}
