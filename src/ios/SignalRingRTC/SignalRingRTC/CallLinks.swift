//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import SignalRingRTC.RingRTC

public struct CallLinkRootKey: CustomStringConvertible {
    public struct ValidationError: Error {}

    public let bytes: Data

    public init(_ string: String) throws {
        var result: Self? = nil
        rtc_calllinks_CallLinkRootKey_parse(string, &result) { resultOpaquePtr, parsedBytes in
            resultOpaquePtr!.assumingMemoryBound(to: Optional<Self>.self).pointee = Self(validatedBytes: parsedBytes)
        }
        guard let result else {
            throw ValidationError()
        }
        self = result
    }

    public init(_ bytes: Data) throws {
        try bytes.withRtcBytes { bytes in
            if !rtc_calllinks_CallLinkRootKey_validate(bytes) {
                throw ValidationError()
            }
        }
        self.bytes = bytes
    }

    private init(validatedBytes bytes: rtc_Bytes) {
        self.bytes = bytes.toData()!
    }

    public static func generate() -> Self {
        var result: Self? = nil
        rtc_calllinks_CallLinkRootKey_generate(&result) { resultOpaquePtr, generatedBytes in
            resultOpaquePtr!.assumingMemoryBound(to: Optional<Self>.self).pointee = Self(validatedBytes: generatedBytes)
        }
        return result!
    }

    public static func generateAdminPasskey() -> Data {
        var result: Data? = nil
        rtc_calllinks_CallLinkRootKey_generateAdminPasskey(&result) { resultOpaquePtr, generatedPasskey in
            resultOpaquePtr!.assumingMemoryBound(to: Optional<Data>.self).pointee = generatedPasskey.toData()
        }
        return result!
    }

    public func deriveRoomId() -> Data {
        var result: Data? = nil
        let errorCStr = bytes.withRtcBytes { bytes in
            rtc_calllinks_CallLinkRootKey_deriveRoomId(bytes, &result) { resultOpaquePtr, roomIdBytes in
                resultOpaquePtr!.assumingMemoryBound(to: Optional<Data>.self).pointee = roomIdBytes.toData()
            }
        }
        if let errorCStr {
            fail(String(cString: errorCStr))
        }
        return result!
    }

    public var description: String {
        var result: String? = nil
        let errorCStr = bytes.withRtcBytes { bytes in
            rtc_calllinks_CallLinkRootKey_toFormattedString(bytes, &result) { resultOpaquePtr, rtcString in
                resultOpaquePtr!.assumingMemoryBound(to: Optional<String>.self).pointee = rtcString.toString()
            }
        }
        if let errorCStr {
            fail(String(cString: errorCStr))
        }
        return result!
    }
}

public struct CallLinkEpoch: CustomStringConvertible {
    public struct ValidationError: Error {}
    public static var SERIALIZED_SIZE = 4;
    
    private var value: UInt32

    public var bytes: Data {
        get {
            withUnsafeBytes(of: value.bigEndian) {
                Data($0)
            }
        }
    }
    
    private init(_ value: UInt32) {
        self.value = value
    }
    
    public init(_ string: String) throws {
        var result: Self? = nil
        rtc_calllinks_CallLinkEpoch_parse(string, &result) { resultOpaquePtr, optional_value in
            resultOpaquePtr!.assumingMemoryBound(to: Optional<Self>.self).pointee = Self.fromOptionalU32(optional_value)
        }
        guard let result else {
            throw ValidationError()
        }
        self = result
    }
    
    public init(_ bytes: Data) throws {
        precondition(bytes.count == 4)
        let value = bytes[0..<4].reduce(0) { accum, byte in
            return (accum << 8) | UInt32(byte)
        }
        self.init(value)
    }
    
    public var description: String {
        var result: String? = nil
        let errorCStr = rtc_calllinks_CallLinkEpoch_toFormattedString(self.value, &result) { resultOpaquePtr, rtcString in
            resultOpaquePtr!.assumingMemoryBound(to: Optional<String>.self).pointee = rtcString.toString()
        }
        if let errorCStr {
            fail(String(cString: errorCStr))
        }
        return result!
    }
    
    // Internal: for FFI use.
    internal static func fromOptionalU32(_ optional_value: rtc_OptionalU32) -> Self? {
        if optional_value.valid {
            CallLinkEpoch(optional_value.value)
        } else {
            nil
        }
    }
    
    // Internal: for FFI use.
    internal static func toOptionalU32(_ epoch: CallLinkEpoch?) -> rtc_OptionalU32 {
        if let epochId = epoch {
            rtc_OptionalU32(value: epochId.value, valid: true)
        } else {
            rtc_OptionalU32(value: 0, valid: false)
        }
    }
}

extension CallLinkEpoch: Equatable {
    public static func == (lhs: CallLinkEpoch, rhs: CallLinkEpoch) -> Bool {
        lhs.value == rhs.value
    }
}

public struct CallLinkState {
    public enum Restrictions {
      case none, adminApproval, unknown

      func toOrdinal() -> Int8 {
        return switch self {
            case .none:
                0
            case .adminApproval:
                1
            default:
                -1
        }
      }
    }

    /// Is never null, but may be empty.
    public var name: String
    public var restrictions: Restrictions
    public var revoked: Bool
    public var expiration: Date
    public var epoch: CallLinkEpoch?

    public init(name: String, restrictions: Restrictions, revoked: Bool, expiration: Date, epoch: CallLinkEpoch?) {
        self.name = name
        self.restrictions = restrictions
        self.revoked = revoked
        self.expiration = expiration
        self.epoch = epoch
    }

    static func fromRtc(_ rtcResponse: rtc_calllinks_CallLinkState) -> Self {
        let name = rtcResponse.name.toString() ?? ""
        let restrictions: Restrictions
        switch rtcResponse.raw_restrictions {
        case 0:
            restrictions = .none
        case 1:
            restrictions = .adminApproval
        default:
            restrictions = .unknown
        }
        let expiration = Date(timeIntervalSince1970: TimeInterval(rtcResponse.expiration_epoch_seconds))
        let epoch = CallLinkEpoch.fromOptionalU32(rtcResponse.epoch)
        return Self(name: name, restrictions: restrictions, revoked: rtcResponse.revoked, expiration: expiration, epoch: epoch)
    }
}
