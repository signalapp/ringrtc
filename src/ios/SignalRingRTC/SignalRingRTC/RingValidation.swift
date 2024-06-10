//
// Copyright 2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import SignalRingRTC.RingRTC

/// Type of media for call at time of origination.
public enum CallMediaType: Int32 {
    /// Call should start as audio only.
    case audioCall = 0
    /// Call should start as audio/video.
    case videoCall = 1
}

public func isValidOfferMessage(opaque: Data, messageAgeSec: UInt64, callMediaType: CallMediaType) -> Bool {
    Logger.debug("")

    return opaque.withUnsafeBytes { buffer in
        ringrtcIsValidOffer(AppByteSlice(bytes: buffer.baseAddress?.assumingMemoryBound(to: UInt8.self),
                                         len: buffer.count),
                            messageAgeSec,
                            callMediaType.rawValue)
    }
}

public func isValidOpaqueRing(opaqueCallMessage: Data,
                              messageAgeSec: UInt64,
                              validateGroupRing: (_ groupId: Data, _ ringId: Int64) -> Bool) -> Bool {
    // Use a pointer to the argument to pass a closure down through a C-based interface;
    // withoutActuallyEscaping promises the compiler we won't persist it.
    // This is different from most RingRTC APIs, which are asynchronous; this one is synchronous and stateless.
    withoutActuallyEscaping(validateGroupRing) { validateGroupRing in
        withUnsafePointer(to: validateGroupRing) { validateGroupRingPtr in
            typealias CallbackType = (Data, Int64) -> Bool
            Logger.debug("")

            return opaqueCallMessage.withUnsafeBytes { buffer in
                let opaqueSlice = AppByteSlice(bytes: buffer.baseAddress?.assumingMemoryBound(to: UInt8.self),
                                               len: buffer.count)
                return ringrtcIsCallMessageValidOpaqueRing(opaqueSlice,
                                                           messageAgeSec,
                                                           UnsafeMutableRawPointer(mutating: validateGroupRingPtr)) {
                    (groupId, ringId, context) in
                    let innerValidate = context!.assumingMemoryBound(to: CallbackType.self)
                    return innerValidate.pointee(groupId.asData() ?? Data(), ringId)
                }
            }
        }
    }
}
