//
// Copyright 2019-2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

// FFI that allows RingRTC to make HTTP requests through
// a Delegate implemented by the application.

import SignalRingRTC.RingRTC

// Same as rust http::Method.
public enum HTTPMethod: Int32 {
    case get = 0
    case put = 1
    case post = 2
    case delete = 3

    static func fromRtc(_ rtcMethod: Int32) -> Self? {
        return HTTPMethod(rawValue: rtcMethod)
    }
}

// Same as rust http::Request (nicer version of rtc_http_Request)
public struct HTTPRequest {
    public let method: HTTPMethod
    public let url: String
    public let headers: [String: String]
    public let body: Data?

    static func fromRtc(_ rtcRequest: rtc_http_Request) -> Self? {
        guard let method = HTTPMethod.fromRtc(rtcRequest.method) else {
            failDebug("unexpected HTTP request method")
            return nil
        }

        guard let url = rtcRequest.url.toString() else {
            Logger.error("invalid HTTP request URL")
            return nil
        }

        return HTTPRequest(
            method: method,
            url: url,
            headers: rtcRequest.headers.toDictionary(),
            body: rtcRequest.body.toData()
        )
    }
}

extension rtc_http_Headers {
    func asUnsafeBufferPointer() -> UnsafeBufferPointer<rtc_http_Header> {
        return UnsafeBufferPointer(start: self.ptr, count: self.count)
    }

    func toDictionary() -> [String: String] {
        var valueByName: [String: String] = [:]
        for header in self.asUnsafeBufferPointer() {
            guard let name = header.name.toString() else {
                continue
            }
            valueByName[name] = header.value.toString()
        }
        return valueByName
    }
}

// Same as rust http::Response (nicer version of rtc_http_Response)
public struct HTTPResponse {
    public let statusCode: UInt16
    public let body: Data?

    public init(statusCode: UInt16, body: Data?) {
        self.statusCode = statusCode
        self.body = body
    }
}

extension rtc_http_Response {
    // Don't forget to call deallocate()
    static func allocate(from response: HTTPResponse) -> Self {
        return Self(status_code: response.statusCode, body: rtc_Bytes.allocate(from: response.body))
    }

    func deallocate() {
        self.body.deallocate()
    }
}

// Same as rust http::Delegate
// Should be implemented by the application.
public protocol HTTPDelegate: AnyObject {
    // An HTTP request should be sent to the given url.
    // The HTTP response should be returned by calling the HttpClient.receivedResponse(requestId, ...).
    // or HttpClient.requestFailed(requestId) if the request failed to get a response.
    @MainActor
    func sendRequest(requestId: UInt32, request: HTTPRequest)
}

public class HTTPClient {
    private let delegateWrapper: HTTPDelegateWrapper
    // This is owned and must be deleted in deinit()
    let rtcClient: OpaquePointer

    public init(delegate: HTTPDelegate? = nil) {
        self.delegateWrapper = HTTPDelegateWrapper(delegate)
        guard let rtcClient = rtc_http_Client_create(self.delegateWrapper.asRtc()) else {
            fail("unable to create RingRTC HttpClient")
        }

        self.rtcClient = rtcClient
        Logger.debug("object! RingRTC HttpClient created... \(ObjectIdentifier(self))")
    }

    deinit {
        rtc_http_Client_destroy(self.rtcClient)
    }

    public var delegate: HTTPDelegate? {
        get {
            return self.delegateWrapper.delegate
        }
        set(delegate) {
            self.delegateWrapper.delegate = delegate
        }
    }

    @MainActor
    public func receivedResponse(requestId: UInt32, response: HTTPResponse) {
        Logger.debug("HttpClient.receivedResponse")

        let rtcResponse = rtc_http_Response.allocate(from: response)
        defer {
            rtcResponse.deallocate()
        }
        rtc_http_Client_received_response(self.rtcClient, requestId, rtcResponse)
    }

    @MainActor
    public func httpRequestFailed(requestId: UInt32) {
        Logger.debug("httpRequestFailed")

        rtc_http_Client_request_failed(self.rtcClient, requestId)
    }
}

// We wrap the Delegate so we can have a pointer (pointers to Protocols don't seem to work.)
// Plus, it's a convenient place to put ptr conversion methods.
// Plus, we need a weak ref somewhere.
private class HTTPDelegateWrapper {
    // We make this weak to avoid a reference cycle
    // from HTTPClient -> rtc_http_Delegate -> HTTPDelegateWrapper -> HTTPDelegate -> HTTPClient (for callbacks)
    // And it makes it easier to construct an HTTPClient because you can set the delegate later.
    weak var delegate: HTTPDelegate?

    init(_ delegate: HTTPDelegate? = nil) {
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

    func asRtc() -> rtc_http_Delegate {
        return rtc_http_Delegate(
            retained: self.asRetainedPtr(),
            release: { (retained: UnsafeMutableRawPointer?) in
                guard let retained = retained else {
                    return
                }

                _ = HTTPDelegateWrapper.from(retained: retained)
            },
            send_request: { (unretained: UnsafeRawPointer?, requestId: UInt32, rtcRequest: rtc_http_Request) in
                guard let unretained = unretained else {
                    return
                }

                let wrapper = HTTPDelegateWrapper.from(unretained: unretained)
                guard let request = HTTPRequest.fromRtc(rtcRequest) else {
                    return
                }

                Logger.debug("HTTPDelegate.sendRequest")
                Task { @MainActor in
                    Logger.debug("HTTPDelegate.sendRequest (on main.async)")

                    guard let delegate = wrapper.delegate else {
                        // Request came in after SFUClient was deleted
                        return
                    }

                    delegate.sendRequest(requestId: requestId, request: request)
                }
            }
        )
    }
}
