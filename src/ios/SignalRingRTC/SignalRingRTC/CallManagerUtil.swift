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
}

extension Data {
    var uuid: UUID {
        get {
            return UUID(uuid: withUnsafeBytes { $0.load(as: uuid_t.self) } )
        }
    }
}

extension UUID {
    var data: Data {
        return withUnsafeBytes(of: self.uuid, { Data($0) })
    }
}
