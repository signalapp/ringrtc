//
// Copyright 2019-2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

// FFI that allows the application to make requests
// to the SFU, such as peeking a group call.

import SignalRingRTC.RingRTC

public struct PeekRequest {
    public let sfuURL: String
    public let membershipProof: Data
    public let groupMembers: [GroupMember]

    public init(sfuURL: String, membershipProof: Data, groupMembers: [GroupMember]) {
        self.sfuURL = sfuURL
        self.membershipProof = membershipProof
        self.groupMembers = groupMembers
    }
}

extension rtc_sfu_PeekRequest {
    // Don't forget to call deallocate()
    static func allocate(_ request: PeekRequest) -> Self {
        return Self(
            sfu_url: rtc_String.allocate(from: request.sfuURL),
            membership_proof: rtc_Bytes.allocate(from: request.membershipProof),
            group_members: rtc_sfu_GroupMembers.allocate(request.groupMembers)
        )
    }

    func deallocate() {
        self.sfu_url.deallocate()
        self.membership_proof.deallocate()
        self.group_members.deallocate()
    }
}

extension rtc_sfu_GroupMembers {
    static func allocate(_ array: [GroupMember]) -> Self {
        let ptr = UnsafeMutablePointer<rtc_sfu_GroupMember>.allocate(capacity: array.count)
        for i in 0..<array.count {
            ptr[i] = rtc_sfu_GroupMember.allocate(array[i])
        }
        return Self(ptr: ptr, count: array.count)
    }

    func deallocate() {
        for i in 0..<self.count {
            self.ptr[i].deallocate()
        }
        self.ptr.deallocate()
    }
}

// Same as rust sfu::GroupMember (nicer version of rtc_sfu_GroupMember)
public struct GroupMember {
    public let userId: UUID
    // AKA memberId
    public let userIdCipherText: Data

    public init(userId: UUID, userIdCipherText: Data) {
        self.userId = userId
        self.userIdCipherText = userIdCipherText
    }
}

// Was previously called GroupMemberInfo, so this makes
// it a little easier to use with existing code.
public typealias GroupMemberInfo = GroupMember

extension rtc_sfu_GroupMember {
    static func allocate(_ groupMember: GroupMember) -> Self {
        return Self(
            user_id: rtc_Bytes.allocate(from: groupMember.userId.data),
            member_id: rtc_Bytes.allocate(from: groupMember.userIdCipherText)
        )
    }

    func deallocate() {
        self.user_id.deallocate()
        self.member_id.deallocate()
    }
}

// Same as rust sfu::PeekResponse (nicer version of rtc_sfu_PeekResponse)
public struct PeekResponse {
    public let errorStatusCode: UInt16?
    public let peekInfo: PeekInfo

    static func fromRtc(_ rtcPeekResponse: rtc_sfu_Response_rtc_sfu_PeekInfo) -> Self {
        var errorStatusCode: UInt16? = rtcPeekResponse.error_status_code.asUInt16()
        if errorStatusCode == 0 {
            errorStatusCode = nil
        }
        return PeekResponse(
            errorStatusCode: errorStatusCode,
            peekInfo: PeekInfo.fromRtc(rtcPeekResponse.value)
        )
    }
}

// Same as rust sfu::PeekInfo (nicer version of rtc_sfu_PeekInfo)
public struct PeekInfo {
    /// In a peek response, indicates that a call link has expired or been revoked.
    public static let expiredCallLinkStatus: UInt16 = 703

    /// In a peek response, indicates that a call link is invalid.
    ///
    /// It may have expired a long time ago.
    public static let invalidCallLinkStatus: UInt16 = 704

    public let joinedMembers: [UUID]
    public let creator: UUID?
    public let eraId: String?
    public let maxDevices: UInt32?
    public let deviceCountIncludingPendingDevices: UInt32
    public let deviceCountExcludingPendingDevices: UInt32
    public let pendingUsers: [UUID]

    static func fromRtc(_ rtcPeekInfo: rtc_sfu_PeekInfo) -> Self {
        return PeekInfo(
            joinedMembers: rtcPeekInfo.joined_members.toUUIDs(),
            creator: rtcPeekInfo.creator.toUUID(),
            eraId: rtcPeekInfo.era_id.toString(),
            maxDevices: rtcPeekInfo.max_devices.asUInt32(),
            deviceCountIncludingPendingDevices: rtcPeekInfo.device_count_including_pending_devices,
            deviceCountExcludingPendingDevices: rtcPeekInfo.device_count_excluding_pending_devices,
            pendingUsers: rtcPeekInfo.pending_users.toUUIDs()
        )
    }

    @available(*, deprecated, message: "use 'deviceCountIncludingPendingDevices' or 'deviceCountExcludingPendingDevices' as appropriate")
    public var deviceCount: UInt32 { deviceCountIncludingPendingDevices }
}

extension rtc_UserIds {
    func toUUIDs() -> [UUID] {
        var uuids: [UUID] = []
        for i in 0..<self.count {
            guard let uuid = self.ptr[i].toUUID() else {
                Logger.debug("missing userId")
                continue
            }
            uuids.append(uuid)
        }
        return uuids
    }
}

public enum SFUResult<Value> {
    case success(Value)
    case failure(UInt16)
}

public class SFUClient {
    private let httpClient: HTTPClient
    private let peekRequests: Requests<PeekResponse> = Requests()
    private let callLinkRequests: Requests<SFUResult<CallLinkState>> = Requests()
    private let emptyRequests: Requests<SFUResult<()>> = Requests()

    public init(httpClient: HTTPClient) {
        self.httpClient = httpClient
    }

    @MainActor
    public func peek(request: PeekRequest) async -> PeekResponse {
        return await withCheckedContinuation { continuation in
            Logger.debug("peekGroupCall")

            let requestId = self.peekRequests.add(continuation)
            let rtcRequest: rtc_sfu_PeekRequest = rtc_sfu_PeekRequest.allocate(request)
            defer {
                rtcRequest.deallocate()
            }
            let delegateWrapper = SFUDelegateWrapper(self)
            rtc_sfu_peek(self.httpClient.rtcClient, requestId, rtcRequest, delegateWrapper.asRtc())
        }
    }

    /// Asynchronous request for the active call state from the SFU for a particular
    /// call link. Does not require a group call object.
    ///
    /// Possible (synthetic) failure codes include:
    /// - `PeekInfo.expiredCallLinkStatus`: the call link has expired or been revoked
    /// - `PeekInfo.invalidCallLinkStatus`: the call link is invalid; it may have expired a long time ago
    ///
    /// Will produce an "empty" `PeekInfo` if the link is valid but no call is active.
    ///
    /// - Parameter sfuUrl: The URL to use when accessing the SFU.
    /// - Parameter authCredentialPresentation: A serialized `CallLinkAuthCredentialPresentation`
    /// - Parameter linkRootKey: The root key for the call link
    @MainActor
    public func peek(sfuUrl: String, authCredentialPresentation: [UInt8], linkRootKey: CallLinkRootKey) async -> PeekResponse {
        return await withCheckedContinuation { continuation in
            Logger.debug("peekCallLinkCall")

            let requestId = self.peekRequests.add(continuation)
            let delegateWrapper = SFUDelegateWrapper(self)
            authCredentialPresentation.withRtcBytes { authCredentialPresentation in
                linkRootKey.bytes.withRtcBytes { linkRootKey in
                    rtc_sfu_peekCallLink(self.httpClient.rtcClient, requestId, sfuUrl, authCredentialPresentation, linkRootKey, delegateWrapper.asRtc())
                }
            }
        }
    }

    @MainActor
    func handlePeekResponse(requestId: UInt32, response: PeekResponse) {
        let resolved = self.peekRequests.resolve(id: requestId, response: response);
        if !resolved {
            Logger.warn("Invalid requestId for handlePeekResponse: \(requestId)")
        }
    }

    @MainActor
    func handleCallLinkResponse(requestId: UInt32, response: SFUResult<CallLinkState>) {
        let resolved = self.callLinkRequests.resolve(id: requestId, response: response)
        if !resolved {
            Logger.warn("Invalid requestId for handleCallLinkResponse: \(requestId)")
        }
    }

    @MainActor
    func handleEmptyResponse(requestId: UInt32, response: SFUResult<()>) {
        let resolved = self.emptyRequests.resolve(id: requestId, response: response)
        if !resolved {
            Logger.warn("Invalid requestId for handleEmptyResponse: \(requestId)")
        }
    }

    /// Asynchronous request to get information about a call link.
    ///
    /// - Parameter sfuUrl: the URL to use when accessing the SFU
    /// - Parameter authCredentialPresentation: a serialized CallLinkAuthCredentialPresentation
    /// - Parameter linkRootKey: the root key for the call link
    ///
    /// Expected failure codes include:
    /// - 404: the room does not exist (or expired so long ago that it has been removed from the server)
    @MainActor
    public func readCallLink(sfuUrl: String, authCredentialPresentation: [UInt8], linkRootKey: CallLinkRootKey) async -> SFUResult<CallLinkState> {
        return await withCheckedContinuation { continuation in
            Logger.debug("createCallLink")

            let requestId = self.callLinkRequests.add(continuation)
            let delegateWrapper = SFUDelegateWrapper(self)
            authCredentialPresentation.withRtcBytes { authCredentialPresentation in
                linkRootKey.bytes.withRtcBytes { linkRootKey in
                    rtc_sfu_readCallLink(self.httpClient.rtcClient, requestId, sfuUrl, authCredentialPresentation, linkRootKey, delegateWrapper.asRtc())
                }
            }
        }
    }

    /// Asynchronous request to create a new call link.
    ///
    /// This request is idempotent; if it fails due to a network issue, it is safe to retry.
    ///
    /// ```
    /// let linkKey = CallLinkRootKey.generate()
    /// let adminPasskey = CallLinkRootKey.generateAdminPasskey()
    /// let roomId = linkKey.deriveRoomId()
    /// CreateCallLinkCredential credential = requestCreateCredentialFromChatServer(roomId) // using libsignal
    /// let secretParams = CallLinkSecretParams.deriveFromRootKey(linkKey.bytes)
    /// let credentialPresentation = credential.present(roomId, secretParams).serialize()
    /// let serializedPublicParams = secretParams.getPublicParams().serialize()
    /// let restrictions = CallLinkState.Restrictions.adminApprovla
    /// sfu.createCallLink(
    ///     sfuUrl: sfuUrl,
    ///     createCredentialPresentation: credentialPresentation,
    ///     linkRootKey: linkKey,
    ///     adminPasskey: adminPasskey,
    ///     callLinkPublicParams: serializedPublicParams,
    ///     restrictions: restrictions
    /// ).done { result in
    ///   switch result {
    ///   case .success(let state):
    ///     // In actuality you may not want to do this until the user clicks Done.
    ///     saveToDatabase(linkKey.bytes, adminPasskey, state)
    ///     syncToOtherDevices(linkKey.bytes, adminPasskey)
    ///   case .failure(409):
    ///     // The room already exists (and isn't yours), i.e. you've hit a 1-in-a-billion conflict.
    ///     fallthrough
    ///   case .failure(let code):
    ///     // Unexpected error, kick the user out for now.
    ///   }
    /// }
    /// ```
    ///
    /// - Parameter sfuUrl: the URL to use when accessing the SFU
    /// - Parameter createCredentialPresentation: a serialized CreateCallLinkCredentialPresentation
    /// - Parameter linkRootKey: the root key for the call link
    /// - Parameter adminPasskey: the arbitrary passkey to use for the new room
    /// - Parameter callLinkPublicParams: the serialized CallLinkPublicParams for the new room
    /// - Parameter restrictions: the restrictions for joining the room. Restrictions.unknown is invalid for creation
    @MainActor
    public func createCallLink(sfuUrl: String, createCredentialPresentation: [UInt8], linkRootKey: CallLinkRootKey, adminPasskey: Data, callLinkPublicParams: [UInt8], restrictions: CallLinkState.Restrictions) async -> SFUResult<CallLinkState> {
        return await withCheckedContinuation { continuation in
            Logger.debug("createCallLink")

            let requestId = self.callLinkRequests.add(continuation)
            let delegateWrapper = SFUDelegateWrapper(self)
            createCredentialPresentation.withRtcBytes { createCredentialPresentation in
                linkRootKey.bytes.withRtcBytes { linkRootKey in
                    adminPasskey.withRtcBytes { adminPasskey in
                        callLinkPublicParams.withRtcBytes { callLinkPublicParams in
                            let rawRestrictions = restrictions.toOrdinal()
                            if rawRestrictions < 0 {
                                preconditionFailure("cannot create call link with restrictions 'unknown'")
                            }
                            rtc_sfu_createCallLink(self.httpClient.rtcClient, requestId, sfuUrl, createCredentialPresentation, linkRootKey, adminPasskey, callLinkPublicParams, rawRestrictions, delegateWrapper.asRtc())
                        }
                    }
                }
            }
        }
    }

    /// Asynchronous request to update a call link's name.
    ///
    /// Possible failure codes include:
    /// - 401: the room does not exist (and this is the wrong API to create a new room)
    /// - 403: the admin passkey is incorrect
    ///
    /// This request is idempotent; if it fails due to a network issue, it is safe to retry.
    ///
    /// - Parameter sfuUrl: the URL to use when accessing the SFU
    /// - Parameter authCredentialPresentation: a serialized CallLinkAuthCredentialPresentation
    /// - Parameter linkRootKey: the root key for the call link
    /// - Parameter adminPasskey: the passkey specified when the link was created
    /// - Parameter newName: the new name to use
    @MainActor
    public func updateCallLinkName(sfuUrl: String, authCredentialPresentation: [UInt8], linkRootKey: CallLinkRootKey, adminPasskey: Data, newName: String) async -> SFUResult<CallLinkState> {
        return await withCheckedContinuation { continuation in
            Logger.debug("updateCallLinkName")

            let requestId = self.callLinkRequests.add(continuation)
            let delegateWrapper = SFUDelegateWrapper(self)
            authCredentialPresentation.withRtcBytes { createCredentialPresentation in
                linkRootKey.bytes.withRtcBytes { linkRootKey in
                    adminPasskey.withRtcBytes { adminPasskey in
                        rtc_sfu_updateCallLink(self.httpClient.rtcClient, requestId, sfuUrl, createCredentialPresentation, linkRootKey, adminPasskey, newName, -1, -1, delegateWrapper.asRtc())
                    }
                }
            }
        }
    }

    /// Asynchronous request to update a call link's restrictions.
    ///
    /// Possible failure codes include:
    /// - 401: the room does not exist (and this is the wrong API to create a new room)
    /// - 403: the admin passkey is incorrect
    /// - 409: the room is currently in use, so restrictions cannot be changed at the moment
    ///
    /// This request is idempotent; if it fails due to a network issue, it is safe to retry.
    ///
    /// - Parameter sfuUrl: the URL to use when accessing the SFU
    /// - Parameter authCredentialPresentation: a serialized CallLinkAuthCredentialPresentation
    /// - Parameter linkRootKey: the root key for the call link
    /// - Parameter adminPasskey: the passkey specified when the link was created
    /// - Parameter restrictions: the new restrictions
    @MainActor
    public func updateCallLinkRestrictions(sfuUrl: String, authCredentialPresentation: [UInt8], linkRootKey: CallLinkRootKey, adminPasskey: Data, restrictions: CallLinkState.Restrictions) async -> SFUResult<CallLinkState> {
        return await withCheckedContinuation { continuation in
            Logger.debug("updateCallLinkRestrictions")

            let requestId = self.callLinkRequests.add(continuation)
            let delegateWrapper = SFUDelegateWrapper(self)
            authCredentialPresentation.withRtcBytes { createCredentialPresentation in
                linkRootKey.bytes.withRtcBytes { linkRootKey in
                    adminPasskey.withRtcBytes { adminPasskey in
                        let rawRestrictions = restrictions.toOrdinal()
                        if rawRestrictions < 0 {
                            preconditionFailure("cannot update restrictions to 'unknown'")
                        }
                        rtc_sfu_updateCallLink(self.httpClient.rtcClient, requestId, sfuUrl, createCredentialPresentation, linkRootKey, adminPasskey, nil, rawRestrictions, -1, delegateWrapper.asRtc())
                    }
                }
            }
        }
    }

    /// Asynchronous request to delete a call link.
    ///
    /// Possible failure codes include:
    /// - 403: the admin passkey is incorrect
    /// - 409: the room has an ongoing call.
    ///
    /// This request is idempotent; if it fails due to a network issue, it is safe to retry.
    ///
    /// - Parameter sfuUrl: the URL to use when accessing the SFU
    /// - Parameter authCredentialPresentation: a serialized CallLinkAuthCredentialPresentation
    /// - Parameter linkRootKey: the root key for the call link
    /// - Parameter adminPasskey: the passkey specified when the link was created
    @MainActor
    public func deleteCallLink(sfuUrl: String, authCredentialPresentation: [UInt8], linkRootKey: CallLinkRootKey, adminPasskey: Data) async -> SFUResult<()> {
        return await withCheckedContinuation { continuation in
            Logger.debug("deleteCallLink")

            let requestId = self.emptyRequests.add(continuation)
            let delegateWrapper = SFUDelegateWrapper(self)
            authCredentialPresentation.withRtcBytes { createCredentialPresentation in
                linkRootKey.bytes.withRtcBytes { linkRootKey in
                    adminPasskey.withRtcBytes { adminPasskey in
                        rtc_sfu_deleteCallLink(self.httpClient.rtcClient, requestId, sfuUrl, createCredentialPresentation, linkRootKey, adminPasskey, delegateWrapper.asEmptyRtc())
                    }
                }
            }
        }
    }
}

// NOTE: We don't need an SFUDelegate from the app yet.
// But it's still nice to follow the same model as HTTPDelegateWrapper.
// Plus, we need a weak ref somewhere.
private class SFUDelegateWrapper {
    // We make this weak to avoid a reference cycle
    // from SFUClient -> rtc_sfu_Delegate -> SFUDelegateWrapper -> SFUClient
    weak var delegate: SFUClient?

    init(_ delegate: SFUClient? = nil) {
        self.delegate = delegate
    }

    func asRetainedPtr() -> UnsafeMutableRawPointer {
        return UnsafeMutableRawPointer(Unmanaged.passRetained(self).toOpaque())
    }

    static func from(retained: UnsafeRawPointer) -> Self {
        return Unmanaged<Self>.fromOpaque(retained).takeRetainedValue()
    }

    static func from(unretained: UnsafeRawPointer) -> Self {
        return Unmanaged<Self>.fromOpaque(unretained).takeUnretainedValue()
    }

    func asRtc() -> rtc_sfu_Delegate {
        return rtc_sfu_Delegate(
            retained: self.asRetainedPtr(),
            release: { (retained: UnsafeMutableRawPointer?) in
                guard let retained = retained else {
                    return
                }

                _ = SFUDelegateWrapper.from(retained: retained)
            },
            handle_peek_response: { (unretained: UnsafeRawPointer?, requestId: UInt32, response: rtc_sfu_Response_rtc_sfu_PeekInfo) in
                guard let unretained = unretained else {
                    return
                }

                let wrapper = SFUDelegateWrapper.from(unretained: unretained)
                let response = PeekResponse.fromRtc(response)

                Logger.debug("SFUDelegateWrapper.handlePeekResponse")

                Task { @MainActor in
                    Logger.debug("SFUDelegateWrapper.handlePeekResponse - main.async")

                    guard let delegate = wrapper.delegate else {
                        // Response came back after SFUClient was deleted
                        return
                    }
                    delegate.handlePeekResponse(requestId: requestId, response: response)
                }
            }
        )
    }

    func asRtc() -> rtc_sfu_CallLinkDelegate {
        return rtc_sfu_CallLinkDelegate(
            retained: self.asRetainedPtr(),
            release: { (retained: UnsafeMutableRawPointer?) in
                guard let retained = retained else {
                    return
                }

                _ = SFUDelegateWrapper.from(retained: retained)
            },
            handle_response: { (unretained: UnsafeRawPointer?, requestId: UInt32, response: rtc_sfu_Response_rtc_calllinks_CallLinkState) in
                guard let unretained = unretained else {
                    return
                }

                let wrapper = SFUDelegateWrapper.from(unretained: unretained)
                let result: SFUResult<CallLinkState>

                if let errorStatusCode = response.error_status_code.asUInt16() {
                    result = .failure(errorStatusCode)
                } else {
                    result = .success(CallLinkState.fromRtc(response.value))
                }

                Logger.debug("SFUDelegateWrapper.handleResponse")

                Task { @MainActor in
                    Logger.debug("SFUDelegateWrapper.handleResponse - main.async")

                    guard let delegate = wrapper.delegate else {
                        // Response came back after SFUClient was deleted
                        return
                    }
                    delegate.handleCallLinkResponse(requestId: requestId, response: result)
                }
            }
        )
    }

    func asEmptyRtc() -> rtc_sfu_EmptyDelegate {
        return rtc_sfu_EmptyDelegate(
            retained: self.asRetainedPtr(),
            release: { (retained: UnsafeMutableRawPointer?) in
                guard let retained = retained else {
                    return
                }

                _ = SFUDelegateWrapper.from(retained: retained)
            },
            handle_response: { (unretained: UnsafeRawPointer?, requestId: UInt32, response: rtc_sfu_Response_bool) in
                guard let unretained = unretained else {
                    return
                }

                let wrapper = SFUDelegateWrapper.from(unretained: unretained)
                let result: SFUResult<()>

                if let errorStatusCode = response.error_status_code.asUInt16() {
                    result = .failure(errorStatusCode)
                } else {
                    result = .success(())
                }

                Logger.debug("SFUDelegateWrapper.handleEmptyResponse")

                Task { @MainActor in
                    Logger.debug("SFUDelegateWrapper.handleEmptyResponse - main.async")

                    guard let delegate = wrapper.delegate else {
                        // Response came back after SFUClient was deleted
                        return
                    }
                    delegate.handleEmptyResponse(requestId: requestId, response: result)
                }
            }
        )
    }
}


