//
//  Copyright (c) 2020 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC

extension AppByteSlice {
    func asUnsafeBufferPointer() -> UnsafeBufferPointer<UInt8> {
        return UnsafeBufferPointer(start: bytes, count: len)
    }

    func asString(encoding: String.Encoding = String.Encoding.utf8) -> String? {
        return String(bytes: asUnsafeBufferPointer(), encoding: encoding)
    }
}
