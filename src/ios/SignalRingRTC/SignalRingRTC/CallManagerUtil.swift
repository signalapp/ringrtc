//
//  Copyright (c) 2020 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC

extension AppByteSlice {
    func asUnsafeBufferPointer() -> UnsafeBufferPointer<UInt8> {
        return UnsafeBufferPointer(start: bytes, count: len)
    }

    func asString(encoding: String.Encoding = String.Encoding.utf8) -> String? {
        if self.bytes == nil {
            return nil
        }
        return String(bytes: asUnsafeBufferPointer(), encoding: encoding)
    }

    func asBytes() -> [UInt8]? {
        if self.bytes == nil {
            return nil
        }
        return Array(asUnsafeBufferPointer())
    }

    func asData() -> Data? {
        if self.bytes == nil {
            return nil
        }
        return Data(asUnsafeBufferPointer())
    }
}
