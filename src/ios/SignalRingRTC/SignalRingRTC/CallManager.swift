//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import SignalRingRTC.RingRTC
import WebRTC
import SignalCoreKit
import PromiseKit

// Errors that the Call Manager APIs can throw.
public enum CallManagerError: Error {
    case apiFailed(description: String)
}

/// Primary events a Call UI can act upon.
public enum CallManagerEvent: Int32 {
    /// Inbound call only: The call signaling (ICE) is complete.
    case ringingLocal = 0
    /// Outbound call only: The call signaling (ICE) is complete.
    case ringingRemote
    /// The local side has accepted and connected the call.
    case connectedLocal
    /// The remote side has accepted and connected the call.
    case connectedRemote
    /// The call ended because of a local hangup.
    case endedLocalHangup
    /// The call ended because of a remote hangup.
    case endedRemoteHangup
    /// The call ended because the remote needs permission.
    case endedRemoteHangupNeedPermission
    /// The call ended because the call was accepted by a different device.
    case endedRemoteHangupAccepted
    /// The call ended because the call was declined by a different device.
    case endedRemoteHangupDeclined
    /// The call ended because the call was declared busy by a different device.
    case endedRemoteHangupBusy
    /// The call ended because of a remote busy message.
    case endedRemoteBusy
    /// The call ended because of glare (received offer from same remote).
    case endedRemoteGlare
    /// The call ended because it timed out during setup.
    case endedTimeout
    /// The call ended because of an internal error condition.
    case endedInternalFailure
    /// The call ended because a signaling message couldn't be sent.
    case endedSignalingFailure
    /// The call ended because setting up the connection failed.
    case endedConnectionFailure
    /// The call ended because the application wanted to drop the call.
    case endedDropped
    /// The remote side has enabled video.
    case remoteVideoEnable
    /// The remote side has disabled video.
    case remoteVideoDisable
    /// The remote side has enabled screen sharing.
    case remoteSharingScreenEnable
    /// The remote side has disabled screen sharing.
    case remoteSharingScreenDisable
    /// The call dropped while connected and is now reconnecting.
    case reconnecting
    /// The call dropped while connected and is now reconnected.
    case reconnected
    /// The received offer is expired.
    case receivedOfferExpired
    /// Received an offer while already handling an active call.
    case receivedOfferWhileActive
    /// Received an offer while already handling an active call and glare was detected.
    case receivedOfferWithGlare
    /// Received an offer on a linked device from one that doesn't support multi-ring.
    case ignoreCallsFromNonMultiringCallers
}

/// Type of media for call at time of origination.
public enum CallMediaType: Int32 {
    /// Call should start as audio only.
    case audioCall = 0
    /// Call should start as audio/video.
    case videoCall = 1
}

/// Modes of operation when working with different bandwidth environments.
public enum BandwidthMode: Int32 {
    /// Intended for audio-only, to help ensure reliable audio over
    /// severely constrained networks.
    case veryLow = 0
    /// Intended for low bitrate video calls. Useful to reduce
    /// bandwidth costs, especially on mobile networks.
    case low = 1
    /// (Default) No specific constraints, but keep a relatively
    /// high bitrate to ensure good quality.
    case normal = 2
}

/// Type of hangup message.
public enum HangupType: Int32 {
    /// Normal hangup, typically remote user initiated.
    case normal = 0
    /// Call was accepted elsewhere by a different device.
    case accepted = 1
    /// Call was declined elsewhere by a different device.
    case declined = 2
    /// Call was declared busy elsewhere by a different device.
    case busy = 3
    /// Call needed permission on a different device.
    case needPermission = 4
}

/// Contains the list of currently joined participants and related info about the call in progress.
public struct PeekInfo {
    public let joinedMembers: [UUID]
    public let creator: UUID?
    public let eraId: String?
    public let maxDevices: UInt32?
    public let deviceCount: UInt32

    public init(joinedMembers: [UUID], creator: UUID?, eraId: String?, maxDevices: UInt32?, deviceCount: UInt32) {
        self.joinedMembers = joinedMembers
        self.creator = creator
        self.eraId = eraId
        self.maxDevices = maxDevices
        self.deviceCount = deviceCount
    }
}

/// The HTTP methods supported by the Call Manager.
public enum CallManagerHttpMethod: Int32 {
    case get = 0
    case put = 1
    case post = 2
    case delete = 3
}

/// Class to wrap the group call dictionary so group call objects can reference
/// it. All operations must be done on the main thread.
class GroupCallByClientId {
    private var groupCallByClientId: [UInt32: GroupCall] = [:]

    subscript(clientId: UInt32) -> GroupCall? {
        get { groupCallByClientId[clientId] }
        set { groupCallByClientId[clientId] = newValue }
    }
}

class Requests<T> {
    private var sealById: [UInt32: Resolver<T>] = [:]
    private var nextId: UInt32 = 1

    func add() -> (UInt32, Promise<T>) {
        let id = self.nextId
        self.nextId += 1
        let promise: Promise<T> = Promise { seal in
            self.sealById[id] = seal
        }
        return (id, promise)
    }

    func resolve(id: UInt32, response: T) -> Bool {
        guard let seal = self.sealById[id] else {
            return false
        }
        seal.fulfill(response)
        self.sealById[id] = nil
        return true
    }
}

public protocol CallManagerDelegate: AnyObject {

    associatedtype CallManagerDelegateCallType: CallManagerCallReference

    /**
     * A call, either outgoing or incoming, should be started by the application.
     * Invoked on the main thread, asychronously.
     */
    func callManager(_ callManager: CallManager<CallManagerDelegateCallType, Self>, shouldStartCall call: CallManagerDelegateCallType, callId: UInt64, isOutgoing: Bool, callMediaType: CallMediaType)

    /**
     * onEvent will be invoked in response to Call Manager library operations.
     * Invoked on the main thread, asychronously.
     */
    func callManager(_ callManager: CallManager<CallManagerDelegateCallType, Self>, onEvent call: CallManagerDelegateCallType, event: CallManagerEvent)

    /**
     * An Offer message should be sent to the given remote.
     * Invoked on the main thread, asychronously.
     * If there is any error, the UI can reset UI state and invoke the reset() API.
     */
    func callManager(_ callManager: CallManager<CallManagerDelegateCallType, Self>, shouldSendOffer callId: UInt64, call: CallManagerDelegateCallType, destinationDeviceId: UInt32?, opaque: Data, callMediaType: CallMediaType)

    /**
     * An Answer message should be sent to the given remote.
     * Invoked on the main thread, asychronously.
     * If there is any error, the UI can reset UI state and invoke the reset() API.
     */
    func callManager(_ callManager: CallManager<CallManagerDelegateCallType, Self>, shouldSendAnswer callId: UInt64, call: CallManagerDelegateCallType, destinationDeviceId: UInt32?, opaque: Data)

    /**
     * An Ice Candidate message should be sent to the given remote.
     * Invoked on the main thread, asychronously.
     * If there is any error, the UI can reset UI state and invoke the reset() API.
     */
    func callManager(_ callManager: CallManager<CallManagerDelegateCallType, Self>, shouldSendIceCandidates callId: UInt64, call: CallManagerDelegateCallType, destinationDeviceId: UInt32?, candidates: [Data])

    /**
     * A Hangup message should be sent to the given remote.
     * Invoked on the main thread, asychronously.
     * If there is any error, the UI can reset UI state and invoke the reset() API.
     */
    func callManager(_ callManager: CallManager<CallManagerDelegateCallType, Self>, shouldSendHangup callId: UInt64, call: CallManagerDelegateCallType, destinationDeviceId: UInt32?, hangupType: HangupType, deviceId: UInt32, useLegacyHangupMessage: Bool)

    /**
     * A Busy message should be sent to the given remote.
     * Invoked on the main thread, asychronously.
     * If there is any error, the UI can reset UI state and invoke the reset() API.
     */
    func callManager(_ callManager: CallManager<CallManagerDelegateCallType, Self>, shouldSendBusy callId: UInt64, call: CallManagerDelegateCallType, destinationDeviceId: UInt32?)

    /**
     * A call message should be sent to the given remote recipient.
     * Invoked on the main thread, asychronously.
     * If there is any error, the UI can reset UI state and invoke the reset() API.
     */
    func callManager(_ callManager: CallManager<CallManagerDelegateCallType, Self>, shouldSendCallMessage recipientUuid: UUID, message: Data)

    /**
     * A HTTP request should be sent to the given url.
     * Invoked on the main thread, asychronously.
     * The result of the call should be indicated by calling the receivedHttpResponse() function.
     */
    func callManager(_ callManager: CallManager<CallManagerDelegateCallType, Self>, shouldSendHttpRequest requestId: UInt32, url: String, method: CallManagerHttpMethod, headers: [String: String], body: Data?)

    /**
     * Two call 'remote' pointers should be compared to see if they refer to the same
     * remote peer/contact.
     * Invoked on the main thread, *synchronously*.
     */
    func callManager(_ callManager: CallManager<CallManagerDelegateCallType, Self>, shouldCompareCalls call1: CallManagerDelegateCallType, call2: CallManagerDelegateCallType) -> Bool

    /**
     * The local video track has been enabled and can be connected to the
     * UI's display surface/view for the outgoing media.
     * Invoked on the main thread, asychronously.
     */
    func callManager(_ callManager: CallManager<CallManagerDelegateCallType, Self>, onUpdateLocalVideoSession call: CallManagerDelegateCallType, session: AVCaptureSession?)

    /**
     * The remote peer has connected and their video track can be connected to the
     * UI's display surface/view for the incoming media.
     * Invoked on the main thread, asychronously.
     */
    func callManager(_ callManager: CallManager<CallManagerDelegateCallType, Self>, onAddRemoteVideoTrack call: CallManagerDelegateCallType, track: RTCVideoTrack)
}

public protocol CallManagerCallReference: AnyObject { }

// Implementation of the Call Manager for iOS.
public class CallManager<CallType, CallManagerDelegateType>: CallManagerInterfaceDelegate where CallManagerDelegateType: CallManagerDelegate, CallManagerDelegateType.CallManagerDelegateCallType == CallType {

    public weak var delegate: CallManagerDelegateType?

    private var factory: RTCPeerConnectionFactory?

    // This dictionary is shared with each groupCall object, but the
    // permanent reference to it is here.
    private let groupCallByClientId: GroupCallByClientId

    private let peekInfoRequests: Requests<PeekInfo> = Requests()

    private var ringRtcCallManager: UnsafeMutableRawPointer!

    private var videoCaptureController: VideoCaptureController?

    public init() {
        // Initialize the global object (mainly for logging).
        _ = CallManagerGlobal.shared

        // Initialize the WebRTC factory.
        let decoderFactory = RTCDefaultVideoDecoderFactory()
        let encoderFactory = RTCDefaultVideoEncoderFactory()
        self.factory = RTCPeerConnectionFactory(encoderFactory: encoderFactory, decoderFactory: decoderFactory)

        self.groupCallByClientId = GroupCallByClientId()

        // Create an anonymous Call Manager interface. Ownership will
        // be transferred to RingRTC.
        let interface = CallManagerInterface(delegate: self)

        // Create the RingRTC Call Manager itself.
        guard let ringRtcCallManager = ringrtcCreate(Unmanaged.passUnretained(self).toOpaque(), interface.getWrapper()) else {
            owsFail("unable to create ringRtcCallManager")
        }

        self.ringRtcCallManager = ringRtcCallManager

        Logger.debug("object! CallManager created... \(ObjectIdentifier(self))")
    }

    deinit {
        // Close the RingRTC Call Manager.
        let retPtr = ringrtcClose(self.ringRtcCallManager)
        if retPtr == nil {
            Logger.warn("Call Manager couldn't be properly closed")
        }

        Logger.debug("object! CallManager destroyed... \(ObjectIdentifier(self))")
    }

    // MARK: - Control API

    /// Place a call to a remote peer.
    ///
    /// - Parameters:
    ///   - call: The application call context
    ///   - callMediaType: The type of call to place (audio or video)
    ///   - localDevice: The local device ID of the client (must be valid for lifetime of the call)
    public func placeCall(call: CallType, callMediaType: CallMediaType, localDevice: UInt32) throws {
        AssertIsOnMainThread()
        Logger.debug("call")

        let unmanagedCall: Unmanaged<CallType> = Unmanaged.passUnretained(call)

        let retPtr = ringrtcCall(ringRtcCallManager, unmanagedCall.toOpaque(), callMediaType.rawValue, localDevice)
        if retPtr == nil {
            throw CallManagerError.apiFailed(description: "call() function failure")
        }

        // Keep the call reference around until rust says we're done with the call.
        _ = unmanagedCall.retain()
    }

    public func accept(callId: UInt64) throws {
        AssertIsOnMainThread()
        Logger.debug("accept")

        let retPtr = ringrtcAccept(ringRtcCallManager, callId)
        if retPtr == nil {
            throw CallManagerError.apiFailed(description: "accept() function failure")
        }
    }

    public func hangup() throws {
        AssertIsOnMainThread()
        Logger.debug("hangup")

        let retPtr = ringrtcHangup(ringRtcCallManager)
        if retPtr == nil {
            throw CallManagerError.apiFailed(description: "hangup() function failure")
        }
    }

    // MARK: - Flow API

    /// Proceed with a call after the shouldStartCall delegate was invoked.
    ///
    /// - Parameters:
    ///   - callId: The callId as provided by the shouldStartCall delegate
    ///   - iceServers: A list of RTC Ice Servers to be provided to WebRTC
    ///   - hideIp: A flag used to hide the IP of the user by using relay (TURN) servers only
    ///   - videoCaptureController: UI provided capturer interface
    ///   - bandwidthMode: The desired bandwidth mode to start the session with
    public func proceed(callId: UInt64, iceServers: [RTCIceServer], hideIp: Bool, videoCaptureController: VideoCaptureController, bandwidthMode: BandwidthMode) throws {
        AssertIsOnMainThread()
        Logger.debug("proceed")

        // Create a shared media sources.
        let audioConstraints = RTCMediaConstraints(mandatoryConstraints: nil, optionalConstraints: nil)
        let audioSource = self.factory!.audioSource(with: audioConstraints)
        // Note: This must stay "audio1" to stay in sync with V4 signaling.
        let audioTrack = self.factory!.audioTrack(with: audioSource, trackId: "audio1")
        audioTrack.isEnabled = false

        let videoSource = self.factory!.videoSource()
        // Note: This must stay "video1" to stay in sync with V4 signaling.
        let videoTrack = self.factory!.videoTrack(with: videoSource, trackId: "video1")
        videoTrack.isEnabled = false

        // Define maximum output video format for 1:1 calls.
        videoSource.adaptOutputFormat(
            toWidth: 1280,
            height: 720,
            fps: 30
        )

        videoCaptureController.capturerDelegate = videoSource

        // This defaults to ECDSA, which should be fast.
        let certificate = RTCCertificate.generate(withParams: [:])!

        // Create a call context object to hold on to some of
        // the settings needed by the application when actually
        // creating the connection.
        let appCallContext = CallContext(iceServers: iceServers, hideIp: hideIp, audioSource: audioSource, audioTrack: audioTrack, videoSource: videoSource, videoTrack: videoTrack, videoCaptureController: videoCaptureController, certificate: certificate)

        let retPtr = ringrtcProceed(ringRtcCallManager, callId, appCallContext.getWrapper(), bandwidthMode.rawValue)
        if retPtr == nil {
            throw CallManagerError.apiFailed(description: "proceed() function failure")
        }
    }

    public func drop(callId: UInt64) {
        AssertIsOnMainThread()
        Logger.debug("drop")

        let retPtr = ringrtcDrop(ringRtcCallManager, callId)
        if retPtr == nil {
            owsFailDebug("ringrtcDrop() function failure")
        }
    }

    public func signalingMessageDidSend(callId: UInt64) throws {
        AssertIsOnMainThread()
        Logger.debug("signalingMessageDidSend")

        let retPtr = ringrtcMessageSent(ringRtcCallManager, callId)
        if retPtr == nil {
            throw CallManagerError.apiFailed(description: "ringrtcMessageSent() function failure")
        }
    }

    public func signalingMessageDidFail(callId: UInt64) {
        AssertIsOnMainThread()
        Logger.debug("signalingMessageDidFail")

        let retPtr = ringrtcMessageSendFailure(ringRtcCallManager, callId)
        if retPtr == nil {
            owsFailDebug("ringrtcMessageSendFailure() function failure")
        }
    }

    public func reset() {
        AssertIsOnMainThread()
        Logger.debug("reset")

        let retPtr = ringrtcReset(ringRtcCallManager)
        if retPtr == nil {
            owsFailDebug("ringrtcReset() function failure")
        }
    }

    public func setLocalAudioEnabled(enabled: Bool) {
        AssertIsOnMainThread()
        Logger.debug("setLocalAudioEnabled(\(enabled))")

        let retPtr = ringrtcGetActiveCallContext(ringRtcCallManager)
        guard let callContext = retPtr else {
            if enabled {
                owsFailDebug("Can't enable audio on non-existent context")
            }
            return
        }

        let appCallContext: CallContext = Unmanaged.fromOpaque(callContext).takeUnretainedValue()

        appCallContext.setAudioEnabled(enabled: enabled)
    }

    public func setLocalVideoEnabled(enabled: Bool, call: CallType) {
        AssertIsOnMainThread()
        Logger.debug("setLocalVideoEnabled(\(enabled))")

        let retPtr = ringrtcGetActiveCallContext(ringRtcCallManager)
        guard let callContext = retPtr else {
            if enabled {
                owsFailDebug("Can't enable video on non-existent context")
            }
            return
        }

        let appCallContext: CallContext = Unmanaged.fromOpaque(callContext).takeUnretainedValue()

        if appCallContext.setVideoEnabled(enabled: enabled) {
            // The setting changed, so actually update components to the new state.

            appCallContext.setCameraEnabled(enabled: enabled)

            if ringrtcSetVideoEnable(ringRtcCallManager, enabled) == nil {
                owsFailDebug("ringrtcSetVideoEnable() function failure")
                return
            }

            DispatchQueue.main.async {
                Logger.debug("setLocalVideoEnabled - main async")

                guard let delegate = self.delegate else { return }

                if enabled {
                    delegate.callManager(self, onUpdateLocalVideoSession: call, session: appCallContext.getCaptureSession())
                } else {
                    delegate.callManager(self, onUpdateLocalVideoSession: call, session: nil)
                }
            }
        }
    }

    public func udpateBandwidthMode(bandwidthMode: BandwidthMode) {
        AssertIsOnMainThread()
        Logger.debug("udpateBandwidthMode(\(bandwidthMode))")

        ringrtcUpdateBandwidthMode(ringRtcCallManager, bandwidthMode.rawValue)
    }

    // MARK: - Signaling API
    public func receivedOffer<CallType: CallManagerCallReference>(call: CallType, sourceDevice: UInt32, callId: UInt64, opaque: Data, messageAgeSec: UInt64, callMediaType: CallMediaType, localDevice: UInt32, remoteSupportsMultiRing: Bool, isLocalDevicePrimary: Bool, senderIdentityKey: Data, receiverIdentityKey: Data) throws {
        AssertIsOnMainThread()
        Logger.debug("receivedOffer")

        let opaqueSlice = allocatedAppByteSliceFromData(maybe_data: opaque)
        let senderIdentityKeySlice = allocatedAppByteSliceFromData(maybe_data: senderIdentityKey)
        let receiverIdentityKeySlice = allocatedAppByteSliceFromData(maybe_data: receiverIdentityKey)

        // Make sure to release the allocated memory when the function exists,
        // to ensure that the pointers are still valid when used in the RingRTC
        // API function.
        defer {
            if opaqueSlice.bytes != nil {
                 opaqueSlice.bytes.deallocate()
            }
            if senderIdentityKeySlice.bytes != nil {
                 senderIdentityKeySlice.bytes.deallocate()
            }
            if receiverIdentityKeySlice.bytes != nil {
                 receiverIdentityKeySlice.bytes.deallocate()
            }
        }

        let unmanagedRemote: Unmanaged<CallType> = Unmanaged.passUnretained(call)
        let retPtr = ringrtcReceivedOffer(ringRtcCallManager, callId, unmanagedRemote.toOpaque(), sourceDevice, opaqueSlice, messageAgeSec, callMediaType.rawValue, localDevice, remoteSupportsMultiRing, isLocalDevicePrimary, senderIdentityKeySlice, receiverIdentityKeySlice)
        if retPtr == nil {
            throw CallManagerError.apiFailed(description: "receivedOffer() function failure")
        }

        // Keep the call reference around until rust says we're done with the call.
        _ = unmanagedRemote.retain()
    }

    public func receivedAnswer(sourceDevice: UInt32, callId: UInt64, opaque: Data, remoteSupportsMultiRing: Bool, senderIdentityKey: Data, receiverIdentityKey: Data) throws {
        AssertIsOnMainThread()
        Logger.debug("receivedAnswer")

        let opaqueSlice = allocatedAppByteSliceFromData(maybe_data: opaque)
        let senderIdentityKeySlice = allocatedAppByteSliceFromData(maybe_data: senderIdentityKey)
        let receiverIdentityKeySlice = allocatedAppByteSliceFromData(maybe_data: receiverIdentityKey)

        // Make sure to release the allocated memory when the function exists,
        // to ensure that the pointers are still valid when used in the RingRTC
        // API function.
        defer {
            if opaqueSlice.bytes != nil {
                 opaqueSlice.bytes.deallocate()
            }
            if senderIdentityKeySlice.bytes != nil {
                 senderIdentityKeySlice.bytes.deallocate()
            }
            if receiverIdentityKeySlice.bytes != nil {
                 receiverIdentityKeySlice.bytes.deallocate()
            }
        }

        let retPtr = ringrtcReceivedAnswer(ringRtcCallManager, callId, sourceDevice, opaqueSlice, remoteSupportsMultiRing, senderIdentityKeySlice, receiverIdentityKeySlice)
        if retPtr == nil {
            throw CallManagerError.apiFailed(description: "receivedAnswer() function failure")
        }
    }

    public func receivedIceCandidates(sourceDevice: UInt32, callId: UInt64, candidates: [Data]) throws {
        AssertIsOnMainThread()
        Logger.debug("receivedIceCandidates")

        let appIceCandidates: [AppByteSlice] = candidates.map { candidate in
            return allocatedAppByteSliceFromData(maybe_data: candidate)
        }

        // Make sure to release the allocated memory when the function exists,
        // to ensure that the pointers are still valid when used in the RingRTC
        // API function.
        defer {
            for appIceCandidate in appIceCandidates {
                if appIceCandidate.bytes != nil {
                    appIceCandidate.bytes.deallocate()
                }
            }
        }

        var appIceCandidateArray = appIceCandidates.withUnsafeBufferPointer { appIceCandidatesBytes in
            return AppIceCandidateArray(
                candidates: appIceCandidatesBytes.baseAddress,
                count: candidates.count
            )
        }

        let retPtr = ringrtcReceivedIceCandidates(ringRtcCallManager, callId, sourceDevice, &appIceCandidateArray)
        if retPtr == nil {
            throw CallManagerError.apiFailed(description: "ringrtcReceivedIceCandidates() function failure")
        }
    }

    public func receivedHangup(sourceDevice: UInt32, callId: UInt64, hangupType: HangupType, deviceId: UInt32) throws {
        AssertIsOnMainThread()
        Logger.debug("receivedHangup")

        let retPtr = ringrtcReceivedHangup(ringRtcCallManager, callId, sourceDevice, hangupType.rawValue, deviceId)
        if retPtr == nil {
            throw CallManagerError.apiFailed(description: "receivedHangup() function failure")
        }
    }

    public func receivedBusy(sourceDevice: UInt32, callId: UInt64) throws {
        AssertIsOnMainThread()
        Logger.debug("receivedBusy")

        let retPtr = ringrtcReceivedBusy(ringRtcCallManager, callId, sourceDevice)
        if retPtr == nil {
            throw CallManagerError.apiFailed(description: "receivedBusy() function failure")
        }
    }

    public func receivedCallMessage(senderUuid: UUID, senderDeviceId: UInt32, localDeviceId: UInt32, message: Data, messageAgeSec: UInt64) {
        AssertIsOnMainThread()
        Logger.debug("receivedCallMessage")

        let senderUuidSlice = allocatedAppByteSliceFromData(maybe_data: senderUuid.data)
        let messageSlice = allocatedAppByteSliceFromData(maybe_data: message)

        // Make sure to release the allocated memory when the function exists,
        // to ensure that the pointers are still valid when used in the RingRTC
        // API function.
        defer {
            if senderUuidSlice.bytes != nil {
                 senderUuidSlice.bytes.deallocate()
            }
            if messageSlice.bytes != nil {
                messageSlice.bytes.deallocate()
            }
        }

        ringrtcReceivedCallMessage(ringRtcCallManager, senderUuidSlice, senderDeviceId, localDeviceId, messageSlice, messageAgeSec)
    }

    public func receivedHttpResponse(requestId: UInt32, statusCode: UInt16, body: Data?) {
        AssertIsOnMainThread()
        Logger.debug("receivedHttpResponse")

        let bodySlice = allocatedAppByteSliceFromData(maybe_data: body)

        // Make sure to release the allocated memory when the function exists,
        // to ensure that the pointers are still valid when used in the RingRTC
        // API function.
        defer {
            if bodySlice.bytes != nil {
                bodySlice.bytes.deallocate()
            }
        }

        ringrtcReceivedHttpResponse(ringRtcCallManager, requestId, statusCode, bodySlice)
    }

    public func httpRequestFailed(requestId: UInt32) {
        AssertIsOnMainThread()
        Logger.debug("httpRequestFailed")

        ringrtcHttpRequestFailed(ringRtcCallManager, requestId)
    }

    // MARK: - Group Call

    public func createGroupCall(groupId: Data, sfuUrl: String, videoCaptureController: VideoCaptureController) -> GroupCall? {
        AssertIsOnMainThread()
        Logger.debug("createGroupCall")

        guard let factory = self.factory else {
            owsFailDebug("No factory found for GroupCall")
            return nil
        }

        let groupCall = GroupCall(ringRtcCallManager: ringRtcCallManager, factory: factory, groupCallByClientId: self.groupCallByClientId, groupId: groupId, sfuUrl: sfuUrl, videoCaptureController: videoCaptureController)
        return groupCall
    }

    public func peekGroupCall(sfuUrl: String, membershipProof: Data, groupMembers: [GroupMemberInfo]) -> Promise<PeekInfo> {
        AssertIsOnMainThread()
        Logger.debug("peekGroupCall")

        let sfuUrlSlice = allocatedAppByteSliceFromString(maybe_string: sfuUrl)
        let membershipProofSlice = allocatedAppByteSliceFromData(maybe_data: membershipProof)

        let appMembers: [AppGroupMemberInfo] = groupMembers.map { member in
            let userIdSlice = allocatedAppByteSliceFromData(maybe_data: member.userId.data)
            let userIdCipherTextSlice = allocatedAppByteSliceFromData(maybe_data: member.userIdCipherText)

            return AppGroupMemberInfo(userId: userIdSlice, userIdCipherText: userIdCipherTextSlice)
        }

        // Make sure to release the allocated memory when the function exists,
        // to ensure that the pointers are still valid when used in the RingRTC
        // API function.
        defer {
            if sfuUrlSlice.bytes != nil {
                sfuUrlSlice.bytes.deallocate()
            }
            if membershipProofSlice.bytes != nil {
                membershipProofSlice.bytes.deallocate()
            }

            for appMember in appMembers {
                if appMember.userId.bytes != nil {
                    appMember.userId.bytes.deallocate()
                }
                if appMember.userIdCipherText.bytes != nil {
                    appMember.userIdCipherText.bytes.deallocate()
                }
            }
        }

        var appGroupMemberInfoArray = appMembers.withUnsafeBufferPointer { appMembersBytes in
            return AppGroupMemberInfoArray(
                members: appMembersBytes.baseAddress,
                count: groupMembers.count
            )
        }

        let (requestId, promise) = self.peekInfoRequests.add()
        ringrtcPeekGroupCall(self.ringRtcCallManager, requestId, sfuUrlSlice, membershipProofSlice, &appGroupMemberInfoArray)
        return promise
    }

    // MARK: - Event Observers

    func onStartCall(remote: UnsafeRawPointer, callId: UInt64, isOutgoing: Bool, callMediaType: CallMediaType) {
        Logger.debug("onStartCall")

        DispatchQueue.main.async {
            Logger.debug("onStartCall - main.async")

            guard let delegate = self.delegate else { return }

            let callReference: CallType = Unmanaged.fromOpaque(remote).takeUnretainedValue()
            delegate.callManager(self, shouldStartCall: callReference, callId: callId, isOutgoing: isOutgoing, callMediaType: callMediaType)
        }
    }

    func onEvent(remote: UnsafeRawPointer, event: CallManagerEvent) {
        Logger.debug("onEvent")

        DispatchQueue.main.async {
            Logger.debug("onEvent - main.async")

            guard let delegate = self.delegate else { return }

            let callReference: CallType = Unmanaged.fromOpaque(remote).takeUnretainedValue()
            delegate.callManager(self, onEvent: callReference, event: event)
        }
    }

    // MARK: - Signaling Observers

    func onSendOffer(callId: UInt64, remote: UnsafeRawPointer, destinationDeviceId: UInt32?, opaque: Data, callMediaType: CallMediaType) {
        Logger.debug("onSendOffer")

        DispatchQueue.main.async {
            Logger.debug("onSendOffer - main.async")

            guard let delegate = self.delegate else { return }

            let callReference: CallType = Unmanaged.fromOpaque(remote).takeUnretainedValue()
            delegate.callManager(self, shouldSendOffer: callId, call: callReference, destinationDeviceId: destinationDeviceId, opaque: opaque, callMediaType: callMediaType)
        }
    }

    func onSendAnswer(callId: UInt64, remote: UnsafeRawPointer, destinationDeviceId: UInt32?, opaque: Data) {
        Logger.debug("onSendAnswer")

        DispatchQueue.main.async {
            Logger.debug("onSendAnswer - main.async")

            guard let delegate = self.delegate else { return }

            let callReference: CallType = Unmanaged.fromOpaque(remote).takeUnretainedValue()
            delegate.callManager(self, shouldSendAnswer: callId, call: callReference, destinationDeviceId: destinationDeviceId, opaque: opaque)
        }
    }

    func onSendIceCandidates(callId: UInt64, remote: UnsafeRawPointer, destinationDeviceId: UInt32?, candidates: [Data]) {
        Logger.debug("onSendIceCandidates")

        DispatchQueue.main.async {
            Logger.debug("onSendIceCandidates - main.async")

            guard let delegate = self.delegate else { return }

            let callReference: CallType = Unmanaged.fromOpaque(remote).takeUnretainedValue()
            delegate.callManager(self, shouldSendIceCandidates: callId, call: callReference, destinationDeviceId: destinationDeviceId, candidates: candidates)
        }
    }

    func onSendHangup(callId: UInt64, remote: UnsafeRawPointer, destinationDeviceId: UInt32?, hangupType: HangupType, deviceId: UInt32, useLegacyHangupMessage: Bool) {
        Logger.debug("onSendHangup")

        DispatchQueue.main.async {
            Logger.debug("onSendHangup - main.async")

            guard let delegate = self.delegate else { return }

            let callReference: CallType = Unmanaged.fromOpaque(remote).takeUnretainedValue()
            delegate.callManager(self, shouldSendHangup: callId, call: callReference, destinationDeviceId: destinationDeviceId, hangupType: hangupType, deviceId: deviceId, useLegacyHangupMessage: useLegacyHangupMessage)
        }
    }

    func onSendBusy(callId: UInt64, remote: UnsafeRawPointer, destinationDeviceId: UInt32?) {
        Logger.debug("onSendBusy")

        DispatchQueue.main.async {
            Logger.debug("onSendBusy - main.async")

            guard let delegate = self.delegate else { return }

            let callReference: CallType = Unmanaged.fromOpaque(remote).takeUnretainedValue()
            delegate.callManager(self, shouldSendBusy: callId, call: callReference, destinationDeviceId: destinationDeviceId)
        }
    }

    func sendCallMessage(recipientUuid: UUID, message: Data) {
        Logger.debug("sendCallMessage")

        DispatchQueue.main.async {
            Logger.debug("sendCallMessage - main.async")

            guard let delegate = self.delegate else { return }

            delegate.callManager(self, shouldSendCallMessage: recipientUuid, message: message)
        }
    }

    func sendHttpRequest(requestId: UInt32, url: String, method: CallManagerHttpMethod, headers: [String: String], body: Data?) {
        Logger.debug("onSendHttpRequest")

        DispatchQueue.main.async {
            Logger.debug("onSendHttpRequest - main.async")

            guard let delegate = self.delegate else { return }

            delegate.callManager(self, shouldSendHttpRequest: requestId, url: url, method: method, headers: headers, body: body)
        }
    }

    // MARK: - Utility Observers

    func onCreateConnection(pcObserver: UnsafeMutableRawPointer?, deviceId: UInt32, appCallContext: CallContext, enableDtls: Bool, enableRtpDataChannel: Bool) -> (connection: Connection, pc: UnsafeMutableRawPointer?) {
        Logger.debug("onCreateConnection")

        // We create default configuration settings here as per
        // Signal Messenger policies.

        // Create the configuration.
        let configuration = RTCConfiguration()
        // All the connections of a given call should use the same certificate.
        configuration.certificate = appCallContext.certificate

        // Update the configuration with the provided Ice Servers.
        // @todo Validate and if none, set a backup value, don't expect
        // application to know what the backup should be.
        configuration.iceServers = appCallContext.iceServers

        // Initialize the configuration.
        configuration.bundlePolicy = .maxBundle
        configuration.rtcpMuxPolicy = .require
        configuration.tcpCandidatePolicy = .disabled

        if appCallContext.hideIp {
            configuration.iceTransportPolicy = .relay
        }

        configuration.enableDtlsSrtp = enableDtls
        configuration.enableRtpDataChannel = enableRtpDataChannel

        // Create the default media constraints.
        let constraints: RTCMediaConstraints
        if enableDtls {
            constraints = RTCMediaConstraints(mandatoryConstraints: nil, optionalConstraints: ["DtlsSrtpKeyAgreement": "true"])
        } else {
            constraints = RTCMediaConstraints(mandatoryConstraints: nil, optionalConstraints: nil)
        }

        Logger.debug("Create application connection object...")
        let connection = Connection(pcObserver: pcObserver!,
                                       factory: self.factory!,
                                 configuration: configuration,
                                   constraints: constraints)

        let pc = connection.getRawPeerConnection()
        // If pc is nil CallManager will handle it internally.

        // We always negotiate for both audio and video streams, add
        // them to the connection so WebRTC sets them up.

        // Add an Audio Sender to the connection.
        connection.createAudioSender(audioTrack: appCallContext.audioTrack)

        // Add a Video Sender to the connection.
        connection.createVideoSender(videoTrack: appCallContext.videoTrack)

        return (connection, pc)
    }

    func onConnectMedia(remote: UnsafeRawPointer, appCallContext: CallContext, stream: RTCMediaStream) {
        Logger.debug("onConnectMedia")

        guard stream.videoTracks.count > 0 else {
            owsFailDebug("Missing video stream")
            return
        }

        DispatchQueue.main.async {
            Logger.debug("onConnectMedia - main async")

            guard let delegate = self.delegate else { return }

            let callReference: CallType = Unmanaged.fromOpaque(remote).takeUnretainedValue()
            delegate.callManager(self, onAddRemoteVideoTrack: callReference, track: stream.videoTracks[0])
        }
    }

    func onCompareRemotes(remote1: UnsafeRawPointer, remote2: UnsafeRawPointer) -> Bool {
        Logger.debug("onCompareRemotes")

        // Invoke the delegate function synchronously.

        guard let delegate = self.delegate else {
            return false
        }

        let callReference1: CallType = Unmanaged.fromOpaque(remote1).takeUnretainedValue()
        let callReference2: CallType = Unmanaged.fromOpaque(remote2).takeUnretainedValue()
        return delegate.callManager(self, shouldCompareCalls: callReference1, call2: callReference2)
    }

    func onCallConcluded(remote: UnsafeRawPointer) {
        Logger.debug("onCallConcluded")

        DispatchQueue.main.async {
            Logger.debug("onCallConcluded - main.async")

            let unmanagedRemote: Unmanaged<CallType> = Unmanaged.fromOpaque(remote)

            // rust lib has signaled that it's done with the call reference
            unmanagedRemote.release()
        }
    }

    func handlePeekResponse(requestId: UInt32, peekInfo: PeekInfo) {
        Logger.debug("handlePeekResponse")

        DispatchQueue.main.async {
            Logger.debug("handlePeekResponse - main.async")

            if !self.peekInfoRequests.resolve(id: requestId, response: peekInfo) {
                Logger.warn("Invalid requestId for handlePeekResponse: \(requestId)")
            }
        }
    }

    // MARK: - Group Call Observers

    func requestMembershipProof(clientId: UInt32) {
        Logger.debug("requestMembershipProof")

        DispatchQueue.main.async {
            Logger.debug("requestMembershipProof - main.async")

            guard let groupCall = self.groupCallByClientId[clientId] else {
                return
            }

            groupCall.requestMembershipProof()
        }
    }

    func requestGroupMembers(clientId: UInt32) {
        Logger.debug("requestGroupMembers")

        DispatchQueue.main.async {
            Logger.debug("requestGroupMembers - main.async")

            guard let groupCall = self.groupCallByClientId[clientId] else {
                return
            }

            groupCall.requestGroupMembers()
        }
    }

    func handleConnectionStateChanged(clientId: UInt32, connectionState: ConnectionState) {
        Logger.debug("handleConnectionStateChanged")

        DispatchQueue.main.async {
            Logger.debug("handleConnectionStateChanged - main.async")

            guard let groupCall = self.groupCallByClientId[clientId] else {
                return
            }

            groupCall.handleConnectionStateChanged(connectionState: connectionState)
        }
    }

    func handleJoinStateChanged(clientId: UInt32, joinState: JoinState) {
        Logger.debug("handleJoinStateChanged")

        DispatchQueue.main.async {
            Logger.debug("handleJoinStateChanged - main.async")

            guard let groupCall = self.groupCallByClientId[clientId] else {
                return
            }

            groupCall.handleJoinStateChanged(joinState: joinState)
        }
    }

    func handleRemoteDevicesChanged(clientId: UInt32, remoteDeviceStates: [RemoteDeviceState]) {
        Logger.debug("handleRemoteDevicesChanged")

        DispatchQueue.main.async {
            Logger.debug("handleRemoteDevicesChanged - main.async")

            guard let groupCall = self.groupCallByClientId[clientId] else {
                return
            }

            groupCall.handleRemoteDevicesChanged(remoteDeviceStates: remoteDeviceStates)
        }
    }

    func handleIncomingVideoTrack(clientId: UInt32, remoteDemuxId: UInt32, nativeVideoTrack: UnsafeMutableRawPointer?) {
        Logger.debug("handleIncomingVideoTrack")

        DispatchQueue.main.async {
            Logger.debug("handleIncomingVideoTrack - main.async")

            guard let groupCall = self.groupCallByClientId[clientId] else {
                return
            }

            groupCall.handleIncomingVideoTrack(remoteDemuxId: remoteDemuxId, nativeVideoTrack: nativeVideoTrack)
        }
    }

    func handlePeekChanged(clientId: UInt32, peekInfo: PeekInfo) {
        Logger.debug("handlePeekChanged")

        DispatchQueue.main.async {
            Logger.debug("handlePeekChanged - main.async")

            guard let groupCall = self.groupCallByClientId[clientId] else {
                return
            }

            groupCall.handlePeekChanged(peekInfo: peekInfo)
        }
    }

    func handleEnded(clientId: UInt32, reason: GroupCallEndReason) {
        Logger.debug("handleEnded")

        DispatchQueue.main.async {
            Logger.debug("handleEnded - main.async")

            guard let groupCall = self.groupCallByClientId[clientId] else {
                return
            }

            groupCall.handleEnded(reason: reason)
        }
    }
}

func allocatedAppByteSliceFromArray(maybe_bytes: [UInt8]?) -> AppByteSlice {
    guard let bytes = maybe_bytes else {
        return AppByteSlice(bytes: nil, len: 0)
    }
    let ptr = UnsafeMutablePointer<UInt8>.allocate(capacity: bytes.count)
    ptr.initialize(from: bytes, count: bytes.count)
    return AppByteSlice(bytes: ptr, len: bytes.count)
}

func allocatedAppByteSliceFromString(maybe_string: String?) -> AppByteSlice {
    guard let string = maybe_string else {
        return allocatedAppByteSliceFromArray(maybe_bytes: nil)
    }
    return allocatedAppByteSliceFromArray(maybe_bytes: Array(string.utf8))
}

func allocatedAppByteSliceFromData(maybe_data: Data?) -> AppByteSlice {
    guard let data = maybe_data else {
        return allocatedAppByteSliceFromArray(maybe_bytes: nil)
    }
    return allocatedAppByteSliceFromArray(maybe_bytes: Array(data))
}
