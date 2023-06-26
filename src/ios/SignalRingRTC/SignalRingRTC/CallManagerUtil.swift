//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
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

    func toUUID() -> UUID? {
        guard let ptr = UnsafeRawPointer(self.bytes),
              self.len >= MemoryLayout<uuid_t>.size else {
            return nil
        }
        return UUID(uuid: ptr.loadUnaligned(as: uuid_t.self))
    }
}

extension UUID {
    var data: Data {
        return withUnsafeBytes(of: self.uuid, { Data($0) })
    }
}
