//
//  Copyright (c) 2020 Open Whisper Systems. All rights reserved.
//

import XCTest
@testable import SignalRingRTC
import WebRTC
import SignalCoreKit

import Nimble

// Simulation of a call data type of context that Call Manager must treat opaquely.
public class OpaqueCallData {
    let value: Int32

    var callId: UInt64?

    // There are three states: normal (false/false), ended (true/false), and concluded (*/true)
    var ended = false

    init(value: Int32) {
        self.value = value

        Logger.debug("object! OpaqueCallData created... \(ObjectIdentifier(self))")
    }

    deinit {
        Logger.debug("object! OpaqueCallData destroyed... \(ObjectIdentifier(self))")
    }
}

extension OpaqueCallData: CallManagerCallReference { }

final class TestDelegate: CallManagerDelegate {
    public typealias CallManagerDelegateCallType = OpaqueCallData

    // Simulate the promise-like async handling of signaling messages.
    private let signalingQueue = DispatchQueue(label: "org.signal.signalingQueue")

    // Setup hooks.
    var doAutomaticProceed = false
    var iceServers: [RTCIceServer] = []
    var useTurnOnly = false
    var deviceList: [UInt32] = []
    var doFailSendOffer = false
    var doFailSendAnswer = false
    var doFailSendIce = false
    var doFailSendHangup = false
    var doFailSendBusy = false

    // Setup invocation records.
    var generalInvocationDetected = false

    var shouldSendOfferInvoked = false
    var shouldSendAnswerInvoked = false
    var shouldSendIceCandidatesInvoked = false
    var shouldSendHangupInvoked: Bool? = false
    var shouldCompareCallsInvoked = false
    var shouldStartRingingLocal = false
    var shouldStartRingingRemote = false
//    var shouldConcludeCallInvoked = false
//    var concludedCallCount = 0

    var startOutgoingCallInvoked = false
    var startIncomingCallInvoked = false
    var eventConnectedInvoked = false
    var eventEndedRemoteHangup = false
    var eventEndedSignalingFailure = false

    // When starting a call, if it was prevented from invoking proceed due to call concluded.
//    var callWasConcludedNoProceed = false

    // For object verification, the value expected in callData (i.e. the remote object).
    var expectedValue: Int32 = 0

    // The most recent callId handled.
    var recentCallId: UInt64 = 0

    var sentOffer: String = ""
    var sentAnswer: String = ""
    var sentIceCandidates: [CallManagerIceCandidate] = []

    var remoteCompareResult: Bool? = .none

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldStartCall call: OpaqueCallData, callId: UInt64, isOutgoing: Bool) {
        Logger.debug("TestDelegate:shouldStartCall")
        generalInvocationDetected = true

        guard call.value == expectedValue else {
            XCTFail("call object not expected")
            return
        }

        recentCallId = callId

        if isOutgoing {
            startOutgoingCallInvoked = true
        } else {
            startIncomingCallInvoked = true
        }

        // Simulate asynchronous handling resulting in proceed.
        if doAutomaticProceed {
            // Do it on a different thread (off the main thread).
            signalingQueue.async {
                // @todo Add ability to simulate failure.
                // @todo Add configurable sleep.
                usleep(100 * 1000)

                // Get back on the main thread.
                DispatchQueue.main.async {
                    Logger.debug("TestDelegate:shouldStartCall - main.async")

//                    // We will only call proceed if we haven't concluded the call.
//                    if !callData.concluded {
                        do {
                            _ = try callManager.proceed(callId: callId, iceServers: self.iceServers, hideIp: self.useTurnOnly, deviceList: self.deviceList)
                        } catch {
                            XCTFail("\(error)")
                        }
//                    } else {
//                        self.callWasConcludedNoProceed = true
//                    }
                }
            }
        }
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, onEvent call: OpaqueCallData, event: CallManagerEvent) {
        Logger.debug("TestDelegate:onEvent")
        generalInvocationDetected = true

        guard call.value == expectedValue else {
            XCTFail("call object not expected")
            return
        }

        switch event {
        case .ringingLocal:
            Logger.debug("TestDelegate:ringingLocal")
            shouldStartRingingLocal = true
            
        case .ringingRemote:
            Logger.debug("TestDelegate:ringingRemote")
            shouldStartRingingRemote = true
            
        case .connectedLocal:
            Logger.debug("TestDelegate:connectedLocal")
        case .connectedRemote:
            Logger.debug("TestDelegate:connectedRemote")
        case .endedLocalHangup:
            Logger.debug("TestDelegate:endedLocalHangup")
        case .endedRemoteHangup:
            Logger.debug("TestDelegate:endedRemoteHangup")
            eventEndedRemoteHangup = true

        case .endedRemoteBusy:
            Logger.debug("TestDelegate:endedRemoteBusy")
        case .endedRemoteGlare:
            Logger.debug("TestDelegate:endedRemoteGlare")
        case .endedTimeout:
            Logger.debug("TestDelegate:endedTimeout")
        case .endedInternalFailure:
            Logger.debug("TestDelegate:endedInternalFailure")
        case .endedSignalingFailure:
            Logger.debug("TestDelegate:endedSignalingFailure")
            eventEndedSignalingFailure = true

        case .endedConnectionFailure:
            Logger.debug("TestDelegate:endedConnectionFailure")
        case .endedDropped:
            Logger.debug("TestDelegate:endedDropped")
        case .remoteVideoEnable:
            Logger.debug("TestDelegate:remoteVideoEnable")
        case .remoteVideoDisable:
            Logger.debug("TestDelegate:remoteVideoDisable")
        case .reconnecting:
            Logger.debug("TestDelegate:reconnecting")
        case .reconnected:
            Logger.debug("TestDelegate:reconnected")
        case .endedReceivedOfferExpired:
            Logger.debug("TestDelegate:endedReceivedOfferExpired")
        case .endedReceivedOfferWhileActive:
            Logger.debug("TestDelegate:endedReceivedOfferWhileActive")
        }
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendOffer callId: UInt64, call: OpaqueCallData, destDevice: UInt32?, sdp: String) {
        Logger.debug("TestDelegate:shouldSendOffer")
        generalInvocationDetected = true

        guard call.value == expectedValue else {
            XCTFail("call object not expected")
            return
        }

        recentCallId = callId
        sentOffer = sdp

        signalingQueue.async {
            Logger.debug("TestDelegate:shouldSendOffer - async")

            // @todo Add ability to simulate failure.
            // @todo Add configurable sleep.
            usleep(150 * 1000)

            DispatchQueue.main.async {
                Logger.debug("TestDelegate:shouldSendOffer - main.async")
                self.shouldSendOfferInvoked = true

                if !self.doFailSendOffer {
                    do {
                        try callManager.signalingMessageDidSend(callId: callId)
                    } catch {
                        // @todo
                    }
                } else {
                    callManager.signalingMessageDidFail(callId: callId)
                }
            }
        }
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendAnswer callId: UInt64, call: OpaqueCallData, destDevice: UInt32?, sdp: String) {
        Logger.debug("TestDelegate:shouldSendAnswer")
        generalInvocationDetected = true

        recentCallId = callId
        sentAnswer = sdp

        signalingQueue.async {
            Logger.debug("TestDelegate:shouldSendAnswer - async")

            // @todo Add ability to simulate failure.
            // @todo Add configurable sleep.
            usleep(150 * 1000)

            DispatchQueue.main.async {
                Logger.debug("TestDelegate:shouldSendAnswer - main.async")
                self.shouldSendAnswerInvoked = true

                if !self.doFailSendAnswer {
                    do {
                        try callManager.signalingMessageDidSend(callId: callId)
                    } catch {
                        // @todo
                    }
                } else {
                    callManager.signalingMessageDidFail(callId: callId)
                }
            }
        }
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendIceCandidates callId: UInt64, call: OpaqueCallData, destDevice: UInt32?, candidates: [CallManagerIceCandidate]) {
        Logger.debug("TestDelegate:shouldSendIceCandidates count: \(candidates.count)")
        generalInvocationDetected = true

        recentCallId = callId
        sentIceCandidates += candidates

        signalingQueue.async {
            Logger.debug("TestDelegate:shouldSendIceCandidates - async")

            // @todo Add ability to simulate failure.
            // @todo Add configurable sleep.
            usleep(150 * 1000)

            DispatchQueue.main.async {
                Logger.debug("TestDelegate:shouldSendIceCandidates - main.async")
                self.shouldSendIceCandidatesInvoked = true

                if !self.doFailSendIce {
                    do {
                        try callManager.signalingMessageDidSend(callId: callId)
                    } catch {
                        // @todo
                    }
                } else {
                    callManager.signalingMessageDidFail(callId: callId)
                }
            }
        }
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendHangup callId: UInt64, call: OpaqueCallData, destDevice: UInt32?) {
        Logger.debug("TestDelegate:shouldSendHangup")
        generalInvocationDetected = true

        recentCallId = callId

        signalingQueue.async {
            Logger.debug("TestDelegate:shouldSendHangup - async")

            // @todo Add ability to simulate failure.
            // @todo Add configurable sleep.
            usleep(150 * 1000)

            DispatchQueue.main.async {
                Logger.debug("TestDelegate:shouldSendHangup - main.async")
                self.shouldSendHangupInvoked = true

                if !self.doFailSendHangup {
                    do {
                        try callManager.signalingMessageDidSend(callId: callId)
                    } catch {
                        // @todo
                    }
                } else {
                    callManager.signalingMessageDidFail(callId: callId)
                }
            }
        }
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendBusy callId: UInt64, call: OpaqueCallData, destDevice: UInt32?) {
        Logger.debug("TestDelegate:shouldSendBusy")
        generalInvocationDetected = true

        recentCallId = callId

        signalingQueue.async {
            Logger.debug("TestDelegate:shouldSendBusy - async")

            // @todo Add ability to simulate failure.
            // @todo Add configurable sleep.
            usleep(150 * 1000)

            DispatchQueue.main.async {
                Logger.debug("TestDelegate:shouldSendBusy - main.async")
                if !self.doFailSendBusy {
                    do {
                        try callManager.signalingMessageDidSend(callId: callId)
                    } catch {
                        // @todo
                    }
                } else {
                    callManager.signalingMessageDidFail(callId: callId)
                }
            }
        }
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldCompareCalls call1: OpaqueCallData, call2: OpaqueCallData) -> Bool {
        Logger.debug("TestDelegate:shouldCompareCalls")
        generalInvocationDetected = true

        shouldCompareCallsInvoked = true

        if call1.value == call2.value {
            remoteCompareResult = true
            return true
        } else {
            remoteCompareResult = false
            return false
        }
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, onUpdateLocalVideoSession call: OpaqueCallData, session: AVCaptureSession?) {
        Logger.debug("TestDelegate:onUpdateLocalVideoSession")
        generalInvocationDetected = true
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, onAddRemoteVideoTrack call: OpaqueCallData, track: RTCVideoTrack) {
        Logger.debug("TestDelegate:onAddRemoteVideoTrack")
        generalInvocationDetected = true
    }
}

class SignalRingRTCTests: XCTestCase {

    override func setUp() {
        // Put setup code here. This method is called before the invocation of each test method in the class.

        // Initialize logging, direct it to the console.
        DDLog.add(DDTTYLogger.sharedInstance)
    }

    override func tearDown() {
        // Put teardown code here. This method is called after the invocation of each test method in the class.
    }

//    func testExample() {
//        // This is an example of a functional test case.
//        // Use XCTAssert and related functions to verify your tests produce the correct results.
//    }
//
//    func testPerformanceExample() {
//        // This is an example of a performance test case.
//        self.measure {
//            // Put the code you want to measure the time of here.
//        }
//    }

    // This function does a simple conversion of an Offer to an Answer.
    func convertOfferToAnswer(offer: String) -> String {

        var answer: String = ""

        answer = offer.replacingOccurrences(of: "actpass", with: "active")

        return answer
    }

    // Helper function to delay, without blocking the main thread.
    func delay(interval: TimeInterval) {
        var timerFlag = false
        Timer.scheduledTimer(withTimeInterval: interval, repeats: false, block: { (_) in
            timerFlag = true
        })
        // Wait for the timer to expire, and give expectation timeout in excess of delay.
        expect(timerFlag).toEventually(equal(true), timeout: interval + 1.0)
    }

    func testCallManagerMinimalLifetime() {
        Logger.debug("Test: Minimal Lifetime...")

        // The Call Manager object itself is fairly lightweight, although its initializer
        // creates the global singleton and associated global logger in the RingRTC
        // Rust object.

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())
        expect(delegate.generalInvocationDetected).to(equal(false))
        callManager = nil

        // Delay the end of the test to give Logger time to catch up.
        delay(interval: 0.1)
    }

    func testCallManagerMinimalLifetimeMulti() {
        Logger.debug("Test: Minimal Lifetime Multiple...")

        // Initialize the Call Manager multiple times to ensure consistent operation.
        // @note The global singleton should not be re-allocated after the first time.
        // The CallManagerLogger and CallManagerGlobal should persist for application life.

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())
        expect(delegate.generalInvocationDetected).to(equal(false))
        callManager = nil

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())
        expect(delegate.generalInvocationDetected).to(equal(false))
        callManager = nil

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())
        expect(delegate.generalInvocationDetected).to(equal(false))
        callManager = nil

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())
        expect(delegate.generalInvocationDetected).to(equal(false))
        callManager = nil

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())
        expect(delegate.generalInvocationDetected).to(equal(false))
        callManager = nil

        // Delay the end of the test to give Logger time to catch up.
        delay(interval: 0.1)
    }

    func testCallManagerShortLife() {
        Logger.debug("Test: Create the Call Manager and close it quickly...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        // Create Call Manager object, which will create a WebRTC factory
        // and the RingRTC Rust Call Manager object(s).
        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())
        expect(delegate.generalInvocationDetected).to(equal(false))

        // Delay to make sure things have time to spool up.
        // @todo Is this necessary? Is it all blocking?
        // @todo We should certainly test without these internal delays also!
        delay(interval: 1.0)

        // We didn't do anything, so there should not have been any notifications.
        expect(delegate.generalInvocationDetected).to(equal(false))

        // Release the Call Manager.
        callManager = nil

        // It should have blocked, so we can move on.

        expect(delegate.generalInvocationDetected).to(equal(false))

        // Delay the end of the test to give Logger time to catch up.
        delay(interval: 1.0)
    }

    func testCallManagerOutgoing() {
        Logger.debug("Test: Outgoing Call...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        do {
            Logger.debug("Test: Invoking call()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegate.expectedValue)

            try callManager?.placeCall(call: call)
        } catch {
            XCTFail("Call Manager call() failed: \(error)")
            return
        }

        expect(delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
        delegate.startOutgoingCallInvoked = false

        let iceServers = [RTCIceServer(urlStrings: ["stun:stun1.l.google.com:19302"])]
        let useTurnOnly = false
        let deviceList: [UInt32] = [1]

        var callId = delegate.recentCallId

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManager?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, deviceList: deviceList)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        // @todo Any other notifications/events we expect?

        expect(delegate.shouldSendOfferInvoked).toEventually(equal(true), timeout: 1)
        delegate.shouldSendOfferInvoked = false

        // We've sent an offer, so we should see some Ice candidates.
        // @todo Update now that we can send Ice candidates before receiving the Answer.

        // Simulate receiving an Answer. We will use the recently sent Offer.
        let answer = self.convertOfferToAnswer(offer: delegate.sentOffer)
        let sourceDevice: UInt32 = 1

        do {
            Logger.debug("Test: Invoking receivedAnswer()...")
            try callManager?.receivedAnswer(sourceDevice: 1, callId: callId, sdp: answer)
        } catch {
            XCTFail("Call Manager receivedAnswer() failed: \(error)")
            return
        }

        // We don't care how many though. No need to reset the flag.
        expect(delegate.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: 1)

        // Delay to see if we can catch all Ice candidates being sent...
        delay(interval: 2.0)

        // Simulate receiving Ice candidates. We will use the recently sent Ice candidates.
        let candidates = delegate.sentIceCandidates
        callId = delegate.recentCallId

        do {
            Logger.debug("Test: Invoking receivedIceCandidates()...")
            try callManager?.receivedIceCandidates(sourceDevice: sourceDevice, callId: callId, candidates: candidates)
        } catch {
            XCTFail("Call Manager receivedIceCandidates() failed: \(error)")
            return
        }

        // Delay for about a second (for now).
        delay(interval: 1.0)

        // Try hanging up...
        do {
            Logger.debug("Test: Invoking hangup()...")
            try callManager?.hangup()
        } catch {
            XCTFail("Call Manager hangup() failed: \(error)")
            return
        }

        // @continue...

        // Delay the end of the test to give Logger time to catch up.
        delay(interval: 5.0)

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")

        // @note We would generally just wait here to see a complete call get connected up...
//        // Delay the end of the test to give Logger time to catch up.
//        delay(interval: 60.0)
    }

    func testCallManagerOutgoingSendOfferFail() {
        Logger.debug("Test: Outgoing Call Send Offer Fail...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        do {
            Logger.debug("Test: Invoking call()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegate.expectedValue)

            try callManager?.placeCall(call: call)
        } catch {
            XCTFail("Call Manager call() failed: \(error)")
            return
        }

        expect(delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
        delegate.startOutgoingCallInvoked = false

        let iceServers = [RTCIceServer(urlStrings: ["stun:stun1.l.google.com:19302"])]
        let useTurnOnly = false
        let deviceList: [UInt32] = [1]

        let callId = delegate.recentCallId

        // Make sure the offer fails to send...
        delegate.doFailSendOffer = true

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManager?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, deviceList: deviceList)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        expect(delegate.shouldSendOfferInvoked).toEventually(equal(true), timeout: 1)

        // We should get the endedSignalingFailure event.
        expect(delegate.eventEndedSignalingFailure).toEventually(equal(true), timeout: 1)

        // We expect to get a hangup, because, the Call Manager doesn't make
        // any assumptions that the offer didn't really actually get out.
        // Just to be sure, it will send the hangup...
        expect(delegate.shouldSendHangupInvoked).toEventually(equal(true), timeout: 1)

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testCallManagerIncoming() {
        Logger.debug("Test: Incoming Call...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        let callId: UInt64 = 1234
        let sourceDevice: UInt32 = 1

        do {
            Logger.debug("Test: Invoking receivedOffer()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegate.expectedValue)

            // Inject current timestamp for now. We assume that Rust will also look
            // at the system clock, but it may be nice to hook that up also to some
            // value injection mechanism.
            let timestamp = UInt64(Date().timeIntervalSince1970 * 1000)

            try callManager?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, sdp: self.audioOffer, timestamp: timestamp)
        } catch {
            XCTFail("Call Manager receivedOffer() failed: \(error)")
            return
        }

        expect(delegate.startIncomingCallInvoked).toEventually(equal(true), timeout: 1)
        delegate.startIncomingCallInvoked = false

        let iceServers = [RTCIceServer(urlStrings: ["stun:stun1.l.google.com:19302"])]
        let useTurnOnly = false
        let deviceList: [UInt32] = [1]

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManager?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, deviceList: deviceList)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        expect(delegate.shouldSendAnswerInvoked).toEventually(equal(true), timeout: 1)
        delegate.shouldSendAnswerInvoked = false

        expect(delegate.recentCallId).to(equal(callId))

        // We've sent an answer, so we should see some Ice Candidates.

        // We don't care how many though. No need to reset the flag.
        expect(delegate.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: 1)

        // Delay to see if we can catch all Ice candidates being sent..
        delay(interval: 2.0)

        // Simulate receiving Ice candidates. We will use the recently sent Ice candidates.
        let candidates = delegate.sentIceCandidates

        do {
            Logger.debug("Test: Invoking receivedIceCandidates()...")
            try callManager?.receivedIceCandidates(sourceDevice: sourceDevice, callId: callId, candidates: candidates)
        } catch {
            XCTFail("Call Manager receivedIceCandidates() failed: \(error)")
            return
        }

        // Delay for a couple seconds to see if we can connect.
        delay(interval: 2.0)

        // Try hanging up, which is essentially a "Decline Call" at this point...
        do {
            Logger.debug("Test: Invoking hangup()...")
            try callManager?.hangup()
        } catch {
            XCTFail("Call Manager hangup() failed: \(error)")
            return
        }

        // @continue...

        // Delay the end of the test to give Logger time to catch up.
        delay(interval: 5.0)

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")

        // @note We would generally just wait here to see a complete call get connected up...
//        // Delay the end of the test to give Logger time to catch up.
//        delay(interval: 60.0)
    }

    func testCallManagerOutgoingMultiHangupMin() {
        Logger.debug("Test: MultiHangup Minimum...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        for _ in 1...10 {
            do {
                Logger.debug("Test: Invoking call()...")

                // Define some CallData for simulation. This is defined in a block
                // so that we validate that it is retained correctly and accessible
                // outside this block.
                let call = OpaqueCallData(value: delegate.expectedValue)

                try callManager?.placeCall(call: call)
            } catch {
                XCTFail("Call Manager call() failed: \(error)")
                return
            }

            // Try hanging up...
            do {
                Logger.debug("Test: Invoking hangup()...")
                try callManager?.hangup()
            } catch {
                XCTFail("Call Manager hangup() failed: \(error)")
                return
            }
        }

        // Add a small delay before closing.
        delay(interval: 0.05)

        // All memory should be freed.
//        expect(delegate.concludedCallCount).to(equal(10))

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testCallManagerOutgoingMultiHangup() {
        Logger.debug("Test: MultiHangup...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        for _ in 1...10 {
            do {
                Logger.debug("Test: Invoking call()...")

                // Define some CallData for simulation. This is defined in a block
                // so that we validate that it is retained correctly and accessible
                // outside this block.
                let call = OpaqueCallData(value: delegate.expectedValue)

                try callManager?.placeCall(call: call)
            } catch {
                XCTFail("Call Manager call() failed: \(error)")
                return
            }

            expect(delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
            delegate.startOutgoingCallInvoked = false

            // Try hanging up...
            do {
                Logger.debug("Test: Invoking hangup()...")
                try callManager?.hangup()
            } catch {
                XCTFail("Call Manager hangup() failed: \(error)")
                return
            }
        }

        // Add a small delay before closing.
        delay(interval: 0.05)

        // All memory should be freed.
//        expect(delegate.concludedCallCount).to(equal(10))

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testCallManagerOutgoingMultiHangupProceed() {
        Logger.debug("Test: MultiHangup with Proceed...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        for _ in 1...10 {
            do {
                Logger.debug("Test: Invoking call()...")

                // Define some CallData for simulation. This is defined in a block
                // so that we validate that it is retained correctly and accessible
                // outside this block.
                let call = OpaqueCallData(value: delegate.expectedValue)

                try callManager?.placeCall(call: call)
            } catch {
                XCTFail("Call Manager call() failed: \(error)")
                return
            }

            expect(delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
            delegate.startOutgoingCallInvoked = false

            let iceServers = [RTCIceServer(urlStrings: ["stun:stun1.l.google.com:19302"])]
            let useTurnOnly = false
            let deviceList: [UInt32] = [1]

            let callId = delegate.recentCallId

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManager?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, deviceList: deviceList)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            // @note We call hangup immediately, but internally no offer went out. Why is hangup going out?

            // Try hanging up...
            do {
                Logger.debug("Test: Invoking hangup()...")
                try callManager?.hangup()
            } catch {
                XCTFail("Call Manager hangup() failed: \(error)")
                return
            }
        }

        Logger.debug("Test: Waiting to end...")

        // Add a small delay before closing.
        delay(interval: 0.1)

        // All memory should be freed.
//        expect(delegate.concludedCallCount).to(equal(10))

        Logger.debug("Test: Now ending...")

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testCallManagerOutgoingMultiHangupProceedOffer() {
        Logger.debug("Test: MultiHangup with Proceed until offer sent...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        for _ in 1...50 {
            do {
                Logger.debug("Test: Invoking call()...")

                // Define some CallData for simulation. This is defined in a block
                // so that we validate that it is retained correctly and accessible
                // outside this block.
                let call = OpaqueCallData(value: delegate.expectedValue)

                try callManager?.placeCall(call: call)
            } catch {
                XCTFail("Call Manager call() failed: \(error)")
                return
            }

            expect(delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
            delegate.startOutgoingCallInvoked = false

            let iceServers = [RTCIceServer(urlStrings: ["stun:stun1.l.google.com:19302"])]
            let useTurnOnly = false
            let deviceList: [UInt32] = [1]

            let callId = delegate.recentCallId

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManager?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, deviceList: deviceList)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegate.shouldSendOfferInvoked).toEventually(equal(true), timeout: 1)
            delegate.shouldSendOfferInvoked = false

            // Try hanging up...
            do {
                Logger.debug("Test: Invoking hangup()...")
                try callManager?.hangup()
            } catch {
                XCTFail("Call Manager hangup() failed: \(error)")
                return
            }
        }

        Logger.debug("Test: Waiting to end...")

        // Add a small delay before closing.
        delay(interval: 0.5)

        // All memory should be freed.
//        expect(delegate.concludedCallCount).to(equal(50))

        Logger.debug("Test: Now ending...")

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testCallManagerIncomingQuickHangupNoDelay() {
        Logger.debug("Test: Incoming Call Offer with quick Hangup No Delay...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        let callId: UInt64 = 1234
        let sourceDevice: UInt32 = 1

        // Setup to simulate proceed automatically.
        delegate.doAutomaticProceed = true
        delegate.iceServers = [RTCIceServer(urlStrings: ["stun:stun1.l.google.com:19302"])]
        delegate.useTurnOnly = false
        delegate.deviceList = [1]

        do {
            Logger.debug("Test: Invoking receivedOffer()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegate.expectedValue)

            // Inject current timestamp for now. We assume that Rust will also look
            // at the system clock, but it may be nice to hook that up also to some
            // value injection mechanism.
            let timestamp = UInt64(Date().timeIntervalSince1970 * 1000)

            try callManager?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, sdp: self.audioOffer, timestamp: timestamp)
        } catch {
            XCTFail("Call Manager receivedOffer() failed: \(error)")
            return
        }

        // In this case, hangup should come in so fast that not even onStartIncomingCall
        // will get a chance to be invoked.
//        delay(interval: 0.00)

        // Say a hangup comes in immediately, because the other end does a quick hangup.
        do {
            Logger.debug("Test: Invoking receivedHangup()...")
            try callManager?.receivedHangup(sourceDevice: sourceDevice, callId: callId)
        } catch {
            XCTFail("Call Manager receivedHangup() failed: \(error)")
            return
        }

        // Wait a half second to see what events were fired.
        delay(interval: 0.5)

        expect(delegate.eventEndedRemoteHangup).to(equal(true))
//        expect(delegate.shouldConcludeCallInvoked).to(equal(true))

        // onStartIncomingCall should NOT be invoked!
        expect(delegate.startOutgoingCallInvoked).notTo(equal(true))

        // All memory should be freed.
//        expect(delegate.concludedCallCount).to(equal(1))

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testCallManagerIncomingQuickHangupMediumDelay() {
        Logger.debug("Test: Incoming Call Offer with quick Hangup Medium Delay...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        let callId: UInt64 = 1234
        let sourceDevice: UInt32 = 1

        // Setup to simulate proceed automatically.
        delegate.doAutomaticProceed = true
        delegate.iceServers = [RTCIceServer(urlStrings: ["stun:stun1.l.google.com:19302"])]
        delegate.useTurnOnly = false
        delegate.deviceList = [1]

        do {
            Logger.debug("Test: Invoking receivedOffer()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegate.expectedValue)

            // Inject current timestamp for now. We assume that Rust will also look
            // at the system clock, but it may be nice to hook that up also to some
            // value injection mechanism.
            let timestamp = UInt64(Date().timeIntervalSince1970 * 1000)

            try callManager?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, sdp: self.audioOffer, timestamp: timestamp)
        } catch {
            XCTFail("Call Manager receivedOffer() failed: \(error)")
            return
        }

        // In this case, hangup will come after the offer could be minimally processed. (50ms)
        delay(interval: 0.05)

        // Say a hangup comes in immediately, because the other end does a quick hangup.
        do {
            Logger.debug("Test: Invoking receivedHangup()...")
            try callManager?.receivedHangup(sourceDevice: sourceDevice, callId: callId)
        } catch {
            XCTFail("Call Manager receivedHangup() failed: \(error)")
            return
        }

        // Wait a half second to see what events were fired.
        delay(interval: 0.5)

        expect(delegate.eventEndedRemoteHangup).to(equal(true))
//        expect(delegate.shouldConcludeCallInvoked).to(equal(true))

        // onStartIncomingCall should be invoked!
        expect(delegate.startOutgoingCallInvoked).to(equal(true))

        // And make sure proceed was not invoked due to call being concluded.
//        expect(delegate.callWasConcludedNoProceed).to(equal(true))

        // All memory should be freed.
//        expect(delegate.concludedCallCount).to(equal(10))

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testCallManagerIncomingQuickHangupLongDelay() {
        Logger.debug("Test: Incoming Call Offer with quick Hangup Long Delay...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        let callId: UInt64 = 1234
        let sourceDevice: UInt32 = 1

        // Setup to simulate proceed automatically.
        delegate.doAutomaticProceed = true
        delegate.iceServers = [RTCIceServer(urlStrings: ["stun:stun1.l.google.com:19302"])]
        delegate.useTurnOnly = false
        delegate.deviceList = [1]

        do {
            Logger.debug("Test: Invoking receivedOffer()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegate.expectedValue)

            // Inject current timestamp for now. We assume that Rust will also look
            // at the system clock, but it may be nice to hook that up also to some
            // value injection mechanism.
            let timestamp = UInt64(Date().timeIntervalSince1970 * 1000)

            try callManager?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, sdp: self.audioOffer, timestamp: timestamp)
        } catch {
            XCTFail("Call Manager receivedOffer() failed: \(error)")
            return
        }

        // In this case, hangup will come after the offer, long after. (250ms)
        delay(interval: 0.25)

        // Say a hangup comes in immediately, because the other end does a quick hangup.
        do {
            Logger.debug("Test: Invoking receivedHangup()...")
            try callManager?.receivedHangup(sourceDevice: sourceDevice, callId: callId)
        } catch {
            XCTFail("Call Manager receivedHangup() failed: \(error)")
            return
        }

        // Wait a half second to see what events were fired.
        delay(interval: 0.5)

        expect(delegate.eventEndedRemoteHangup).to(equal(true))
//        expect(delegate.shouldConcludeCallInvoked).to(equal(true))

        // onStartIncomingCall should be invoked!
        expect(delegate.startOutgoingCallInvoked).to(equal(true))

        // And make sure proceed was not invoked due to call being concluded.
//        expect(delegate.callWasConcludedNoProceed).to(equal(true))

        // All memory should be freed.
//        expect(delegate.concludedCallCount).to(equal(1))

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testCallManagerMultiCall() {
        Logger.debug("Test: MultiCall...")

        let delegateCaller = TestDelegate()
        let delegateCallee = TestDelegate()

        var callManagerCaller: CallManager<OpaqueCallData, TestDelegate>?
        var callManagerCallee: CallManager<OpaqueCallData, TestDelegate>?

        callManagerCaller = CallManager()
        callManagerCaller?.delegate = delegateCaller
        expect(callManagerCaller).toNot(beNil())

        callManagerCallee = CallManager()
        callManagerCallee?.delegate = delegateCallee
        expect(callManagerCallee).toNot(beNil())

        delegateCaller.expectedValue = 1111
        delegateCallee.expectedValue = 2222

        do {
            Logger.debug("Test: Invoking call()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegateCaller.expectedValue)

            try callManagerCaller?.placeCall(call: call)
        } catch {
            XCTFail("Call Manager call() failed: \(error)")
            return
        }

        expect(delegateCaller.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
        delegateCaller.startOutgoingCallInvoked = false

        // For now, these variables will be common to both Call Managers.
        let iceServers = [RTCIceServer(urlStrings: ["stun:stun1.l.google.com:19302"])]
        let useTurnOnly = false
        let deviceList: [UInt32] = [1]

        let callId = delegateCaller.recentCallId

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManagerCaller?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, deviceList: deviceList)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        expect(delegateCaller.shouldSendOfferInvoked).toEventually(equal(true), timeout: 1)
        delegateCaller.shouldSendOfferInvoked = false

        // We've sent an offer, so we should see some Ice candidates.
        // @note Currently, it seems candidates aren't sent until we get an Answer?
        // @todo Change this behavior, but for now, try to send an Answer...

        // We sent the offer! Let's give it to our callee!
        let sourceDevice: UInt32 = 1

        do {
            Logger.debug("Test: Invoking receivedOffer()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegateCallee.expectedValue)

            // Inject current timestamp for now. We assume that Rust will also look
            // at the system clock, but it may be nice to hook that up also to some
            // value injection mechanism.
            let timestamp = UInt64(Date().timeIntervalSince1970 * 1000)

            try callManagerCallee?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, sdp: delegateCaller.sentOffer, timestamp: timestamp)
        } catch {
            XCTFail("Call Manager receivedOffer() failed: \(error)")
            return
        }

        expect(delegateCallee.startIncomingCallInvoked).toEventually(equal(true), timeout: 1)
        delegateCallee.startIncomingCallInvoked = false

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManagerCallee?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, deviceList: deviceList)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        expect(delegateCallee.shouldSendAnswerInvoked).toEventually(equal(true), timeout: 2)
        delegateCallee.shouldSendAnswerInvoked = false

        expect(delegateCallee.recentCallId).to(equal(callId))

        // We have an answer, so give it back to the caller.

        do {
            Logger.debug("Test: Invoking receivedAnswer()...")

            try callManagerCaller?.receivedAnswer(sourceDevice: sourceDevice, callId: callId, sdp: delegateCallee.sentAnswer)
        } catch {
            XCTFail("Call Manager receivedAnswer() failed: \(error)")
            return
        }

        // Delay to see if we can catch all Ice candidates being sent...
        delay(interval: 1.0)

        // We've sent an answer, so we should see some Ice Candidates.
        // We don't care how many though. No need to reset the flag.
        expect(delegateCaller.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: 1)
        expect(delegateCallee.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: 1)

        // Give Ice candidates to one another.

        do {
            Logger.debug("Test: Invoking receivedIceCandidates()...")
            try callManagerCaller?.receivedIceCandidates(sourceDevice: sourceDevice, callId: callId, candidates: delegateCallee.sentIceCandidates)
        } catch {
            XCTFail("Call Manager receivedIceCandidates() failed: \(error)")
            return
        }

        do {
            Logger.debug("Test: Invoking receivedIceCandidates()...")
            try callManagerCallee?.receivedIceCandidates(sourceDevice: sourceDevice, callId: callId, candidates: delegateCaller.sentIceCandidates)
        } catch {
            XCTFail("Call Manager receivedIceCandidates() failed: \(error)")
            return
        }

        // We should get to the ringing state in each client.
        expect(delegateCaller.shouldStartRingingRemote).toEventually(equal(true), timeout: 2)
        expect(delegateCallee.shouldStartRingingLocal).toEventually(equal(true), timeout: 1)

        // Delay the end of the test to give Logger time to catch up.
        delay(interval: 1.0)

        // Release the Call Managers.
        callManagerCaller = nil
        callManagerCallee = nil

        // See what clears up after closing the Call Manager...
        delay(interval: 1.0)

        Logger.debug("Test: Exiting test function...")
    }

    func testCallManagerMultiCallFastIceCheck() {
        Logger.debug("Test: MultiCall check that immediate ICE message is handled...")

        let delegateCaller = TestDelegate()
        let delegateCallee = TestDelegate()

        var callManagerCaller: CallManager<OpaqueCallData, TestDelegate>?
        var callManagerCallee: CallManager<OpaqueCallData, TestDelegate>?

        callManagerCaller = CallManager()
        callManagerCaller?.delegate = delegateCaller
        expect(callManagerCaller).toNot(beNil())

        callManagerCallee = CallManager()
        callManagerCallee?.delegate = delegateCallee
        expect(callManagerCallee).toNot(beNil())

        delegateCaller.expectedValue = 1111
        delegateCallee.expectedValue = 2222

        do {
            Logger.debug("Test: Invoking call()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegateCaller.expectedValue)

            try callManagerCaller?.placeCall(call: call)
        } catch {
            XCTFail("Call Manager call() failed: \(error)")
            return
        }

        expect(delegateCaller.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
        delegateCaller.startOutgoingCallInvoked = false

        // For now, these variables will be common to both Call Managers.
        let iceServers = [RTCIceServer(urlStrings: ["stun:stun1.l.google.com:19302"])]
        let useTurnOnly = false
        let deviceList: [UInt32] = [1]

        let callId = delegateCaller.recentCallId

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManagerCaller?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, deviceList: deviceList)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        // Wait for the offer.
        expect(delegateCaller.shouldSendOfferInvoked).toEventually(equal(true), timeout: 1)

        // Wait for the initial set of ICE candidates.
        expect(delegateCaller.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: 1)

        // We have an Offer and ICE candidates. Simulate them coming in rapid
        // succession, to ensure that the Offer is handled and the ICE candidates
        // aren't dropped.
        let sourceDevice: UInt32 = 1

        do {
            Logger.debug("Test: Invoking receivedOffer() and receivedIceCandidates()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegateCallee.expectedValue)

            // Inject current timestamp for now. We assume that Rust will also look
            // at the system clock, but it may be nice to hook that up also to some
            // value injection mechanism.
            let timestamp = UInt64(Date().timeIntervalSince1970 * 1000)

            try callManagerCallee?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, sdp: delegateCaller.sentOffer, timestamp: timestamp)
            try callManagerCallee?.receivedIceCandidates(sourceDevice: sourceDevice, callId: callId, candidates: delegateCaller.sentIceCandidates)
        } catch {
            XCTFail("Call Manager receivedOffer() failed: \(error)")
            return
        }

        // Continue on with the call to see it get a connection.
        expect(delegateCallee.startIncomingCallInvoked).toEventually(equal(true), timeout: 1)
        delegateCallee.startIncomingCallInvoked = false

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManagerCallee?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, deviceList: deviceList)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        expect(delegateCallee.shouldSendAnswerInvoked).toEventually(equal(true), timeout: 2)
        delegateCallee.shouldSendAnswerInvoked = false

        expect(delegateCallee.recentCallId).to(equal(callId))

        // We have an answer, so give it back to the caller.

        do {
            Logger.debug("Test: Invoking receivedAnswer()...")

            try callManagerCaller?.receivedAnswer(sourceDevice: sourceDevice, callId: callId, sdp: delegateCallee.sentAnswer)
        } catch {
            XCTFail("Call Manager receivedAnswer() failed: \(error)")
            return
        }

        // Delay to see if we can catch all Ice candidates being sent...
        delay(interval: 1.0)

        // We've sent an answer, so we should see some Ice Candidates.
        // We don't care how many though. No need to reset the flag.
        expect(delegateCallee.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: 1)

        // Give Ice candidates to one another.

        do {
            Logger.debug("Test: Invoking receivedIceCandidates()...")
            try callManagerCaller?.receivedIceCandidates(sourceDevice: sourceDevice, callId: callId, candidates: delegateCallee.sentIceCandidates)
        } catch {
            XCTFail("Call Manager receivedIceCandidates() failed: \(error)")
            return
        }

        // We should get to the ringing state in each client.
        expect(delegateCaller.shouldStartRingingRemote).toEventually(equal(true), timeout: 5)
        expect(delegateCallee.shouldStartRingingLocal).toEventually(equal(true), timeout: 1)

        delay(interval: 1.0)
        
        // Release the Call Managers.
        callManagerCaller = nil
        callManagerCallee = nil

        // See what clears up after closing the Call Manager...
        delay(interval: 1.0)

        Logger.debug("Test: Exiting test function...")
    }

    // MARK: - Constants

    let audioOffer =
        "v=0\r\n" +
        "o=- 6814183694769985039 2 IN IP4 127.0.0.1\r\n" +
        "s=-\r\n" +
        "t=0 0\r\n" +
        "a=group:BUNDLE audio data\r\n" +
        "a=msid-semantic: WMS ARDAMS\r\n" +
        "m=audio 9 UDP/TLS/RTP/SAVPF 111 103 104 9 102 0 8 106 105 13 110 112 113 126\r\n" +
        "c=IN IP4 0.0.0.0\r\n" +
        "a=rtcp:9 IN IP4 0.0.0.0\r\n" +
        "a=ice-ufrag:VLSN\r\n" +
        "a=ice-pwd:9i7G0u4UW2NBi+HFScgTi9PF\r\n" +
        "a=ice-options:trickle renomination\r\n" +
        "a=fingerprint:sha-256 71:CB:D2:0B:59:35:DA:C6:E0:DD:B8:86:E0:97:F7:44:C2:8D:ED:D3:C7:75:1D:F2:0C:2D:A7:B0:D9:29:33:95\r\n" +
        "a=setup:actpass\r\n" +
        "a=mid:audio\r\n" +
        "a=extmap:2 http://www.webrtc.org/experiments/rtp-hdrext/abs-send-time\r\n" +
        "a=extmap:3 http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01\r\n" +
        "a=sendrecv\r\n" +
        "a=rtcp-mux\r\n" +
        "a=rtpmap:111 opus/48000/2\r\n" +
        "a=rtcp-fb:111 transport-cc\r\n" +
        "a=fmtp:111 cbr=1;minptime=10;useinbandfec=1\r\n" +
        "a=rtpmap:103 ISAC/16000\r\n" +
        "a=rtpmap:104 ISAC/32000\r\n" +
        "a=rtpmap:9 G722/8000\r\n" +
        "a=rtpmap:102 ILBC/8000\r\n" +
        "a=rtpmap:0 PCMU/8000\r\n" +
        "a=rtpmap:8 PCMA/8000\r\n" +
        "a=rtpmap:106 CN/32000\r\n" +
        "a=rtpmap:105 CN/16000\r\n" +
        "a=rtpmap:13 CN/8000\r\n" +
        "a=rtpmap:110 telephone-event/48000\r\n" +
        "a=rtpmap:112 telephone-event/32000\r\n" +
        "a=rtpmap:113 telephone-event/16000\r\n" +
        "a=rtpmap:126 telephone-event/8000\r\n" +
        "a=ssrc:34648539 cname:r3JjCVJ2BiIklTY6\r\n" +
        "a=ssrc:34648539 msid:ARDAMS cbc05114-13bf-473d-9224-f665b9c5ee84\r\n" +
        "a=ssrc:34648539 mslabel:ARDAMS\r\n" +
        "a=ssrc:34648539 label:cbc05114-13bf-473d-9224-f665b9c5ee84\r\n" +
        "m=application 9 UDP/DTLS/SCTP webrtc-datachannel\r\n" +
        "c=IN IP4 0.0.0.0\r\n" +
        "a=ice-ufrag:VLSN\r\n" +
        "a=ice-pwd:9i7G0u4UW2NBi+HFScgTi9PF\r\n" +
        "a=ice-options:trickle renomination\r\n" +
        "a=fingerprint:sha-256 71:CB:D2:0B:59:35:DA:C6:E0:DD:B8:86:E0:97:F7:44:C2:8D:ED:D3:C7:75:1D:F2:0C:2D:A7:B0:D9:29:33:95\r\n" +
        "a=setup:actpass\r\n" +
        "a=mid:data\r\n" +
        "a=sctp-port:5000\r\n" +
        "a=max-message-size:262144\r\n"
}
