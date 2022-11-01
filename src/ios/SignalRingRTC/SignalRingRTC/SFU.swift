//
// Copyright 2019-2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

// FFI that allows the application to make requests
// to the SFU, such as peeking a group call.

import SignalRingRTC.RingRTC
import SignalCoreKit

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

// Was previouly called GroupMemberInfo, so this makes
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

    static func fromRtc(_ rtcPeekResponse: rtc_sfu_PeekResponse) -> Self {
        var errorStatusCode: UInt16? = rtcPeekResponse.error_status_code.asUInt16()
        if errorStatusCode == 0 {
            errorStatusCode = nil
        }
        return PeekResponse(
            errorStatusCode: errorStatusCode,
            peekInfo: PeekInfo.fromRtc(rtcPeekResponse.peek_info)
        )
    }
}

// Same as rust sfu::PeekInfo (nicer version of rtc_sfu_PeekInfo)
public struct PeekInfo {
    public let joinedMembers: [UUID]
    public let creator: UUID?
    public let eraId: String?
    public let maxDevices: UInt32?
    public let deviceCount: UInt32

    static func fromRtc(_ rtcPeekInfo: rtc_sfu_PeekInfo) -> Self {
        return PeekInfo(
            joinedMembers: rtcPeekInfo.joined_members.toUUIDs(),
            creator: rtcPeekInfo.creator.toUUID(),
            eraId: rtcPeekInfo.era_id.toString(),
            maxDevices: rtcPeekInfo.max_devices.asUInt32(),
            deviceCount: rtcPeekInfo.device_count
        )
    }
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

public class SFUClient {
    private let httpClient: HTTPClient
    private let requests: Requests<PeekResponse> = Requests()

    public init(httpClient: HTTPClient) {
        self.httpClient = httpClient
    }

    public func peek(request: PeekRequest) -> Guarantee<PeekResponse> {
        AssertIsOnMainThread()
        Logger.debug("peekGroupCall")

        let (requestId, seal) = self.requests.add()
        let rtcRequest: rtc_sfu_PeekRequest = rtc_sfu_PeekRequest.allocate(request)
        defer {
            rtcRequest.deallocate()
        }
        let delegateWrapper = SFUDelegateWrapper(self)
        rtc_sfu_peek(self.httpClient.rtcClient, requestId, rtcRequest, delegateWrapper.asRtc())
        return seal
    }

    func handlePeekResponse(requestId: UInt32, response: PeekResponse) {
        let resolved = self.requests.resolve(id: requestId, response: response);
        if !resolved {
            Logger.warn("Invalid requestId for handlePeekResponse: \(requestId)")
        }
    }
}

// NOTE: We don't need an SFUDelegate from the app yet because of how we use Guarantees instead.
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
            handle_peek_response: { (unretained: UnsafeRawPointer?, requestId: UInt32, response: rtc_sfu_PeekResponse) in
                guard let unretained = unretained else {
                    return
                }

                let wrapper = SFUDelegateWrapper.from(unretained: unretained)
                let response = PeekResponse.fromRtc(response)

                Logger.debug("SFUDelegateWrapper.handlePeekResponse")

                DispatchQueue.main.async {
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
}


