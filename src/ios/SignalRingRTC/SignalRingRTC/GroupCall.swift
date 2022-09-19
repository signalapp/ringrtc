//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import SignalRingRTC.RingRTC
import WebRTC
import SignalCoreKit

/// Represents the connection state to a media server for a group call.
@available(iOSApplicationExtension, unavailable)
public enum ConnectionState: Int32 {
    case notConnected = 0
    case connecting = 1
    case connected = 2
    case reconnecting = 3
}

/// Represents whether or not a user is joined to a group call and can exchange media.
@available(iOSApplicationExtension, unavailable)
public enum JoinState: Int32 {
    case notJoined = 0
    case joining = 1
    case joined = 2
}

/// If not ended purposely by the user, gives the reason why a group call ended.
@available(iOSApplicationExtension, unavailable)
public enum GroupCallEndReason: Int32 {
    // Normal events
    case deviceExplicitlyDisconnected = 0
    case serverExplicitlyDisconnected = 1

    // Things that can go wrong
    case callManagerIsBusy = 2
    case sfuClientFailedToJoin = 3
    case failedToCreatePeerConnectionFactory = 4
    case failedToNegotiateSrtpKeys = 5
    case failedToCreatePeerConnection = 6
    case failedToStartPeerConnection = 7
    case failedToUpdatePeerConnection = 8
    case failedToSetMaxSendBitrate = 9
    case iceFailedWhileConnecting = 10
    case iceFailedAfterConnected = 11
    case serverChangedDemuxId = 12
    case hasMaxDevices = 13
}

/// The local device state for a group call.
@available(iOSApplicationExtension, unavailable)
public class LocalDeviceState {
    public internal(set) var connectionState: ConnectionState
    public internal(set) var networkRoute: NetworkRoute
    public internal(set) var joinState: JoinState
    public internal(set) var audioLevel: UInt16

    init() {
        self.connectionState = .notConnected
        self.joinState = .notJoined
        self.networkRoute = NetworkRoute(localAdapterType: .unknown)
        self.audioLevel = 0
    }
}

@available(iOSApplicationExtension, unavailable)
public class ReceivedAudioLevel {
    public let demuxId: UInt32
    public internal(set) var audioLevel: UInt16

    init(demuxId: UInt32, audioLevel: UInt16) {
        self.demuxId = demuxId
        self.audioLevel = audioLevel
    }
}

/// All remote devices in a group call and their associated state.
@available(iOSApplicationExtension, unavailable)
public class RemoteDeviceState: Hashable {
    public let demuxId: UInt32
    public var userId: UUID
    public var mediaKeysReceived: Bool

    public internal(set) var audioMuted: Bool?
    public internal(set) var videoMuted: Bool?
    public internal(set) var presenting: Bool?
    public internal(set) var sharingScreen: Bool?
    public internal(set) var addedTime: UInt64  // unix millis
    public internal(set) var speakerTime: UInt64  // unix millis; 0 if they've never spoken
    public internal(set) var forwardingVideo: Bool?
    public internal(set) var isHigherResolutionPending: Bool
    public internal(set) var audioLevel: UInt16

    public internal(set) var videoTrack: RTCVideoTrack?

    init(demuxId: UInt32, userId: UUID, mediaKeysReceived: Bool, addedTime: UInt64, speakerTime: UInt64, isHigherResolutionPending: Bool) {
        self.demuxId = demuxId
        self.userId = userId
        self.mediaKeysReceived = mediaKeysReceived
        self.addedTime = addedTime
        self.speakerTime = speakerTime
        self.isHigherResolutionPending = isHigherResolutionPending
        self.audioLevel = 0
    }

    public static func ==(lhs: RemoteDeviceState, rhs: RemoteDeviceState) -> Bool {
        return lhs.demuxId == rhs.demuxId && lhs.userId == rhs.userId
    }

    public func hash(into hasher: inout Hasher) {
        hasher.combine(demuxId)
        hasher.combine(userId)
    }
}

/// Used for the application to communicate the actual resolutions of
/// each device in a group call to RingRTC and the media server.
@available(iOSApplicationExtension, unavailable)
public struct VideoRequest {
    let demuxId: UInt32
    let width: UInt16
    let height: UInt16
    let framerate: UInt16?

    public init(demuxId: UInt32, width: UInt16, height: UInt16, framerate: UInt16?) {
        self.demuxId = demuxId
        self.width = width
        self.height = height
        self.framerate = framerate
    }
}

/// The group call observer.
@available(iOSApplicationExtension, unavailable)
public protocol GroupCallDelegate: AnyObject {
    /**
     * Indication that the application should provide an updated proof of membership
     * for the group call.
     */
    func groupCall(requestMembershipProof groupCall: GroupCall)

    /**
     * Indication that the application should provide the list of group members that
     * belong to the group for the purposes of the group call.
     */
    func groupCall(requestGroupMembers groupCall: GroupCall)

    /**
     * Indication that the application should retrieve the latest local device
     * state from the group call and refresh the presentation.
     */
    func groupCall(onLocalDeviceStateChanged groupCall: GroupCall)

    /**
     * Indication that the application should retrieve the latest remote device
     * states from the group call and refresh the presentation.
     */
    func groupCall(onRemoteDeviceStatesChanged groupCall: GroupCall)

    /**
     * Indication that the application should draw audio levels.
     */
    func groupCall(onAudioLevels groupCall: GroupCall)

    /**
     * Indication that the application can retrieve an updated PeekInfo which
     * includes a list of users that are actively in the group call.
     */
    func groupCall(onPeekChanged groupCall: GroupCall)

    /**
     * Indication that group call ended due to a reason other than the user choosing
     * to disconnect from it.
     */
    func groupCall(onEnded groupCall: GroupCall, reason: GroupCallEndReason)
}

@available(iOSApplicationExtension, unavailable)
public class GroupCall {
    let ringRtcCallManager: UnsafeMutableRawPointer
    let factory: RTCPeerConnectionFactory
    var groupCallByClientId: GroupCallByClientId
    let groupId: Data
    let sfuUrl: String
    let hkdfExtraInfo: Data
    let audioLevelsIntervalMillis: UInt64?

    public weak var delegate: GroupCallDelegate?

    // The clientId represents the id of the RingRTC object. For iOS, we
    // create the object in the context of the connect() API and recreate
    // it if it is ever ended abd connect() is called again.
    var clientId: UInt32?

    public private(set) var localDeviceState: LocalDeviceState
    public private(set) var remoteDeviceStates: [UInt32: RemoteDeviceState]
    public private(set) var peekInfo: PeekInfo?

    let videoCaptureController: VideoCaptureController
    var audioTrack: RTCAudioTrack?
    var videoTrack: RTCVideoTrack?

    internal init(ringRtcCallManager: UnsafeMutableRawPointer, factory: RTCPeerConnectionFactory, groupCallByClientId: GroupCallByClientId, groupId: Data, sfuUrl: String, hkdfExtraInfo: Data, audioLevelsIntervalMillis: UInt64?, videoCaptureController: VideoCaptureController) {
        AssertIsOnMainThread()

        self.ringRtcCallManager = ringRtcCallManager
        self.factory = factory
        self.groupCallByClientId = groupCallByClientId
        self.groupId = groupId
        self.sfuUrl = sfuUrl
        self.hkdfExtraInfo = hkdfExtraInfo
        self.audioLevelsIntervalMillis = audioLevelsIntervalMillis

        self.localDeviceState = LocalDeviceState()
        self.remoteDeviceStates = [:]

        self.videoCaptureController = videoCaptureController

        Logger.debug("object! GroupCall created... \(ObjectIdentifier(self))")
    }

    deinit {
        Logger.debug("object! GroupCall destroyed... \(ObjectIdentifier(self))")
    }

    // MARK: - APIs

    /// Connect to a group call, creating a client if one does not already exist.
    /// Return true if successful.
    public func connect() -> Bool {
        AssertIsOnMainThread()
        Logger.debug("connect")

        if self.clientId == nil {
            // There is no RingRTC instance yet or anymore, so create it.

            let groupIdSlice = allocatedAppByteSliceFromData(maybe_data: self.groupId)
            let sfuUrlSlice = allocatedAppByteSliceFromString(maybe_string: self.sfuUrl)
            let hkdfExtraInfoSlice = allocatedAppByteSliceFromData(maybe_data: self.hkdfExtraInfo)
            let audioLevelsIntervalMillis = self.audioLevelsIntervalMillis ?? 0;

            // Make sure to release the allocated memory when the function exists,
            // to ensure that the pointers are still valid when used in the RingRTC
            // API function.
            defer {
                if groupIdSlice.bytes != nil {
                    groupIdSlice.bytes.deallocate()
                }
                if sfuUrlSlice.bytes != nil {
                    sfuUrlSlice.bytes.deallocate()
                }
                if hkdfExtraInfoSlice.bytes != nil {
                    hkdfExtraInfoSlice.bytes.deallocate()
                }
            }

            let audioConstraints = RTCMediaConstraints(mandatoryConstraints: nil, optionalConstraints: nil)
            let audioSource = self.factory.audioSource(with: audioConstraints)
            // Note: This must stay "audio1" to stay in sync with CreateSessionDescriptionForGroupCall.
            let audioTrack = self.factory.audioTrack(with: audioSource, trackId: "audio1")
            audioTrack.isEnabled = !isOutgoingAudioMuted
            self.audioTrack = audioTrack

            let videoSource = self.factory.videoSource()
            // Note: This must stay "video1" to stay in sync with CreateSessionDescriptionForGroupCall.
            let videoTrack = self.factory.videoTrack(with: videoSource, trackId: "video1")
            videoTrack.isEnabled = !isOutgoingVideoMuted
            self.videoTrack = videoTrack

            // Define maximum output video format for group calls.
            videoSource.adaptOutputFormat(
                toWidth: 640,
                height: 360,
                fps: 30
            )

            self.videoCaptureController.capturerDelegate = videoSource

            // Note: getOwnedNativeAudioTrack/getOwnedNativeVideoTrack/getOwnedNativeFactory
            // return owned RCs the first time they are called, and null after that.
            // TODO: Consider renaming getOwnedNativeX to takeNative.
            let clientId = ringrtcCreateGroupCallClient(self.ringRtcCallManager, groupIdSlice, sfuUrlSlice, hkdfExtraInfoSlice, audioLevelsIntervalMillis, self.factory.getOwnedNativeFactory(), audioTrack.getOwnedNativeTrack(), videoTrack.getOwnedNativeTrack())
            if clientId != 0 {
                // Add this instance to the shared dictionary.
                self.groupCallByClientId[clientId] = self
                self.clientId = clientId
            } else {
                Logger.error("failed to create client for groupCall")
                return false
            }

            // Now that we have a client id, let RingRTC know the current audio/video mute state.
            ringrtcSetOutgoingAudioMuted(self.ringRtcCallManager, clientId, isOutgoingAudioMuted)
            ringrtcSetOutgoingVideoMuted(self.ringRtcCallManager, clientId, isOutgoingVideoMuted)
        }

        guard let clientId = self.clientId else {
            Logger.warn("no clientId defined for groupCall")
            return false
        }

        ringrtcConnect(self.ringRtcCallManager, clientId)

        return true
    }

    public func join() {
        AssertIsOnMainThread()
        Logger.debug("join")

        guard let clientId = self.clientId else {
            Logger.warn("no clientId defined for groupCall")
            return
        }

        ringrtcJoin(self.ringRtcCallManager, clientId)
    }

    public func leave() {
        AssertIsOnMainThread()
        Logger.debug("leave")

        guard let clientId = self.clientId else {
            Logger.warn("no clientId defined for groupCall")
            return
        }

        // When leaving, make sure outgoing media is stopped as soon as possible.
        self.audioTrack?.isEnabled = false
        self.videoTrack?.isEnabled = false

        ringrtcLeave(self.ringRtcCallManager, clientId)
    }

    public func disconnect() {
        AssertIsOnMainThread()
        Logger.debug("disconnect")

        guard let clientId = self.clientId else {
            Logger.warn("no clientId defined for groupCall")
            return
        }

        // When disconnecting, make sure outgoing media is stopped as soon as possible.
        self.audioTrack?.isEnabled = false
        self.videoTrack?.isEnabled = false

        ringrtcDisconnect(self.ringRtcCallManager, clientId)
    }

    private var _isOutgoingAudioMuted = false
    public var isOutgoingAudioMuted: Bool {
        get {
            AssertIsOnMainThread()
            return _isOutgoingAudioMuted
        }
        set {
            AssertIsOnMainThread()
            Logger.debug("setOutgoingAudioMuted")

            _isOutgoingAudioMuted = newValue
            self.audioTrack?.isEnabled = !_isOutgoingAudioMuted

            guard let clientId = self.clientId else {
                Logger.warn("no clientId defined for groupCall")
                return
            }

            ringrtcSetOutgoingAudioMuted(self.ringRtcCallManager, clientId, newValue)
        }
    }

    private var _isOutgoingVideoMuted = false
    public var isOutgoingVideoMuted: Bool {
        get {
            AssertIsOnMainThread()
            return _isOutgoingVideoMuted
        }
        set {
            AssertIsOnMainThread()
            Logger.debug("setOutgoingVideoMuted")

            _isOutgoingVideoMuted = newValue
            self.videoTrack?.isEnabled = !_isOutgoingVideoMuted

            guard let clientId = self.clientId else {
                Logger.warn("no clientId defined for groupCall")
                return
            }

            ringrtcSetOutgoingVideoMuted(self.ringRtcCallManager, clientId, newValue)
        }
    }

    public func ringAll() {
        AssertIsOnMainThread()
        Logger.debug("ring")

        guard let clientId = self.clientId else {
            Logger.warn("no clientId defined for groupCall")
            return
        }

        ringrtcGroupRing(self.ringRtcCallManager, clientId, AppByteSlice(bytes: nil, len: 0))
    }

    public func resendMediaKeys() {
        AssertIsOnMainThread()
        Logger.debug("resendMediaKeys")

        guard let clientId = self.clientId else {
            Logger.warn("no clientId defined for groupCall")
            return
        }

        ringrtcResendMediaKeys(self.ringRtcCallManager, clientId)
    }

    public func updateBandwidthMode(bandwidthMode: BandwidthMode) {
        AssertIsOnMainThread()
        Logger.debug("updateBandwidthMode")

        guard let clientId = self.clientId else {
            Logger.warn("no clientId defined for groupCall")
            return
        }

        ringrtcSetBandwidthMode(self.ringRtcCallManager, clientId, bandwidthMode.rawValue)
    }

    /// Provides a collection of VideoRequest objects to the group call
    /// object which are sent to the SFU. This allows the appropriate
    /// video resolution to be sent from the SFU to efficiently fit in
    /// rendered resolution on the screen.
    ///
    /// - parameter resolutions: the VideoRequest objects for each user rendered on the screen
    /// - parameter activeSpeakerHeight: the height of the view for the active speaker, in pixels
    public func updateVideoRequests(resolutions: [VideoRequest], activeSpeakerHeight: UInt16) {
        AssertIsOnMainThread()
        Logger.debug("updateVideoRequests")

        guard let clientId = self.clientId else {
            Logger.warn("no clientId defined for groupCall")
            return
        }

        let appResolutions: [AppVideoRequest] = resolutions.map { resolution in
            let appFramerate: AppOptionalUInt16
            if resolution.framerate != nil {
                appFramerate = AppOptionalUInt16(value: resolution.framerate!, valid: true)
            } else {
                appFramerate = AppOptionalUInt16(value: 0, valid: false)
            }

            return AppVideoRequest(demux_id: resolution.demuxId, width: resolution.width, height: resolution.height, framerate: appFramerate)
        }

        var appResolutionArray = appResolutions.withUnsafeBufferPointer { appResolutionBytes in
            return AppVideoRequestArray(
                resolutions: appResolutionBytes.baseAddress,
                count: resolutions.count
            )
        }

        ringrtcRequestVideo(self.ringRtcCallManager, clientId, &appResolutionArray, activeSpeakerHeight)
    }

    public func updateGroupMembers(members: [GroupMember]) {
        AssertIsOnMainThread()
        Logger.debug("updateGroupMembers")

        guard let clientId = self.clientId else {
            Logger.warn("no clientId defined for groupCall")
            return
        }

        let appMembers: [AppGroupMemberInfo] = members.map { member in
            let userIdSlice = allocatedAppByteSliceFromData(maybe_data: member.userId.data)
            let memberIdSlice = allocatedAppByteSliceFromData(maybe_data: member.userIdCipherText)

            return AppGroupMemberInfo(userId: userIdSlice, memberId: memberIdSlice)
        }

        // Make sure to release the allocated memory when the function exists,
        // to ensure that the pointers are still valid when used in the RingRTC
        // API function.
        defer {
            for appMember in appMembers {
                if appMember.userId.bytes != nil {
                    appMember.userId.bytes.deallocate()
                }
                if appMember.memberId.bytes != nil {
                    appMember.memberId.bytes.deallocate()
                }
            }
        }

        var appGroupMemberInfoArray = appMembers.withUnsafeBufferPointer { appMembersBytes in
            return AppGroupMemberInfoArray(
                members: appMembersBytes.baseAddress,
                count: members.count
            )
        }

        ringrtcSetGroupMembers(self.ringRtcCallManager, clientId, &appGroupMemberInfoArray)
    }

    public func updateMembershipProof(proof: Data) {
        AssertIsOnMainThread()
        Logger.debug("updateMembershipProof")

        guard let clientId = self.clientId else {
            Logger.warn("no clientId defined for groupCall")
            return
        }

        let proofSlice = allocatedAppByteSliceFromData(maybe_data: proof)

        // Make sure to release the allocated memory when the function exists,
        // to ensure that the pointers are still valid when used in the RingRTC
        // API function.
        defer {
            if proofSlice.bytes != nil {
                proofSlice.bytes.deallocate()
            }
        }

        ringrtcSetMembershipProof(self.ringRtcCallManager, clientId, proofSlice)
    }

    // MARK: - Internal Callback Handlers

    func requestMembershipProof() {
        AssertIsOnMainThread()

        self.delegate?.groupCall(requestMembershipProof: self)
    }

    func requestGroupMembers() {
        AssertIsOnMainThread()

        self.delegate?.groupCall(requestGroupMembers: self)
    }

    func handleConnectionStateChanged(connectionState: ConnectionState) {
        AssertIsOnMainThread()

        self.localDeviceState.connectionState = connectionState

        self.delegate?.groupCall(onLocalDeviceStateChanged: self)
    }

    func handleNetworkRouteChanged(networkRoute: NetworkRoute) {
        AssertIsOnMainThread()

        self.localDeviceState.networkRoute = networkRoute;

        self.delegate?.groupCall(onLocalDeviceStateChanged: self)
    }

    func handleAudioLevels(capturedLevel: UInt16, receivedLevels: [ReceivedAudioLevel]) {
        AssertIsOnMainThread()

        self.localDeviceState.audioLevel = capturedLevel;
        for received in receivedLevels {
            let remoteDeviceState = self.remoteDeviceStates[received.demuxId]
            if remoteDeviceState != nil {
                remoteDeviceState?.audioLevel = received.audioLevel
            }
        }

        self.delegate?.groupCall(onAudioLevels: self)
    }

    func handleJoinStateChanged(joinState: JoinState) {
       AssertIsOnMainThread()

       self.localDeviceState.joinState = joinState

       self.delegate?.groupCall(onLocalDeviceStateChanged: self)
    }

    func handleRemoteDevicesChanged(remoteDeviceStates: [RemoteDeviceState]) {
        AssertIsOnMainThread()
        Logger.debug("handleRemoteDevicesChanged() count: \(remoteDeviceStates.count)")

        var remoteDeviceByDemuxId: [UInt32: RemoteDeviceState] = [:]
        for remoteDeviceState in remoteDeviceStates {
            // Maintain the video track and audio level if one already exists.
            let existingDeviceState = self.remoteDeviceStates[remoteDeviceState.demuxId]
            if existingDeviceState != nil {
                remoteDeviceState.videoTrack = existingDeviceState?.videoTrack
                remoteDeviceState.audioLevel = existingDeviceState?.audioLevel ?? 0
            }

            // Build the dictionary version of the array with demuxId as the key.
            remoteDeviceByDemuxId[remoteDeviceState.demuxId] = remoteDeviceState
        }

        self.remoteDeviceStates = remoteDeviceByDemuxId

        self.delegate?.groupCall(onRemoteDeviceStatesChanged: self)
    }

    func handleIncomingVideoTrack(remoteDemuxId: UInt32, videoTrack: RTCVideoTrack) {
        AssertIsOnMainThread()
        Logger.debug("handleIncomingVideoTrack() for remoteDemuxId: 0x\(String(remoteDemuxId, radix: 16))")

        guard let remoteDeviceState = self.remoteDeviceStates[remoteDemuxId] else {
            Logger.debug("No remote device state found for remoteDemuxId")
            return
        }

        remoteDeviceState.videoTrack = videoTrack

        self.delegate?.groupCall(onRemoteDeviceStatesChanged: self)
    }

    func handlePeekChanged(peekInfo: PeekInfo) {
        AssertIsOnMainThread()

        self.peekInfo = peekInfo

        self.delegate?.groupCall(onPeekChanged: self)
    }

    func handleEnded(reason: GroupCallEndReason) {
        AssertIsOnMainThread()

        guard let clientId = self.clientId else {
            Logger.error("no clientId defined for groupCall")
            return
        }

        // Take this instance out of the shared dictionary and reset the
        // associated clientId (because it will no longer exist in RingRTC).
        self.groupCallByClientId[clientId] = nil
        self.clientId = nil

        // Reset the other states so that the object can be used again.
        self.localDeviceState = LocalDeviceState()
        self.remoteDeviceStates = [:]
        self.peekInfo = nil
        self.audioTrack = nil
        self.videoTrack = nil

        ringrtcDeleteGroupCallClient(self.ringRtcCallManager, clientId)

        self.delegate?.groupCall(onEnded: self, reason: reason)
    }
}
