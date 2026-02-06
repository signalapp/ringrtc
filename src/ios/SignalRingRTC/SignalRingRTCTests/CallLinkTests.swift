//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import XCTest
import SignalRingRTC

final class CallLinkTests: XCTestCase {
    private static let EXAMPLE_KEY_V0 = try! CallLinkRootKey("bcdf-ghkm-npqr-stxz-bcdf-ghkm-npqr-stxz")
    private static let EXAMPLE_KEY_V1_INVALID = try! CallLinkRootKey("bcdfghkm-npqrstxz-bcdfghkm-npqrstxz-nc-bbbbbbbb")
    private static let EXAMPLE_KEY_V1_VALID = try! CallLinkRootKey("bcdfghkm-npqrstxz-bcdfghkm-npqrstxz-bc-sbspxdpx")
    private static let EXAMPLE_EPOCH = 3234456222
    private static let EXPIRATION_EPOCH_SECONDS: TimeInterval = 4133980800 // 2101-01-01
    private static let EXAMPLE_STATE_JSON = #"{"restrictions": "none","name":"","revoked":false,"expiration":\#(UInt64(EXPIRATION_EPOCH_SECONDS))}"#
    private static let EXAMPLE_STATE_JSON_WITH_EPOCH = #"{"restrictions": "none","name":"","revoked":false,"expiration":\#(UInt64(EXPIRATION_EPOCH_SECONDS)),"epoch":\#(UInt32(EXAMPLE_EPOCH))}"#
    private static let EXAMPLE_EMPTY_JSON = #"{}"#
    private static let EXAMPLE_ENDORSEMENT_PUBLIC_KEY = Data([0x00, 0x56, 0x23, 0xec, 0x30, 0x93, 0x21, 0x42, 0xa8, 0xd0, 0xd7, 0xcf, 0xfa, 0xb1, 0x97, 0x58, 0x00, 0x9e, 0xdb, 0x82, 0x26, 0xd4, 0x9f, 0xab, 0xd3, 0x82, 0xdc, 0xd9, 0x1d, 0x85, 0x09, 0x60, 0x61])

    func testKeyAccessors() throws {
        let anotherKey = CallLinkRootKey.generate()
        XCTAssertNotEqual(Self.EXAMPLE_KEY_V1_INVALID.bytes, anotherKey.bytes)

        XCTAssertEqual(Self.EXAMPLE_KEY_V1_INVALID.deriveRoomId(), Self.EXAMPLE_KEY_V1_INVALID.deriveRoomId())
        XCTAssertNotEqual(Self.EXAMPLE_KEY_V1_INVALID.deriveRoomId(), anotherKey.deriveRoomId())
    }

    func testFormatting() throws {
        XCTAssertEqual(String(describing: Self.EXAMPLE_KEY_V1_INVALID), "bcdfghkm-npqrstxz-bcdfghkm-npqrstxz-nc-bbbbbbbb")
    }

    @MainActor
    func testCreateSuccessV0() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.createCallLink(sfuUrl: "sfu.example", createCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V1_INVALID, adminPasskey: CallLinkRootKey.generateAdminPasskey(), callLinkPublicParams: [4, 5, 6], restrictions: .none)
        switch result {
        case .success(let result):
            XCTAssertEqual(result.expiration.timeIntervalSince1970, Self.EXPIRATION_EPOCH_SECONDS)
            XCTAssertEqual(String(describing: result.rootKey), "bcdf-ghkm-npqr-stxz-bcdf-ghkm-npqr-stxz")
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }
    
    @MainActor
    func testCreateSuccessV1() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON_WITH_EPOCH.data(using: .utf8)))
        }

        let result = await sfu.createCallLink(sfuUrl: "sfu.example", createCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V1_INVALID, adminPasskey: CallLinkRootKey.generateAdminPasskey(), callLinkPublicParams: [4, 5, 6], restrictions: .none)
        switch result {
        case .success(let result):
            XCTAssertEqual(result.expiration.timeIntervalSince1970, Self.EXPIRATION_EPOCH_SECONDS)
            XCTAssertEqual(String(describing: result.rootKey), "bcdfghkm-npqrstxz-bcdfghkm-npqrstxz-bc-sbspxdpx")
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

        let result = await sfu.createCallLink(sfuUrl: "sfu.example", createCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V1_INVALID, adminPasskey: CallLinkRootKey.generateAdminPasskey(), callLinkPublicParams: [4, 5, 6], restrictions: .adminApproval)
        switch result {
        case .success(let state):
            XCTFail("unexpected success: \(state)")
        case .failure(let code):
            XCTAssertEqual(code, 403)
        }
    }

    @MainActor
    func testReadSuccessV0() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.readCallLink(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V0)
        switch result {
        case .success(let state):
            XCTAssertEqual(state.expiration.timeIntervalSince1970, Self.EXPIRATION_EPOCH_SECONDS)
            XCTAssertEqual(String(describing: state.rootKey), String(describing: Self.EXAMPLE_KEY_V0))
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }
    
    @MainActor
    func testReadSuccessV1() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.readCallLink(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V1_VALID)
        switch result {
        case .success(let state):
            XCTAssertEqual(state.expiration.timeIntervalSince1970, Self.EXPIRATION_EPOCH_SECONDS)
            XCTAssertEqual(String(describing: state.rootKey), String(describing: Self.EXAMPLE_KEY_V1_VALID))
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }

    @MainActor
    func testReadFailureV0() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: Data()))
        }

        let result = await sfu.readCallLink(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V0)
        switch result {
        case .success(let state):
            XCTFail("unexpected success: \(state)")
        case .failure(let code):
            XCTAssertEqual(code, 404)
        }
    }
    
    @MainActor
    func testReadFailureV1() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: Data()))
        }

        let result = await sfu.readCallLink(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V1_VALID)
        switch result {
        case .success(let state):
            XCTFail("unexpected success: \(state)")
        case .failure(let code):
            XCTAssertEqual(code, 404)
        }
    }

    @MainActor
    func testUpdateNameSuccessV0() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.updateCallLinkName(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V0, adminPasskey: CallLinkRootKey.generateAdminPasskey(), newName: "Secret Hideout")

        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }
    
    @MainActor
    func testUpdateNameSuccessV1() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.updateCallLinkName(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V1_VALID, adminPasskey: CallLinkRootKey.generateAdminPasskey(), newName: "Secret Hideout")

        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }

    @MainActor
    func testUpdateNameFailureV0() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 403, body: Data()))
        }

        let result = await sfu.updateCallLinkName(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V0, adminPasskey: CallLinkRootKey.generateAdminPasskey(), newName: "Secret Hideout")
        switch result {
        case .success(let state):
            XCTFail("unexpected success: \(state)")
        case .failure(let code):
            XCTAssertEqual(code, 403)
        }
    }
    
    @MainActor
    func testUpdateNameFailureV1() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 403, body: Data()))
        }

        let result = await sfu.updateCallLinkName(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V1_VALID,  adminPasskey: CallLinkRootKey.generateAdminPasskey(), newName: "Secret Hideout")
        switch result {
        case .success(let state):
            XCTFail("unexpected success: \(state)")
        case .failure(let code):
            XCTAssertEqual(code, 403)
        }
    }

    @MainActor
    func testUpdateNameEmptySuccessV0() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.updateCallLinkName(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V0, adminPasskey: CallLinkRootKey.generateAdminPasskey(), newName: "")
        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }
    
    @MainActor
    func testUpdateNameEmptySuccessV1() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.updateCallLinkName(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V1_VALID, adminPasskey: CallLinkRootKey.generateAdminPasskey(), newName: "")
        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }

    @MainActor
    func testUpdateRestrictionsSuccessV0() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.updateCallLinkRestrictions(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V0, adminPasskey: CallLinkRootKey.generateAdminPasskey(), restrictions: .adminApproval)
        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }
    
    @MainActor
    func testUpdateRestrictionsSuccessV1() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .put)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        }

        let result = await sfu.updateCallLinkRestrictions(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V1_VALID, adminPasskey: CallLinkRootKey.generateAdminPasskey(), restrictions: .adminApproval)
        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }

    @MainActor
    func testDeleteCallLinkSuccessV0() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .delete)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_EMPTY_JSON.data(using: .utf8)))
        }

        let result = await sfu.deleteCallLink(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V0, adminPasskey: CallLinkRootKey.generateAdminPasskey())
        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }
    
    @MainActor
    func testDeleteCallLinkSuccessV1() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .delete)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_EMPTY_JSON.data(using: .utf8)))
        }

        let result = await sfu.deleteCallLink(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V1_VALID, adminPasskey: CallLinkRootKey.generateAdminPasskey())
        switch result {
        case .success(_):
            // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
            break
        case .failure(let code):
            XCTFail("unexpected failure: \(code)")
        }
    }

    @MainActor
    func testPeekNoActiveCallV0() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: Data()))
        }

        let result = await sfu.peek(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V0)
        XCTAssertNil(result.errorStatusCode)
        XCTAssertNil(result.peekInfo.eraId)
        XCTAssertEqual(0, result.peekInfo.deviceCountIncludingPendingDevices)
        XCTAssertEqual(0, result.peekInfo.deviceCountExcludingPendingDevices)
    }
    
    @MainActor
    func testPeekNoActiveCallV1() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: Data()))
        }

        let result = await sfu.peek(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V1_VALID)
        XCTAssertNil(result.errorStatusCode)
        XCTAssertNil(result.peekInfo.eraId)
        XCTAssertEqual(0, result.peekInfo.deviceCountIncludingPendingDevices)
        XCTAssertEqual(0, result.peekInfo.deviceCountExcludingPendingDevices)
    }

    @MainActor
    func testPeekExpiredLinkV0() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: #"{"reason":"expired"}"#.data(using: .utf8)))
        }

        let result = await sfu.peek(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V0)
        XCTAssertEqual(PeekInfo.expiredCallLinkStatus, result.errorStatusCode)
        XCTAssertNil(result.peekInfo.eraId)
        XCTAssertEqual(0, result.peekInfo.deviceCountIncludingPendingDevices)
        XCTAssertEqual(0, result.peekInfo.deviceCountExcludingPendingDevices)
    }
    
    @MainActor
    func testPeekExpiredLinkV1() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: #"{"reason":"expired"}"#.data(using: .utf8)))
        }

        let result = await sfu.peek(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V1_VALID)
        XCTAssertEqual(PeekInfo.expiredCallLinkStatus, result.errorStatusCode)
        XCTAssertNil(result.peekInfo.eraId)
        XCTAssertEqual(0, result.peekInfo.deviceCountIncludingPendingDevices)
        XCTAssertEqual(0, result.peekInfo.deviceCountExcludingPendingDevices)
    }

    @MainActor
    func testPeekInvalidLinkV0() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: #"{"reason":"invalid"}"#.data(using: .utf8)))
        }

        let result = await sfu.peek(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V0)
        XCTAssertEqual(PeekInfo.invalidCallLinkStatus, result.errorStatusCode)
        XCTAssertNil(result.peekInfo.eraId)
        XCTAssertEqual(0, result.peekInfo.deviceCountIncludingPendingDevices)
        XCTAssertEqual(0, result.peekInfo.deviceCountExcludingPendingDevices)
    }
    
    @MainActor
    func testPeekInvalidLinkV1() async throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        delegate.onSendRequest { id, request in
            XCTAssert(request.url.starts(with: "sfu.example"))
            XCTAssertEqual(request.method, .get)
            httpClient.receivedResponse(requestId: id, response: HTTPResponse(statusCode: 404, body: #"{"reason":"invalid"}"#.data(using: .utf8)))
        }

        let result = await sfu.peek(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V1_INVALID)
        XCTAssertEqual(PeekInfo.invalidCallLinkStatus, result.errorStatusCode)
        XCTAssertNil(result.peekInfo.eraId)
        XCTAssertEqual(0, result.peekInfo.deviceCountIncludingPendingDevices)
        XCTAssertEqual(0, result.peekInfo.deviceCountExcludingPendingDevices)
    }

    @MainActor
    func testConnectWithNoResponseV0() throws {
        let delegate = TestDelegate()
        let callManager = createCallManager(delegate)!
        let call = try XCTUnwrap(callManager.createCallLinkCall(sfuUrl: "sfu.example", endorsementPublicKey: Self.EXAMPLE_ENDORSEMENT_PUBLIC_KEY, authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V1_INVALID, adminPasskey: nil, hkdfExtraInfo: Data(), audioLevelsIntervalMillis: nil, videoCaptureController: VideoCaptureController()))
        XCTAssertEqual(call.kind, .callLink)

        let callDelegate = TestGroupCallDelegate()
        call.delegate = callDelegate
        XCTAssert(call.connect())
        delay(interval: 1.0)
        XCTAssertEqual(0, callDelegate.requestMembershipProofCount)
        XCTAssertEqual(0, callDelegate.requestGroupMembersCount)
    }
    
    @MainActor
    func testConnectWithNoResponseV1() throws {
        let delegate = TestDelegate()
        let callManager = createCallManager(delegate)!
        let call = try XCTUnwrap(callManager.createCallLinkCall(sfuUrl: "sfu.example", endorsementPublicKey: Self.EXAMPLE_ENDORSEMENT_PUBLIC_KEY, authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY_V1_INVALID, adminPasskey: nil, hkdfExtraInfo: Data(), audioLevelsIntervalMillis: nil, videoCaptureController: VideoCaptureController()))
        XCTAssertEqual(call.kind, .callLink)

        let callDelegate = TestGroupCallDelegate()
        call.delegate = callDelegate
        XCTAssert(call.connect())
        delay(interval: 1.0)
        XCTAssertEqual(0, callDelegate.requestMembershipProofCount)
        XCTAssertEqual(0, callDelegate.requestGroupMembersCount)
    }
}
