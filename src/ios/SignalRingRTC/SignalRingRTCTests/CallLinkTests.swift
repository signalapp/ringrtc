//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import XCTest
import SignalRingRTC

final class CallLinkTests: XCTestCase {
    private static let EXAMPLE_KEY = try! CallLinkRootKey("bcdf-ghkm-npqr-stxz-bcdf-ghkm-npqr-stxz")
    private static let EXPIRATION_EPOCH_SECONDS: TimeInterval = 4133980800 // 2101-01-01
    private static let EXAMPLE_STATE_JSON = #"{"restrictions": "none","name":"","revoked":false,"expiration":\#(UInt64(EXPIRATION_EPOCH_SECONDS))}"#
    private static let EXAMPLE_EMPTY_JSON = #"{}"#
    private static let EXAMPLE_EPOCH = try! CallLinkEpoch("bcdf-ghkm")
    
    func testKeyAccessors() throws {
        let anotherKey = CallLinkRootKey.generate()
        XCTAssertNotEqual(Self.EXAMPLE_KEY.bytes, anotherKey.bytes)

        XCTAssertEqual(Self.EXAMPLE_KEY.deriveRoomId(), Self.EXAMPLE_KEY.deriveRoomId())
        XCTAssertNotEqual(Self.EXAMPLE_KEY.deriveRoomId(), anotherKey.deriveRoomId())
    }

    func testFormatting() throws {
        XCTAssertEqual(String(describing: Self.EXAMPLE_KEY), "bcdf-ghkm-npqr-stxz-bcdf-ghkm-npqr-stxz")
    }
    
    func testEpochFormatting() throws {
        XCTAssertEqual(String(describing: Self.EXAMPLE_EPOCH), "bcdf-ghkm")
    }

    @MainActor
    func testCreateSuccess() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.createCallLink(sfuUrl: "sfu.example", createCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, adminPasskey: CallLinkRootKey.generateAdminPasskey(), callLinkPublicParams: [4, 5, 6], restrictions: .none)
        switch result {
        case .success(let state):
            XCTAssertEqual(state.expiration.timeIntervalSince1970, Self.EXPIRATION_EPOCH_SECONDS)
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }

    @MainActor
    func testCreateFailure() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 403, body: Data()))
        }

        let result = await sfu.createCallLink(sfuUrl: "sfu.example", createCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, adminPasskey: CallLinkRootKey.generateAdminPasskey(), callLinkPublicParams: [4, 5, 6], restrictions: .adminApproval)
        switch result {
        case .success(let state):
            XCTFail("unexpected success: \(state)")
        case .failure(let code):
            XCTAssertEqual(code, 403)
        }
    }

    @MainActor
    func testReadSuccess() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.readCallLink(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: nil)
        switch result {
        case .success(let state):
            XCTAssertEqual(state.expiration.timeIntervalSince1970, Self.EXPIRATION_EPOCH_SECONDS)
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }
    
    @MainActor
    func testReadSuccessWithEpoch() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.readCallLink(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: Self.EXAMPLE_EPOCH)
        switch result {
        case .success(let state):
            XCTAssertEqual(state.expiration.timeIntervalSince1970, Self.EXPIRATION_EPOCH_SECONDS)
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }

    @MainActor
    func testReadFailure() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: Data()))
        }

        let result = await sfu.readCallLink(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: nil)
        switch result {
        case .success(let state):
            XCTFail("unexpected success: \(state)")
        case .failure(let code):
            XCTAssertEqual(code, 404)
        }
    }
    
    @MainActor
    func testReadFailureWithEpoch() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: Data()))
        }

        let result = await sfu.readCallLink(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: Self.EXAMPLE_EPOCH)
        switch result {
        case .success(let state):
            XCTFail("unexpected success: \(state)")
        case .failure(let code):
            XCTAssertEqual(code, 404)
        }
    }

    @MainActor
    func testUpdateNameSuccess() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.updateCallLinkName(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: nil, adminPasskey: CallLinkRootKey.generateAdminPasskey(), newName: "Secret Hideout")

        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }
    
    @MainActor
    func testUpdateNameSuccessWithEpoch() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.updateCallLinkName(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: Self.EXAMPLE_EPOCH, adminPasskey: CallLinkRootKey.generateAdminPasskey(), newName: "Secret Hideout")

        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }

    @MainActor
    func testUpdateNameFailure() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 403, body: Data()))
        }

        let result = await sfu.updateCallLinkName(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: nil, adminPasskey: CallLinkRootKey.generateAdminPasskey(), newName: "Secret Hideout")
        switch result {
        case .success(let state):
            XCTFail("unexpected success: \(state)")
        case .failure(let code):
            XCTAssertEqual(code, 403)
        }
    }
    
    @MainActor
    func testUpdateNameFailureWithEpoch() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 403, body: Data()))
        }

        let result = await sfu.updateCallLinkName(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: Self.EXAMPLE_EPOCH, adminPasskey: CallLinkRootKey.generateAdminPasskey(), newName: "Secret Hideout")
        switch result {
        case .success(let state):
            XCTFail("unexpected success: \(state)")
        case .failure(let code):
            XCTAssertEqual(code, 403)
        }
    }

    @MainActor
    func testUpdateNameEmptySuccess() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.updateCallLinkName(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: nil, adminPasskey: CallLinkRootKey.generateAdminPasskey(), newName: "")
        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }
    
    @MainActor
    func testUpdateNameEmptySuccessWithEpoch() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.updateCallLinkName(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: Self.EXAMPLE_EPOCH, adminPasskey: CallLinkRootKey.generateAdminPasskey(), newName: "")
        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }

    @MainActor
    func testUpdateRestrictionsSuccess() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.updateCallLinkRestrictions(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: nil, adminPasskey: CallLinkRootKey.generateAdminPasskey(), restrictions: .adminApproval)
        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }
    
    @MainActor
    func testUpdateRestrictionsSuccessWithEpoch() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.updateCallLinkRestrictions(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: Self.EXAMPLE_EPOCH, adminPasskey: CallLinkRootKey.generateAdminPasskey(), restrictions: .adminApproval)
        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }

    @MainActor
    func testDeleteCallLinkSuccess() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .delete)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_EMPTY_JSON.data(using: .utf8)))
        }

        let result = await sfu.deleteCallLink(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: nil, adminPasskey: CallLinkRootKey.generateAdminPasskey())
        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }
    
    @MainActor
    func testDeleteCallLinkSuccessWithEpoch() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .delete)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_EMPTY_JSON.data(using: .utf8)))
        }

        let result = await sfu.deleteCallLink(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: Self.EXAMPLE_EPOCH, adminPasskey: CallLinkRootKey.generateAdminPasskey())
        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }

    @MainActor
    func testPeekNoActiveCall() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: Data()))
        }

        let result = await sfu.peek(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: nil)
        XCTAssertNil(result.errorStatusCode)
        XCTAssertNil(result.peekInfo.eraId)
        XCTAssertEqual(0, result.peekInfo.deviceCountIncludingPendingDevices)
        XCTAssertEqual(0, result.peekInfo.deviceCountExcludingPendingDevices)
    }
    
    @MainActor
    func testPeekNoActiveCallWithEpoch() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: Data()))
        }

        let result = await sfu.peek(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: Self.EXAMPLE_EPOCH)
        XCTAssertNil(result.errorStatusCode)
        XCTAssertNil(result.peekInfo.eraId)
        XCTAssertEqual(0, result.peekInfo.deviceCountIncludingPendingDevices)
        XCTAssertEqual(0, result.peekInfo.deviceCountExcludingPendingDevices)
    }

    @MainActor
    func testPeekExpiredLink() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: #"{"reason":"expired"}"#.data(using: .utf8)))
        }

        let result = await sfu.peek(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: nil)
        XCTAssertEqual(PeekInfo.expiredCallLinkStatus, result.errorStatusCode)
        XCTAssertNil(result.peekInfo.eraId)
        XCTAssertEqual(0, result.peekInfo.deviceCountIncludingPendingDevices)
        XCTAssertEqual(0, result.peekInfo.deviceCountExcludingPendingDevices)
    }
    
    @MainActor
    func testPeekExpiredLinkWithEpoch() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: #"{"reason":"expired"}"#.data(using: .utf8)))
        }

        let result = await sfu.peek(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: Self.EXAMPLE_EPOCH)
        XCTAssertEqual(PeekInfo.expiredCallLinkStatus, result.errorStatusCode)
        XCTAssertNil(result.peekInfo.eraId)
        XCTAssertEqual(0, result.peekInfo.deviceCountIncludingPendingDevices)
        XCTAssertEqual(0, result.peekInfo.deviceCountExcludingPendingDevices)
    }

    @MainActor
    func testPeekInvalidLink() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: #"{"reason":"invalid"}"#.data(using: .utf8)))
        }

        let result = await sfu.peek(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: nil)
        XCTAssertEqual(PeekInfo.invalidCallLinkStatus, result.errorStatusCode)
        XCTAssertNil(result.peekInfo.eraId)
        XCTAssertEqual(0, result.peekInfo.deviceCountIncludingPendingDevices)
        XCTAssertEqual(0, result.peekInfo.deviceCountExcludingPendingDevices)
    }
    
    @MainActor
    func testPeekInvalidLinkWithEpoch() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: #"{"reason":"invalid"}"#.data(using: .utf8)))
        }

        let result = await sfu.peek(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: Self.EXAMPLE_EPOCH)
        XCTAssertEqual(PeekInfo.invalidCallLinkStatus, result.errorStatusCode)
        XCTAssertNil(result.peekInfo.eraId)
        XCTAssertEqual(0, result.peekInfo.deviceCountIncludingPendingDevices)
        XCTAssertEqual(0, result.peekInfo.deviceCountExcludingPendingDevices)
    }

    @MainActor
    func testConnectWithNoResponse() throws {
        let delegate = TestDelegate()
        let callManager = createCallManager(delegate)!
        let call = try XCTUnwrap(callManager.createCallLinkCall(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: nil, adminPasskey: nil, hkdfExtraInfo: Data(), audioLevelsIntervalMillis: nil, videoCaptureController: VideoCaptureController()))
        XCTAssertEqual(call.kind, .callLink)

        let callDelegate = TestGroupCallDelegate()
        call.delegate = callDelegate
        XCTAssert(call.connect())
        delay(interval: 1.0)
        XCTAssertEqual(0, callDelegate.requestMembershipProofCount)
        XCTAssertEqual(0, callDelegate.requestGroupMembersCount)
    }
    
    @MainActor
    func testConnectWithNoResponseWithEpoch() throws {
        let delegate = TestDelegate()
        let callManager = createCallManager(delegate)!
        let call = try XCTUnwrap(callManager.createCallLinkCall(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, epoch: Self.EXAMPLE_EPOCH, adminPasskey: nil, hkdfExtraInfo: Data(), audioLevelsIntervalMillis: nil, videoCaptureController: VideoCaptureController()))
        XCTAssertEqual(call.kind, .callLink)

        let callDelegate = TestGroupCallDelegate()
        call.delegate = callDelegate
        XCTAssert(call.connect())
        delay(interval: 1.0)
        XCTAssertEqual(0, callDelegate.requestMembershipProofCount)
        XCTAssertEqual(0, callDelegate.requestGroupMembersCount)
    }
}
