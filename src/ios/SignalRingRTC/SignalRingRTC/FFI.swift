//
// Copyright 2019-2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import SignalRingRTC.RingRTC

// Here is the new lite/modularized pattern for FFI for a module called X
// (so far where X is "sfu" or "http")
// 1. A swift XClient in X.swift owns a rust X::Client in x.rs
//    which it interacts with via a pointer to X::Client
//    and C funcs prefixed with "rtc_x_Client".
// 2. A rust X::Client owns an RC to a swift XDelegateWrapper
//    which wraps the XDelegate.  The X::Clients get the pointer
//    to the XDelegateWrapper and C funcs from a struct
//    called rtc_x_Delegate that is passed into rtc_x_Client_create.
//    It uses those to make calls to the XDelegateWrapper
//    which then call the XDelegate.
// 3. The swift XDelegate is implemented by the client application.
// 4. All C module-specific structs are defined in x.rs under the x::ios module
//    and have the prefix "rtx_x_".  Common C structs are defined here.
//
// Sometimes a module doesn't need all these parts, but that's the
// general idea.

extension rtc_OptionalU16 {
    func asUInt16() -> UInt16? {
        if self.valid {
            return self.value
        } else{
            return nil
        }
    }
}

extension rtc_OptionalU32 {
    func asUInt32() -> UInt32? {
        if self.valid {
            return self.value
        } else{
            return nil
        }
    }
}

extension rtc_Bytes {
    static func empty() -> Self {
        return Self(ptr: nil, count: 0)
    }

    // Don't forget to call deallocate()
    static func allocate(from maybe: [UInt8]?) -> Self {
        guard let array = maybe else {
            return Self.empty()
        }

        let ptr = UnsafeMutablePointer<UInt8>.allocate(capacity: array.count)
        ptr.initialize(from: array, count: array.count)
        return Self(ptr: ptr, count: array.count)
    }

    // Don't forget to call deallocate()
    static func allocate(from maybe: Data?) -> Self {
        guard let data = maybe else {
            return Self.empty()
        }

        return Self.allocate(from: Array(data))
    }

    func deallocate() {
        if self.ptr == nil {
            return
        }
        self.ptr.deallocate()
    }

    func toArray() -> [UInt8]? {
        if self.ptr == nil {
            return nil
        }
        return Array(self.asUnsafeBufferPointer())
    }

    func toData() -> Data? {
        if self.ptr == nil {
            return nil
        }
        return Data(self.asUnsafeBufferPointer())
    }

    func asUnsafeBufferPointer() -> UnsafeBufferPointer<UInt8> {
        return UnsafeBufferPointer(start: self.ptr, count: self.count)
    }

    func toUUID() -> UUID? {
        guard let ptr = UnsafeRawPointer(self.ptr),
              self.count >= MemoryLayout<uuid_t>.size else {
            return nil
        }
        return UUID(uuid: ptr.loadUnaligned(as: uuid_t.self))
    }
}

extension ContiguousBytes {
    func withRtcBytes<R>(_ body: (rtc_Bytes) throws -> R) rethrows -> R {
        return try withUnsafeBytes { buffer in
            let bytes = rtc_Bytes(ptr: buffer.baseAddress?.assumingMemoryBound(to: UInt8.self), count: buffer.count)
            return try body(bytes)
        }
    }
}

extension rtc_String {
    static func empty() -> Self {
        return Self(ptr: nil, count: 0)
    }

    // Don't forget to call deallocate()
    static func allocate(from maybe: String?) -> Self {
        guard let string = maybe else {
            return Self.empty()
        }

        let bytes = Array(string.utf8);
        let ptr = UnsafeMutablePointer<UInt8>.allocate(capacity: bytes.count)
        ptr.initialize(from: bytes, count: bytes.count)
        return Self(ptr: ptr, count: bytes.count)
    }

    func deallocate() {
        if self.ptr == nil {
            return
        }
        self.ptr.deallocate()
    }

    func toString() -> String? {
        if self.ptr == nil {
            return nil
        }
        return String(bytes: UnsafeBufferPointer(start: self.ptr, count: self.count), encoding: .utf8)
    }
}

class Requests<T> {
    private var continuationById: [UInt32: CheckedContinuation<T, Never>] = [:]
    private var nextId: UInt32 = 1

    func add(_ continuation: CheckedContinuation<T, Never>) -> UInt32 {
        let id = self.nextId
        self.nextId &+= 1
        self.continuationById[id] = continuation
        return id
    }

    func resolve(id: UInt32, response: T) -> Bool {
        if let continuation = self.continuationById.removeValue(forKey: id) {
            continuation.resume(returning: response)
            return true
        }
        return false
    }
}
