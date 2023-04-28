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

    func testKeyAccessors() throws {
        let anotherKey = CallLinkRootKey.generate()
        XCTAssertNotEqual(Self.EXAMPLE_KEY.bytes, anotherKey.bytes)

        XCTAssertEqual(Self.EXAMPLE_KEY.deriveRoomId(), Self.EXAMPLE_KEY.deriveRoomId())
        XCTAssertNotEqual(Self.EXAMPLE_KEY.deriveRoomId(), anotherKey.deriveRoomId())
    }

    func testFormatting() throws {
        XCTAssertEqual(String(describing: Self.EXAMPLE_KEY), "bcdf-ghkm-npqr-stxz-bcdf-ghkm-npqr-stxz")
    }

    func testCreateSuccess() throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        let callbackCompleted = expectation(description: "callbackCompleted")
        sfu.createCallLink(sfuUrl: "sfu.example", createCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, adminPasskey: CallLinkRootKey.generateAdminPasskey(), callLinkPublicParams: [4, 5, 6])
            .done { result in
                switch result {
                case .success(let state):
                    XCTAssertEqual(state.expiration.timeIntervalSince1970, Self.EXPIRATION_EPOCH_SECONDS)
                case .failure(let code):
                    XCTFail("unexpected failure: \(code)")
                }
                callbackCompleted.fulfill()
            }

        wait(for: [delegate.sentHttpRequestExpectation], timeout: 1.0)
        XCTAssert(try XCTUnwrap(delegate.sentHttpRequestUrl).starts(with: "sfu.example"))
        XCTAssertEqual(delegate.sentHttpRequestMethod, .put)
        let requestId = try XCTUnwrap(delegate.sentHttpRequestId)
        httpClient.receivedResponse(requestId: requestId, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        waitForExpectations(timeout: 1.0)
    }

    func testCreateFailure() throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        let callbackCompleted = expectation(description: "callbackCompleted")
        sfu.createCallLink(sfuUrl: "sfu.example", createCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, adminPasskey: CallLinkRootKey.generateAdminPasskey(), callLinkPublicParams: [4, 5, 6])
            .done { result in
                switch result {
                case .success(let state):
                    XCTFail("unexpected success: \(state)")
                case .failure(let code):
                    XCTAssertEqual(code, 403)
                }
                callbackCompleted.fulfill()
            }

        wait(for: [delegate.sentHttpRequestExpectation], timeout: 1.0)
        XCTAssert(try XCTUnwrap(delegate.sentHttpRequestUrl).starts(with: "sfu.example"))
        XCTAssertEqual(delegate.sentHttpRequestMethod, .put)
        let requestId = try XCTUnwrap(delegate.sentHttpRequestId)
        httpClient.receivedResponse(requestId: requestId, response: HTTPResponse(statusCode: 403, body: Data()))
        waitForExpectations(timeout: 1.0)
    }

    func testReadSuccess() throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        let callbackCompleted = expectation(description: "callbackCompleted")
        sfu.readCallLink(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY)
            .done { result in
                switch result {
                case .success(let state):
                    XCTAssertEqual(state.expiration.timeIntervalSince1970, Self.EXPIRATION_EPOCH_SECONDS)
                case .failure(let code):
                    XCTFail("unexpected failure: \(code)")
                }
                callbackCompleted.fulfill()
            }

        wait(for: [delegate.sentHttpRequestExpectation], timeout: 1.0)
        XCTAssert(try XCTUnwrap(delegate.sentHttpRequestUrl).starts(with: "sfu.example"))
        XCTAssertEqual(delegate.sentHttpRequestMethod, .get)
        let requestId = try XCTUnwrap(delegate.sentHttpRequestId)
        httpClient.receivedResponse(requestId: requestId, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        waitForExpectations(timeout: 1.0)
    }

    func testReadFailure() throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        let callbackCompleted = expectation(description: "callbackCompleted")
        sfu.readCallLink(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY)
            .done { result in
                switch result {
                case .success(let state):
                    XCTFail("unexpected success: \(state)")
                case .failure(let code):
                    XCTAssertEqual(code, 404)
                }
                callbackCompleted.fulfill()
            }

        wait(for: [delegate.sentHttpRequestExpectation], timeout: 1.0)
        XCTAssert(try XCTUnwrap(delegate.sentHttpRequestUrl).starts(with: "sfu.example"))
        XCTAssertEqual(delegate.sentHttpRequestMethod, .get)
        let requestId = try XCTUnwrap(delegate.sentHttpRequestId)
        httpClient.receivedResponse(requestId: requestId, response: HTTPResponse(statusCode: 404, body: Data()))
        waitForExpectations(timeout: 1.0)
    }

    func testUpdateNameSuccess() throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        let callbackCompleted = expectation(description: "callbackCompleted")
        sfu.updateCallLinkName(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, adminPasskey: CallLinkRootKey.generateAdminPasskey(), newName: "Secret Hideout")
            .done { result in
                switch result {
                case .success(_):
                    // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
                    break
                case .failure(let code):
                    XCTFail("unexpected failure: \(code)")
                }
                callbackCompleted.fulfill()
            }

        wait(for: [delegate.sentHttpRequestExpectation], timeout: 1.0)
        XCTAssert(try XCTUnwrap(delegate.sentHttpRequestUrl).starts(with: "sfu.example"))
        XCTAssertEqual(delegate.sentHttpRequestMethod, .put)
        let requestId = try XCTUnwrap(delegate.sentHttpRequestId)
        httpClient.receivedResponse(requestId: requestId, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        waitForExpectations(timeout: 1.0)
    }

    func testUpdateNameFailure() throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        let callbackCompleted = expectation(description: "callbackCompleted")
        sfu.updateCallLinkName(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, adminPasskey: CallLinkRootKey.generateAdminPasskey(), newName: "Secret Hideout")
            .done { result in
                switch result {
                case .success(let state):
                    XCTFail("unexpected success: \(state)")
                case .failure(let code):
                    XCTAssertEqual(code, 403)
                }
                callbackCompleted.fulfill()
            }

        wait(for: [delegate.sentHttpRequestExpectation], timeout: 1.0)
        XCTAssert(try XCTUnwrap(delegate.sentHttpRequestUrl).starts(with: "sfu.example"))
        XCTAssertEqual(delegate.sentHttpRequestMethod, .put)
        let requestId = try XCTUnwrap(delegate.sentHttpRequestId)
        httpClient.receivedResponse(requestId: requestId, response: HTTPResponse(statusCode: 403, body: Data()))
        waitForExpectations(timeout: 1.0)
    }

    func testUpdateNameEmptySuccess() throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        let callbackCompleted = expectation(description: "callbackCompleted")
        sfu.updateCallLinkName(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, adminPasskey: CallLinkRootKey.generateAdminPasskey(), newName: "")
            .done { result in
                switch result {
                case .success(_):
                    // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
                    break
                case .failure(let code):
                    XCTFail("unexpected failure: \(code)")
                }
                callbackCompleted.fulfill()
            }

        wait(for: [delegate.sentHttpRequestExpectation], timeout: 1.0)
        XCTAssert(try XCTUnwrap(delegate.sentHttpRequestUrl).starts(with: "sfu.example"))
        XCTAssertEqual(delegate.sentHttpRequestMethod, .put)
        let requestId = try XCTUnwrap(delegate.sentHttpRequestId)
        httpClient.receivedResponse(requestId: requestId, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        waitForExpectations(timeout: 1.0)
    }

    func testUpdateRestrictionsSuccess() throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        let callbackCompleted = expectation(description: "callbackCompleted")
        sfu.updateCallLinkRestrictions(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, adminPasskey: CallLinkRootKey.generateAdminPasskey(), restrictions: .adminApproval)
            .done { result in
                switch result {
                case .success(_):
                    // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
                    break
                case .failure(let code):
                    XCTFail("unexpected failure: \(code)")
                }
                callbackCompleted.fulfill()
            }

        wait(for: [delegate.sentHttpRequestExpectation], timeout: 1.0)
        XCTAssert(try XCTUnwrap(delegate.sentHttpRequestUrl).starts(with: "sfu.example"))
        XCTAssertEqual(delegate.sentHttpRequestMethod, .put)
        let requestId = try XCTUnwrap(delegate.sentHttpRequestId)
        httpClient.receivedResponse(requestId: requestId, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        waitForExpectations(timeout: 1.0)
    }


    func testUpdateRevocationSuccess() throws {
        let delegate = TestDelegate()
        let httpClient = HTTPClient(delegate: delegate)
        let sfu = SFUClient(httpClient: httpClient)

        let callbackCompleted = expectation(description: "callbackCompleted")
        sfu.updateCallLinkRevocation(sfuUrl: "sfu.example", authCredentialPresentation: [1, 2, 3], linkRootKey: Self.EXAMPLE_KEY, adminPasskey: CallLinkRootKey.generateAdminPasskey(), revoked: true)
            .done { result in
                switch result {
                case .success(_):
                    // Don't bother checking anything here, since we are mocking the SFU's responses anyway.
                    break
                case .failure(let code):
                    XCTFail("unexpected failure: \(code)")
                }
                callbackCompleted.fulfill()
            }

        wait(for: [delegate.sentHttpRequestExpectation], timeout: 1.0)
        XCTAssert(try XCTUnwrap(delegate.sentHttpRequestUrl).starts(with: "sfu.example"))
        XCTAssertEqual(delegate.sentHttpRequestMethod, .put)
        let requestId = try XCTUnwrap(delegate.sentHttpRequestId)
        httpClient.receivedResponse(requestId: requestId, response: HTTPResponse(statusCode: 200, body: Self.EXAMPLE_STATE_JSON.data(using: .utf8)))
        waitForExpectations(timeout: 1.0)
    }
}
