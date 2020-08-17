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
    let value: Int32   // Basic token for validation.
    let remote: Int32  // Remote address/user (not deviceId).

    var callId: UInt64?

    // There are three states: normal (false/false), ended (true/false), and concluded (*/true)
    var ended = false

    init(value: Int32, remote: Int32) {
        self.value = value
        self.remote = remote

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
    var videoCaptureController: VideoCaptureController? = nil
    var iceServers: [RTCIceServer] = []
    var useTurnOnly = false
    var localDevice: UInt32 = 1
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
    var shouldSendHangupNormalInvoked = false
    var shouldSendHangupAcceptedInvoked = false
    var shouldSendHangupDeclinedInvoked = false
    var shouldSendHangupBusyInvoked = false
    var shouldSendHangupNeedPermissionInvoked = false
    var shouldSendBusyInvoked = false
    var shouldCompareCallsInvoked = false
//    var shouldConcludeCallInvoked = false
//    var concludedCallCount = 0

    var startOutgoingCallInvoked = false
    var startIncomingCallInvoked = false
    var eventLocalRingingInvoked = false
    var eventRemoteRingingInvoked = false
    var eventLocalConnectedInvoked = false
    var eventRemoteConnectedInvoked = false
    var eventEndedRemoteHangup = false
    var eventEndedRemoteHangupAccepted = false
    var eventEndedRemoteHangupDeclined = false
    var eventEndedRemoteHangupBusy = false
    var eventEndedRemoteHangupNeedPermission = false
    var eventEndedRemoteBusy = false
    var eventEndedRemoteGlare = false
    var eventEndedSignalingFailure = false
    var eventEndedReceivedOfferWhileActive = false
    var eventEndedIgnoreCallsFromNonMultiringCallers = false

    var eventGeneralEnded = false

    // When starting a call, if it was prevented from invoking proceed due to call concluded.
//    var callWasConcludedNoProceed = false

    // For object verification, the value expected in callData (i.e. the remote object).
    var expectedValue: Int32 = 0

    var messageSendingDelay: useconds_t = 150 * 1000

    // The most recent callId handled.
    var recentCallId: UInt64 = 0
    var recentBusyCallId: UInt64 = 0

    var sentOffer: String?
    var sentOfferOpaque: Data?
    var sentAnswer: String?
    var sentAnswerOpaque: Data?
    var sentIceCandidates: [CallManagerIceCandidate] = []

    var remoteCompareResult: Bool? = .none

    var hangupDeviceId: UInt32?

    // CallManager to send ICE candidates when we get them.
    var callManagerICE: [(callManager: CallManager<OpaqueCallData, TestDelegate>, delegate: TestDelegate, deviceId: UInt32)] = []
    var doAutomaticICE = false

    // This is a state variable, but since everything is run on the same
    // main thread, we don't need any protection.
    var canSendICE = false

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldStartCall call: OpaqueCallData, callId: UInt64, isOutgoing: Bool, callMediaType: CallMediaType) {
        Logger.debug("TestDelegate:shouldStartCall")
        generalInvocationDetected = true

        guard call.value == expectedValue else {
            XCTFail("call object not expected expected: \(expectedValue) actual: \(call.value)")
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

                    guard let videoCaptureController = self.videoCaptureController else {
                        return
                    }
                    
//                    // We will only call proceed if we haven't concluded the call.
//                    if !callData.concluded {
                        do {
                            _ = try callManager.proceed(callId: callId, iceServers: self.iceServers, hideIp: self.useTurnOnly, videoCaptureController: videoCaptureController)
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
            eventLocalRingingInvoked = true

        case .ringingRemote:
            Logger.debug("TestDelegate:ringingRemote")
            eventRemoteRingingInvoked = true

        case .connectedLocal:
            Logger.debug("TestDelegate:connectedLocal")
            eventLocalConnectedInvoked = true

        case .connectedRemote:
            Logger.debug("TestDelegate:connectedRemote")
            eventRemoteConnectedInvoked = true

        case .endedLocalHangup:
            Logger.debug("TestDelegate:endedLocalHangup")
            eventGeneralEnded = true

        case .endedRemoteHangup:
            Logger.debug("TestDelegate:endedRemoteHangup")
            eventGeneralEnded = true
            eventEndedRemoteHangup = true

        case .endedRemoteHangupNeedPermission:
            Logger.debug("TestDelegate:endedRemoteHangupNeedPermission")
            eventGeneralEnded = true
            eventEndedRemoteHangupNeedPermission = true

        case .endedRemoteHangupAccepted:
            Logger.debug("TestDelegate:endedRemoteHangupAccepted")
            eventGeneralEnded = true
            eventEndedRemoteHangupAccepted = true

        case .endedRemoteHangupDeclined:
            Logger.debug("TestDelegate:endedRemoteHangupDeclined")
            eventGeneralEnded = true
            eventEndedRemoteHangupDeclined = true

        case .endedRemoteHangupBusy:
            Logger.debug("TestDelegate:endedRemoteHangupBusy")
            eventGeneralEnded = true
            eventEndedRemoteHangupBusy = true

        case .endedRemoteBusy:
            Logger.debug("TestDelegate:endedRemoteBusy")
            eventGeneralEnded = true
            eventEndedRemoteBusy = true

        case .endedRemoteGlare:
            Logger.debug("TestDelegate:endedRemoteGlare")
            eventGeneralEnded = true
            eventEndedRemoteGlare = true

        case .endedTimeout:
            Logger.debug("TestDelegate:endedTimeout")
            eventGeneralEnded = true

        case .endedInternalFailure:
            Logger.debug("TestDelegate:endedInternalFailure")
            eventGeneralEnded = true

        case .endedSignalingFailure:
            Logger.debug("TestDelegate:endedSignalingFailure")
            eventGeneralEnded = true
            eventEndedSignalingFailure = true

        case .endedConnectionFailure:
            Logger.debug("TestDelegate:endedConnectionFailure")
            eventGeneralEnded = true

        case .endedDropped:
            Logger.debug("TestDelegate:endedDropped")
            eventGeneralEnded = true

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
            eventEndedReceivedOfferWhileActive = true

        case .endedIgnoreCallsFromNonMultiringCallers:
            Logger.debug("TestDelegate:endedIgnoreCallsFromNonMultiringCallers")
            eventEndedIgnoreCallsFromNonMultiringCallers = true
        }
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendOffer callId: UInt64, call: OpaqueCallData, destinationDeviceId: UInt32?, opaque: Data?, sdp: String?, callMediaType: CallMediaType) {
        Logger.debug("TestDelegate:shouldSendOffer")
        generalInvocationDetected = true

        guard call.value == expectedValue else {
            XCTFail("call object not expected")
            return
        }

        recentCallId = callId

        // @todo Create a structure to hold offers by deviceId
        if destinationDeviceId == nil || destinationDeviceId == 1 {
            sentOffer = sdp
            sentOfferOpaque = opaque
        }

        signalingQueue.async {
            Logger.debug("TestDelegate:shouldSendOffer - async")

            // @todo Add ability to simulate failure.
            usleep(self.messageSendingDelay)

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

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendAnswer callId: UInt64, call: OpaqueCallData, destinationDeviceId: UInt32?, opaque: Data?, sdp: String?) {
        Logger.debug("TestDelegate:shouldSendAnswer")
        generalInvocationDetected = true

        recentCallId = callId

        // @todo Create a structure to hold answers by deviceId
        if destinationDeviceId == nil || destinationDeviceId == 1 {
            sentAnswer = sdp
            sentAnswerOpaque = opaque
        }

        signalingQueue.async {
            Logger.debug("TestDelegate:shouldSendAnswer - async")

            // @todo Add ability to simulate failure.
            usleep(self.messageSendingDelay)

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

    func tryToSendIceCandidates(callId: UInt64, destinationDeviceId: UInt32?, candidates: [CallManagerIceCandidate]) {
        if destinationDeviceId != nil {
            Logger.debug("callId: \(callId) destinationDeviceId: \(destinationDeviceId ?? 0) candidates.count: \(candidates.count)")
        } else {
            Logger.debug("callId: \(callId) destinationDeviceId: nil candidates.count: \(candidates.count)")
        }

        // @note We don't really care about destinationDeviceId in our current tests
        // because none of them have multiple listeners, so we'll just simulate the
        // replication for all, ignoring it.

        // Add the new local candidates to our queue.
        sentIceCandidates += candidates

        if sentIceCandidates.count > 0 && canSendICE && self.doAutomaticICE {
            do {
                // Send candidates to all referenced Call Managers (simulate replication).
                for element in self.callManagerICE {
                    Logger.debug("Sending ICE candidates to \(element.deviceId) from \(self.localDevice)")
                    try element.callManager.receivedIceCandidates(sourceDevice: self.localDevice, callId: callId, candidates: sentIceCandidates)
                }

                // Clear the queue.
                sentIceCandidates = []
            } catch {
                // @todo
            }
        }
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendIceCandidates callId: UInt64, call: OpaqueCallData, destinationDeviceId: UInt32?, candidates: [CallManagerIceCandidate]) {
        Logger.debug("TestDelegate:shouldSendIceCandidates localDevice: \(self.localDevice) destinationDeviceId: \(destinationDeviceId ?? 0) count: \(candidates.count)")
        generalInvocationDetected = true

        recentCallId = callId

        signalingQueue.async {
            Logger.debug("TestDelegate:shouldSendIceCandidates - async")

            // @todo Add ability to simulate failure.
            usleep(self.messageSendingDelay)

            DispatchQueue.main.async {
                Logger.debug("TestDelegate:shouldSendIceCandidates - main.async")
                self.shouldSendIceCandidatesInvoked = true

                self.tryToSendIceCandidates(callId: callId, destinationDeviceId: destinationDeviceId, candidates: candidates)

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

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendHangup callId: UInt64, call: OpaqueCallData, destinationDeviceId: UInt32?, hangupType: HangupType, deviceId: UInt32, useLegacyHangupMessage: Bool) {
        Logger.debug("TestDelegate:shouldSendHangup")
        generalInvocationDetected = true

        recentCallId = callId

        signalingQueue.async {
            Logger.debug("TestDelegate:shouldSendHangup - async")

            // @todo Add ability to simulate failure.
            usleep(self.messageSendingDelay)

            DispatchQueue.main.async {
                Logger.debug("TestDelegate:shouldSendHangup - main.async")
                switch hangupType {
                case .normal:
                    self.shouldSendHangupNormalInvoked = true
                    self.hangupDeviceId = deviceId
                case .accepted:
                    self.shouldSendHangupAcceptedInvoked = true
                    self.hangupDeviceId = deviceId
                case .declined:
                    self.shouldSendHangupDeclinedInvoked = true
                    self.hangupDeviceId = deviceId
                case .busy:
                    self.shouldSendHangupBusyInvoked = true
                    self.hangupDeviceId = deviceId
                case .needPermission:
                    self.shouldSendHangupNeedPermissionInvoked = true
                    self.hangupDeviceId = deviceId
                }

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

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendBusy callId: UInt64, call: OpaqueCallData, destinationDeviceId: UInt32?) {
        Logger.debug("TestDelegate:shouldSendBusy")
        generalInvocationDetected = true

        recentCallId = callId
        recentBusyCallId = callId

        signalingQueue.async {
            Logger.debug("TestDelegate:shouldSendBusy - async")

            // @todo Add ability to simulate failure.
            usleep(self.messageSendingDelay)

            DispatchQueue.main.async {
                Logger.debug("TestDelegate:shouldSendBusy - main.async")
                self.shouldSendBusyInvoked = true

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

        if call1.remote == call2.remote {
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
        DDLog.add(DDOSLogger.sharedInstance)
    }

    override func tearDown() {
        // Put teardown code here. This method is called after the invocation of each test method in the class.
    }

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

    func testMinimalLifetime() {
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

    func testMinimalLifetimeMulti() {
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

    func testShortLife() {
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
        delay(interval: 1.0)

        // We didn't do anything, so there should not have been any notifications.
        expect(delegate.generalInvocationDetected).to(equal(false))

        // Release the Call Manager.
        callManager = nil

        // It should have blocked, so we can move on.

        expect(delegate.generalInvocationDetected).to(equal(false))

        // Delay the end of the test to give Logger time to catch up.
        delay(interval: 0.1)
    }

    func testOutgoing() {
        Logger.debug("Test: Outgoing Call...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        let localDevice: UInt32 = 1
        
        let videoCaptureController = VideoCaptureController()

        do {
            Logger.debug("Test: Invoking call()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegate.expectedValue, remote: delegate.expectedValue)

            try callManager?.placeCall(call: call, callMediaType: .audioCall, localDevice: localDevice)
        } catch {
            XCTFail("Call Manager call() failed: \(error)")
            return
        }

        expect(delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
        delegate.startOutgoingCallInvoked = false

        let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
        let useTurnOnly = false

        var callId = delegate.recentCallId

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManager?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        expect(delegate.shouldSendOfferInvoked).toEventually(equal(true), timeout: 1)
        delegate.shouldSendOfferInvoked = false

        // We've sent an offer, so we should see some Ice candidates.
        // @todo Update now that we can send Ice candidates before receiving the Answer.

        // Simulate receiving an Answer. We will use the recently sent Offer.
        let answer = self.convertOfferToAnswer(offer: delegate.sentOffer ?? "")
        let sourceDevice: UInt32 = 1

        do {
            Logger.debug("Test: Invoking receivedAnswer()...")
            try callManager?.receivedAnswer(sourceDevice: 1, callId: callId, opaque: nil, sdp: answer, remoteSupportsMultiRing: true)
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

        // Delay the end of the test to give Logger time to catch up.
        delay(interval: 0.1)

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testOutgoingSendOfferFail() {
        Logger.debug("Test: Outgoing Call Send Offer Fail...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        let localDevice: UInt32 = 1

        let videoCaptureController = VideoCaptureController()

        do {
            Logger.debug("Test: Invoking call()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegate.expectedValue, remote: delegate.expectedValue)

            try callManager?.placeCall(call: call, callMediaType: .audioCall, localDevice: localDevice)
        } catch {
            XCTFail("Call Manager call() failed: \(error)")
            return
        }

        expect(delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
        delegate.startOutgoingCallInvoked = false

        let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
        let useTurnOnly = false

        let callId = delegate.recentCallId

        // Make sure the offer fails to send...
        delegate.doFailSendOffer = true

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManager?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
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
        expect(delegate.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: 1)

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testIncoming() {
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
        let localDevice: UInt32 = 1
        let sourceDevice: UInt32 = 1

        let videoCaptureController = VideoCaptureController()

        do {
            Logger.debug("Test: Invoking receivedOffer()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegate.expectedValue, remote: delegate.expectedValue)

            try callManager?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, opaque: nil, sdp: self.audioOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: localDevice, remoteSupportsMultiRing: true, isLocalDevicePrimary: true)
        } catch {
            XCTFail("Call Manager receivedOffer() failed: \(error)")
            return
        }

        expect(delegate.startIncomingCallInvoked).toEventually(equal(true), timeout: 1)
        delegate.startIncomingCallInvoked = false

        let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
        let useTurnOnly = false

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManager?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
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

        // Try hanging up, which is essentially a "Decline Call" at this point...
        do {
            Logger.debug("Test: Invoking hangup()...")
            try callManager?.hangup()
        } catch {
            XCTFail("Call Manager hangup() failed: \(error)")
            return
        }

        // Delay the end of the test to give Logger time to catch up.
        delay(interval: 0.1)

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

   func testIncomingLegacyOnLinked() {
        Logger.debug("Test: Incoming Call from legacy on linked (non-primary)...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        let callId: UInt64 = 1234
        let localDevice: UInt32 = 1
        let sourceDevice: UInt32 = 1

        do {
            Logger.debug("Test: Invoking receivedOffer()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegate.expectedValue, remote: delegate.expectedValue)

            try callManager?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, opaque: nil, sdp: self.audioOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: localDevice, remoteSupportsMultiRing: false, isLocalDevicePrimary: false)
        } catch {
            XCTFail("Call Manager receivedOffer() failed: \(error)")
            return
        }

        expect(delegate.eventEndedIgnoreCallsFromNonMultiringCallers).toEventually(equal(true), timeout: 1)

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testOutgoingMultiHangupMin() {
        Logger.debug("Test: MultiHangup Minimum...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        let localDevice: UInt32 = 1

        for _ in 1...5 {
            do {
                Logger.debug("Test: Invoking call()...")

                // Define some CallData for simulation. This is defined in a block
                // so that we validate that it is retained correctly and accessible
                // outside this block.
                let call = OpaqueCallData(value: delegate.expectedValue, remote: delegate.expectedValue)

                try callManager?.placeCall(call: call, callMediaType: .audioCall, localDevice: localDevice)
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

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testOutgoingMultiHangup() {
        Logger.debug("Test: MultiHangup...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        let localDevice: UInt32 = 1

        for _ in 1...5 {
            do {
                Logger.debug("Test: Invoking call()...")

                // Define some CallData for simulation. This is defined in a block
                // so that we validate that it is retained correctly and accessible
                // outside this block.
                let call = OpaqueCallData(value: delegate.expectedValue, remote: delegate.expectedValue)

                try callManager?.placeCall(call: call, callMediaType: .audioCall, localDevice: localDevice)
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

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testOutgoingMultiHangupProceed() {
        Logger.debug("Test: MultiHangup with Proceed...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        let localDevice: UInt32 = 1

        let videoCaptureController = VideoCaptureController()

        for _ in 1...1 {
            do {
                Logger.debug("Test: Invoking call()...")

                // Define some CallData for simulation. This is defined in a block
                // so that we validate that it is retained correctly and accessible
                // outside this block.
                let call = OpaqueCallData(value: delegate.expectedValue, remote: delegate.expectedValue)

                try callManager?.placeCall(call: call, callMediaType: .audioCall, localDevice: localDevice)
            } catch {
                XCTFail("Call Manager call() failed: \(error)")
                return
            }

            expect(delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
            delegate.startOutgoingCallInvoked = false

            let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
            let useTurnOnly = false

            let callId = delegate.recentCallId

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManager?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
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

        Logger.debug("Test: Waiting to end...")

        // Add a small delay before closing.
        delay(interval: 0.1)

        // We call hangup immediately, but internally no offer should have gone out.
        // No hangup should have been sent for any of the tests either.
        expect(delegate.shouldSendOfferInvoked).to(equal(false))
        expect(delegate.shouldSendHangupNormalInvoked).to(equal(false))

        Logger.debug("Test: Now ending...")

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testOutgoingMultiHangupProceedOffer() {
        Logger.debug("Test: MultiHangup with Proceed until offer sent...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        let localDevice: UInt32 = 1

        let videoCaptureController = VideoCaptureController()

        for _ in 1...5 {
            do {
                Logger.debug("Test: Invoking call()...")

                // Define some CallData for simulation. This is defined in a block
                // so that we validate that it is retained correctly and accessible
                // outside this block.
                let call = OpaqueCallData(value: delegate.expectedValue, remote: delegate.expectedValue)

                try callManager?.placeCall(call: call, callMediaType: .audioCall, localDevice: localDevice)
            } catch {
                XCTFail("Call Manager call() failed: \(error)")
                return
            }

            expect(delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
            delegate.startOutgoingCallInvoked = false

            let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
            let useTurnOnly = false

            let callId = delegate.recentCallId

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManager?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegate.shouldSendOfferInvoked).toEventually(equal(true), timeout: 2)
            delegate.shouldSendOfferInvoked = false

            // Try hanging up...
            do {
                Logger.debug("Test: Invoking hangup()...")
                try callManager?.hangup()
            } catch {
                XCTFail("Call Manager hangup() failed: \(error)")
                return
            }

            expect(delegate.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: 2)
            delegate.shouldSendHangupNormalInvoked = false
        }

        Logger.debug("Test: Waiting to end...")

        // Add a small delay before closing.
        delay(interval: 0.5)

        Logger.debug("Test: Now ending...")

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testIncomingQuickHangupNoDelay() {
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
        let localDevice: UInt32 = 1
        let sourceDevice: UInt32 = 1

        // Setup to simulate proceed automatically.
        delegate.doAutomaticProceed = true
        let videoCaptureController = VideoCaptureController()
        delegate.videoCaptureController = videoCaptureController
        delegate.iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
        delegate.useTurnOnly = false
        delegate.localDevice = 1

        do {
            Logger.debug("Test: Invoking receivedOffer()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegate.expectedValue, remote: delegate.expectedValue)

            try callManager?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, opaque: nil, sdp: self.audioOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: localDevice, remoteSupportsMultiRing: true, isLocalDevicePrimary: true)
        } catch {
            XCTFail("Call Manager receivedOffer() failed: \(error)")
            return
        }

        // Say a hangup comes in immediately, because the other end does a quick hangup.
        do {
            Logger.debug("Test: Invoking receivedHangup()...")
            try callManager?.receivedHangup(sourceDevice: sourceDevice, callId: callId, hangupType: .normal, deviceId: 0)
        } catch {
            XCTFail("Call Manager receivedHangup() failed: \(error)")
            return
        }

        // Wait a half second to see what events were fired.
        delay(interval: 0.5)

        expect(delegate.eventEndedRemoteHangup).to(equal(true))

        // shouldSendAnswerInvoked should NOT be invoked!
        expect(delegate.shouldSendAnswerInvoked).notTo(equal(true))

        // @todo We should not expect startIncomingCallInvoked to be set.
        // However, currently hangup() is not clobbering it...

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testIncomingQuickHangupWithDelay() {
        Logger.debug("Test: Incoming Call Offer with quick Hangup with Delay...")

        let delegate = TestDelegate()
        var callManager: CallManager<OpaqueCallData, TestDelegate>?

        callManager = CallManager()
        callManager?.delegate = delegate
        expect(callManager).toNot(beNil())

        // For our tests, we will have a token opaque object
        // with the given value:
        delegate.expectedValue = 1111

        let callId: UInt64 = 1234
        let localDevice: UInt32 = 1
        let sourceDevice: UInt32 = 1

        // Setup to simulate proceed automatically.
        delegate.doAutomaticProceed = true
        let videoCaptureController = VideoCaptureController()
        delegate.videoCaptureController = videoCaptureController
        delegate.iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
        delegate.useTurnOnly = false
        delegate.localDevice = 1

        do {
            Logger.debug("Test: Invoking receivedOffer()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegate.expectedValue, remote: delegate.expectedValue)

            try callManager?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, opaque: nil, sdp: self.audioOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: localDevice, remoteSupportsMultiRing: true, isLocalDevicePrimary: true)
        } catch {
            XCTFail("Call Manager receivedOffer() failed: \(error)")
            return
        }

        // Wait a half second to start the call and process an Answer.
        delay(interval: 0.5)

        // Say a hangup comes in immediately, because the other end does a quick hangup.
        do {
            Logger.debug("Test: Invoking receivedHangup()...")
            try callManager?.receivedHangup(sourceDevice: sourceDevice, callId: callId, hangupType: .normal, deviceId: 0)
        } catch {
            XCTFail("Call Manager receivedHangup() failed: \(error)")
            return
        }

        // Wait a half second to see what events were fired.
        delay(interval: 0.5)

        expect(delegate.eventEndedRemoteHangup).to(equal(true))

        // startIncomingCallInvoked should be invoked!
        expect(delegate.startIncomingCallInvoked).to(equal(true))

        // shouldSendAnswerInvoked should be invoked!
        expect(delegate.shouldSendAnswerInvoked).to(equal(true))

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func multiCallTesting(loopIterations: Int, enable_opaque: Bool) {
        Logger.debug("Test: MultiCall...")

        let delegateCaller = TestDelegate()
        var callManagerCaller: CallManager<OpaqueCallData, TestDelegate>?
        callManagerCaller = CallManager()
        callManagerCaller?.delegate = delegateCaller
        expect(callManagerCaller).toNot(beNil())
        delegateCaller.expectedValue = 12345
        let callerAddress: Int32 = 888888
        let callerLocalDevice: UInt32 = 1

        let delegateCallee = TestDelegate()
        var callManagerCallee: CallManager<OpaqueCallData, TestDelegate>?
        callManagerCallee = CallManager()
        callManagerCallee?.delegate = delegateCallee
        expect(callManagerCallee).toNot(beNil())
        delegateCallee.expectedValue = 11111
        let calleeAddress: Int32 = 777777
        let calleeLocalDevice: UInt32 = 1

        // Setup the automatic ICE flow for the call.
        delegateCaller.callManagerICE = [(callManagerCallee!, delegateCallee, 1)]
        delegateCallee.callManagerICE = [(callManagerCaller!, delegateCaller, 1)]
        delegateCaller.doAutomaticICE = true
        delegateCallee.doAutomaticICE = true
        delegateCallee.canSendICE = true  // A callee is safe to send Ice whenever needed.

        // For now, these variables will be common to both Call Managers.
        let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
        let useTurnOnly = false
        let sourceDevice: UInt32 = 1

        let videoCaptureController = VideoCaptureController()

        for _ in 1...loopIterations {
            Logger.debug("Test: Start of test loop...")

            // Reset.
            delegateCaller.canSendICE = false
            delegateCaller.sentIceCandidates = []

            do {
                Logger.debug("Test: Invoking call()...")

                // Define some CallData for simulation. This is defined in a block
                // so that we validate that it is retained correctly and accessible
                // outside this block.
                let call = OpaqueCallData(value: delegateCaller.expectedValue, remote: calleeAddress)

                try callManagerCaller?.placeCall(call: call, callMediaType: .audioCall, localDevice: callerLocalDevice)
            } catch {
                XCTFail("Call Manager call() failed: \(error)")
                return
            }

            expect(delegateCaller.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
            delegateCaller.startOutgoingCallInvoked = false

            // This may not be proper...
            let callId = delegateCaller.recentCallId

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerCaller?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateCaller.shouldSendOfferInvoked).toEventually(equal(true), timeout: 1)
            delegateCaller.shouldSendOfferInvoked = false

            // We sent the offer! Let's give it to our callee!
            do {
                Logger.debug("Test: Invoking receivedOffer()...")

                // Define some CallData for simulation. This is defined in a block
                // so that we validate that it is retained correctly and accessible
                // outside this block.
                let call = OpaqueCallData(value: delegateCallee.expectedValue, remote: callerAddress)

                var opaque: Data? = nil
                if enable_opaque {
                    opaque = delegateCallee.sentOfferOpaque
                }

                try callManagerCallee?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, opaque: opaque, sdp: delegateCaller.sentOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: calleeLocalDevice, remoteSupportsMultiRing: true, isLocalDevicePrimary: true)
            } catch {
                XCTFail("Call Manager receivedOffer() failed: \(error)")
                return
            }

            // We've given the offer to the callee device, let's let ICE flow from caller as well.
            // @note Some ICE may flow starting now.
            Logger.debug("Starting ICE flow for caller...")
            delegateCaller.canSendICE = true
            delegateCaller.tryToSendIceCandidates(callId: callId, destinationDeviceId: nil, candidates: [])

            expect(delegateCallee.startIncomingCallInvoked).toEventually(equal(true), timeout: 1)
            delegateCallee.startIncomingCallInvoked = false

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerCallee?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateCallee.shouldSendAnswerInvoked).toEventually(equal(true), timeout: 1)
            delegateCallee.shouldSendAnswerInvoked = false

            expect(delegateCallee.recentCallId).to(equal(callId))

            // We have an answer, so give it back to the caller.

            do {
                Logger.debug("Test: Invoking receivedAnswer()...")

                var opaque: Data? = nil
                if enable_opaque {
                    opaque = delegateCallee.sentAnswerOpaque
                }

                try callManagerCaller?.receivedAnswer(sourceDevice: sourceDevice, callId: callId, opaque: opaque, sdp: delegateCallee.sentAnswer, remoteSupportsMultiRing: true)
            } catch {
                XCTFail("Call Manager receivedAnswer() failed: \(error)")
                return
            }

            // Should get to ringing.
            expect(delegateCaller.eventRemoteRingingInvoked).toEventually(equal(true), timeout: 2)
            delegateCaller.eventRemoteRingingInvoked = false
            expect(delegateCallee.eventLocalRingingInvoked).toEventually(equal(true), timeout: 1)
            delegateCallee.eventLocalRingingInvoked = false

            // Now we want to hangup the callee and start anew.
            do {
                Logger.debug("Test: Invoking hangup()...")
                _ = try callManagerCaller?.hangup()
            } catch {
                XCTFail("Call Manager hangup() failed: \(error)")
                return
            }

            expect(delegateCaller.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: 1)
            delegateCaller.shouldSendHangupNormalInvoked = false

            do {
                Logger.debug("Test: Invoking receivedHangup()...")
                _ = try callManagerCaller?.receivedHangup(sourceDevice: sourceDevice, callId: callId, hangupType: .normal, deviceId: 0)
            } catch {
                XCTFail("Call Manager hangup() failed: \(error)")
                return
            }

            expect(delegateCallee.eventEndedRemoteHangup).toEventually(equal(true), timeout: 1)
            delegateCallee.eventEndedRemoteHangup = false

            Logger.debug("Test: End of test loop...")
        }

        Logger.debug("Test: Done with test loop...")

        // Delay the end of the test to give Logger time to catch up.
        delay(interval: 1.0)

        // Release the Call Managers (but there still might be references in the delegates!).
        callManagerCaller = nil
        callManagerCallee = nil

        // See what clears up after closing the Call Manager...
        delay(interval: 1.0)

        Logger.debug("Test: Exiting test function...")
    }
    
    func testMultiCall() {
        multiCallTesting(loopIterations: 2, enable_opaque: false)
    }

    func testMultiCallOpaque() {
        multiCallTesting(loopIterations: 1, enable_opaque: true)
    }
    
    func testMultiCallFastIceCheck() {
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

        let callerLocalDevice: UInt32 = 1
        let calleeLocalDevice: UInt32 = 1

        let videoCaptureController = VideoCaptureController()

        do {
            Logger.debug("Test: Invoking call()...")

            // Define some CallData for simulation. This is defined in a block
            // so that we validate that it is retained correctly and accessible
            // outside this block.
            let call = OpaqueCallData(value: delegateCaller.expectedValue, remote: delegateCaller.expectedValue)

            try callManagerCaller?.placeCall(call: call, callMediaType: .audioCall, localDevice: callerLocalDevice)
        } catch {
            XCTFail("Call Manager call() failed: \(error)")
            return
        }

        expect(delegateCaller.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
        delegateCaller.startOutgoingCallInvoked = false

        // For now, these variables will be common to both Call Managers.
        let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
        let useTurnOnly = false

        let callId = delegateCaller.recentCallId

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManagerCaller?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
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
            let call = OpaqueCallData(value: delegateCallee.expectedValue, remote: delegateCallee.expectedValue)

            // Send the ICE candidates right after the offer.
            try callManagerCallee?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, opaque: nil, sdp: delegateCaller.sentOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: calleeLocalDevice, remoteSupportsMultiRing: true, isLocalDevicePrimary: true)
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
            _ = try callManagerCallee?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
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

            try callManagerCaller?.receivedAnswer(sourceDevice: sourceDevice, callId: callId, opaque: nil, sdp: delegateCallee.sentAnswer, remoteSupportsMultiRing: true)
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
        expect(delegateCaller.eventRemoteRingingInvoked).toEventually(equal(true), timeout: 2)
        expect(delegateCallee.eventLocalRingingInvoked).toEventually(equal(true), timeout: 1)

        delay(interval: 1.0)

        // Release the Call Managers.
        callManagerCaller = nil
        callManagerCallee = nil

        // See what clears up after closing the Call Manager...
        delay(interval: 1.0)

        Logger.debug("Test: Exiting test function...")
    }

    enum GlareScenario {
        case beforeProceed
        case afterProceed
    }

    func glareTesting(scenario: GlareScenario) {
        Logger.debug("Test: Testing glare for scenario: \(scenario)...")

        let delegateA = TestDelegate()
        var callManagerA: CallManager<OpaqueCallData, TestDelegate>?
        callManagerA = CallManager()
        callManagerA?.delegate = delegateA
        expect(callManagerA).toNot(beNil())
        delegateA.expectedValue = 12345
        let aAddress: Int32 = 888888

        let delegateB = TestDelegate()
        var callManagerB: CallManager<OpaqueCallData, TestDelegate>?
        callManagerB = CallManager()
        callManagerB?.delegate = delegateB
        expect(callManagerB).toNot(beNil())
        delegateB.expectedValue = 11111
        let bAddress: Int32 = 777777

        // Setup the automatic ICE flow for the call.
        delegateA.callManagerICE = [(callManagerB!, delegateB, 1)]
        delegateB.callManagerICE = [(callManagerA!, delegateA, 1)]
        delegateA.doAutomaticICE = false
        delegateB.doAutomaticICE = false

        // For now, these variables will be common to both Call Managers.
        let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
        let useTurnOnly = false
        let localDevice: UInt32 = 1
        let sourceDevice: UInt32 = 1

        let videoCaptureController = VideoCaptureController()

        // A starts to call B.
        do {
            Logger.debug("Test: A calls B...")
            let call = OpaqueCallData(value: delegateA.expectedValue, remote: bAddress)
            try callManagerA?.placeCall(call: call, callMediaType: .audioCall, localDevice: localDevice)
        } catch {
            XCTFail("Call Manager call() failed: \(error)")
            return
        }

        expect(delegateA.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
        delegateA.startOutgoingCallInvoked = false
        let callIdAtoB = delegateA.recentCallId

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManagerA?.proceed(callId: callIdAtoB, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        expect(delegateA.shouldSendOfferInvoked).toEventually(equal(true), timeout: 1)
        delegateA.shouldSendOfferInvoked = false

        // B starts to call A.
        do {
            Logger.debug("Test:B calls A...")
            let call = OpaqueCallData(value: delegateB.expectedValue, remote: aAddress)
            try callManagerB?.placeCall(call: call, callMediaType: .audioCall, localDevice: localDevice)
        } catch {
            XCTFail("Call Manager call() failed: \(error)")
            return
        }

        expect(delegateB.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
        delegateB.startOutgoingCallInvoked = false
        let callIdBtoA = delegateB.recentCallId

        if scenario == .afterProceed {
            // Proceed on the B side.
            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerB?.proceed(callId: callIdBtoA, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateB.shouldSendOfferInvoked).toEventually(equal(true), timeout: 1)
            delegateB.shouldSendOfferInvoked = false
        }

        // Give the offer from A to B.
        do {
            Logger.debug("Test: Invoking B.receivedOffer(A)...")
            let call = OpaqueCallData(value: delegateB.expectedValue, remote: aAddress)
            try callManagerB?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callIdAtoB, opaque: nil, sdp: delegateA.sentOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: localDevice, remoteSupportsMultiRing: true, isLocalDevicePrimary: true)
        } catch {
            XCTFail("Call Manager receivedOffer() failed: \(error)")
            return
        }

        expect(delegateB.eventEndedReceivedOfferWhileActive).toEventually(equal(true), timeout: 1)
        delegateB.eventEndedReceivedOfferWhileActive = false

        expect(delegateB.shouldCompareCallsInvoked).toEventually(equal(true), timeout: 1)
        delegateB.shouldCompareCallsInvoked = false

        // Busy is for the incoming offer.
        expect(delegateB.shouldSendBusyInvoked).toEventually(equal(true), timeout: 1)
        delegateB.shouldSendBusyInvoked = false

        expect(delegateB.eventEndedRemoteGlare).toEventually(equal(true), timeout: 1)
        delegateB.eventEndedRemoteGlare = false

        if scenario == .afterProceed {
            // Hangup is for the outgoing offer.
            expect(delegateB.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: 1)
            delegateB.shouldSendHangupNormalInvoked = false
        }

        // Give the ICE candidates from A to B (they should be ignored).
        expect(delegateA.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: 1)

        do {
            Logger.debug("Test: Invoking B.receivedIceCandidates(A)...")
            try callManagerB?.receivedIceCandidates(sourceDevice: sourceDevice, callId: callIdAtoB, candidates: delegateA.sentIceCandidates)
        } catch {
            XCTFail("Call Manager receivedIceCandidates() failed: \(error)")
            return
        }

        // @todo Is there anything we should confirm here?

        if scenario == .afterProceed {
            // Give the offer from B to A.
            do {
                Logger.debug("Test: Invoking A.receivedOffer(B)...")
                let call = OpaqueCallData(value: delegateA.expectedValue, remote: bAddress)
                try callManagerA?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callIdBtoA, opaque: nil, sdp: delegateB.sentOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: localDevice, remoteSupportsMultiRing: true, isLocalDevicePrimary: true)
            } catch {
                XCTFail("Call Manager receivedOffer() failed: \(error)")
                return
            }

            // Give the ICE candidates from B to A (they should be ignored).
            expect(delegateB.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: 1)

            do {
                Logger.debug("Test: Invoking A.receivedIceCandidates(B)...")
                try callManagerA?.receivedIceCandidates(sourceDevice: sourceDevice, callId: callIdBtoA, candidates: delegateB.sentIceCandidates)
            } catch {
                XCTFail("Call Manager receivedIceCandidates() failed: \(error)")
                return
            }

            expect(delegateA.eventEndedReceivedOfferWhileActive).toEventually(equal(true), timeout: 1)
            delegateA.eventEndedReceivedOfferWhileActive = false

            expect(delegateA.shouldCompareCallsInvoked).toEventually(equal(true), timeout: 1)
            delegateA.shouldCompareCallsInvoked = false

            // Busy is for the incoming offer.
            expect(delegateA.shouldSendBusyInvoked).toEventually(equal(true), timeout: 1)
            delegateA.shouldSendBusyInvoked = false

            expect(delegateA.eventEndedRemoteGlare).toEventually(equal(true), timeout: 1)
            delegateA.eventEndedRemoteGlare = false

            // Hangup is for the outgoing offer.
            expect(delegateA.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: 1)
            delegateA.shouldSendHangupNormalInvoked = false
        }

        // Deliver the Busy from B to A to make sure it is handled correctly.

        do {
            Logger.debug("Test: Invoking A.receivedBusy(B)...")
            try callManagerA?.receivedBusy(sourceDevice: sourceDevice, callId: callIdAtoB)
        } catch {
            XCTFail("Call Manager receivedBusy() failed: \(error)")
            return
        }

        if scenario == .afterProceed {
            // The calls should already be cleaned up on the A side.
            delay(interval: 0.5)
            expect(delegateA.eventEndedRemoteBusy).toNot(equal(true))
        } else {
            expect(delegateA.eventEndedRemoteBusy).toEventually(equal(true), timeout: 1)
            delegateA.eventEndedRemoteBusy = false
        }

        // Deliver the Hangup from B to A to make sure it is handled correctly.

        do {
            Logger.debug("Test: Invoking A.receivedHangup(B)...")
            try callManagerA?.receivedHangup(sourceDevice: sourceDevice, callId: callIdAtoB, hangupType: .normal, deviceId: 0)
        } catch {
            XCTFail("Call Manager receivedHangup() failed: \(error)")
            return
        }

        if scenario == .afterProceed {
            // The calls should already be cleaned up on the A side.
            delay(interval: 0.5)
            expect(delegateA.eventEndedRemoteHangup).toNot(equal(true))
        }

        // Release the Call Managers (but there still might be references in the delegates!).
        callManagerA = nil
        callManagerB = nil

        // See what clears up after closing the Call Manager...
        delay(interval: 1.0)

        Logger.debug("Test: Exiting test function...")
    }

    func testGlareBeforeProceed() {
        glareTesting(scenario: .beforeProceed)
    }

    func testGlareAfterProceed() {
        glareTesting(scenario: .afterProceed)
    }

    enum MultiRingScenario {
        case callerEnds      /// Caller rings multiple callee devices, ends the call, all callees stop ringing.
        case calleeDeclines  /// Caller rings multiple callee devices, one callee declines, all other callees to stop ringing.
        case calleeBusy      /// Caller rings multiple callee devices, one callee is busy with a different peer, all other callees to stop ringing.
        case calleeAccepts   /// Caller rings multiple callee devices, one callee accepts and gets in to call, all other callees stop ringing.
    }

    func multiRingTesting(calleeDeviceCount: Int, loopIterations: Int, scenario: MultiRingScenario) {
        Logger.debug("Test: Testing multi-ring for scenario: \(scenario)...")

        let delegateCaller = TestDelegate()
        var callManagerCaller: CallManager<OpaqueCallData, TestDelegate>? = CallManager()
        callManagerCaller?.delegate = delegateCaller
        expect(callManagerCaller).toNot(beNil())
        delegateCaller.expectedValue = 12345
        let callerAddress: Int32 = 888888
        let callerDevice: UInt32 = 1

        let videoCaptureController = VideoCaptureController()

        // Build the callee structures, the Call Manager and delegate for each.
        var calleeDevices: [(callManager: CallManager<OpaqueCallData, TestDelegate>, delegate: TestDelegate, deviceId: UInt32)] = []
        for i in 1...calleeDeviceCount {
            let callManager: CallManager<OpaqueCallData, TestDelegate> = CallManager()
            let delegate = TestDelegate()

            callManager.delegate = delegate
            delegate.expectedValue = Int32(i * 11111)

            // Setup automatic ICE for the callee.
            delegate.callManagerICE = [(callManagerCaller!, delegateCaller, callerDevice)]
            delegate.doAutomaticICE = true
            delegate.canSendICE = true // A callee is safe to send Ice whenever needed.
            delegate.localDevice = UInt32(i)

            calleeDevices.append((callManager: callManager, delegate: delegate, deviceId: UInt32(i)))
        }
        let calleeAddress: Int32 = 777777

        // Setup automatic ICE for the caller.
        delegateCaller.callManagerICE = calleeDevices
        delegateCaller.doAutomaticICE = true
        delegateCaller.localDevice = callerDevice

        // For now, these variables will be common to both Call Managers.
        let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
        let useTurnOnly = false

        // An extra Call Manger for some scenarions (such as busy).
        let delegateExtra = TestDelegate()
        var callManagerExtra: CallManager<OpaqueCallData, TestDelegate>? = CallManager()
        callManagerExtra?.delegate = delegateExtra
        expect(callManagerExtra).toNot(beNil())
        delegateExtra.expectedValue = 98765
        let extraAddress: Int32 = 666666
        let extraDevice: UInt32 = 1

        // If testing a busy callee...
        let busyCallee = calleeDevices[0]

        // Setup preconditions.
        if scenario == .calleeBusy {
            // In the Busy case, one callee must already be in a call. The first
            // callee will place the call to the extra to get in to a call.

            // Setup ICE for the busy callee, it won't be automatic.
            busyCallee.delegate.callManagerICE = [(callManagerExtra!, delegateExtra, extraDevice)]
            busyCallee.delegate.canSendICE = false

            // Setup automatic ICE for the extra.
            delegateExtra.callManagerICE = [(busyCallee.callManager, busyCallee.delegate, busyCallee.deviceId)]
            delegateExtra.doAutomaticICE = true
            delegateExtra.canSendICE = true // A callee is safe to send Ice whenever needed.
            delegateExtra.localDevice = extraDevice

            do {
                let call = OpaqueCallData(value: busyCallee.delegate.expectedValue, remote: extraAddress)
                try busyCallee.callManager.placeCall(call: call, callMediaType: .audioCall, localDevice: busyCallee.deviceId)
                expect(busyCallee.delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
                busyCallee.delegate.startOutgoingCallInvoked = false

                let callId = busyCallee.delegate.recentCallId
                _ = try busyCallee.callManager.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
                expect(busyCallee.delegate.shouldSendOfferInvoked).toEventually(equal(true), timeout: 1)
                busyCallee.delegate.shouldSendOfferInvoked = false

                let callExtra = OpaqueCallData(value: delegateExtra.expectedValue, remote: calleeAddress)
                try callManagerExtra?.receivedOffer(call: callExtra, sourceDevice: busyCallee.deviceId, callId: callId, opaque: nil, sdp: busyCallee.delegate.sentOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: extraDevice, remoteSupportsMultiRing: true, isLocalDevicePrimary: true)

                expect(busyCallee.delegate.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: 1)
                busyCallee.delegate.canSendICE = true
                busyCallee.delegate.tryToSendIceCandidates(callId: callId, destinationDeviceId: nil, candidates: [])

                expect(delegateExtra.startIncomingCallInvoked).toEventually(equal(true), timeout: 1)
                delegateExtra.startIncomingCallInvoked = false

                try callManagerExtra?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)

                expect(delegateExtra.shouldSendAnswerInvoked).toEventually(equal(true), timeout: 1)
                delegateExtra.shouldSendAnswerInvoked = false
                expect(delegateExtra.recentCallId).to(equal(callId))

                try busyCallee.callManager.receivedAnswer(sourceDevice: extraDevice, callId: callId, opaque: nil, sdp: delegateExtra.sentAnswer, remoteSupportsMultiRing: true)

                expect(delegateExtra.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: 1)

                expect(busyCallee.delegate.eventRemoteRingingInvoked).toEventually(equal(true), timeout: 2)
                expect(delegateExtra.eventLocalRingingInvoked).toEventually(equal(true), timeout: 1)

                try callManagerExtra?.accept(callId: callId)

                // Connected?
                expect(busyCallee.delegate.eventRemoteConnectedInvoked).toEventually(equal(true), timeout: 2)
                expect(delegateExtra.eventLocalConnectedInvoked).toEventually(equal(true), timeout: 1)

                // For fun, we should see a hangup/accepted from the callee here, who was the caller in this case.
                expect(busyCallee.delegate.shouldSendHangupAcceptedInvoked).toEventually(equal(true), timeout: 1)

                // Neither side should have ended the call.
                expect(busyCallee.delegate.eventGeneralEnded).to(equal(false))
                expect(delegateExtra.eventGeneralEnded).to(equal(false))

            } catch {
               XCTFail("Callee setup for busy failed: \(error)")
               return
           }
        }

        for i in 1...loopIterations {
            Logger.debug("Test: Start of test loop \(i)...")

            // Reset.
            delegateCaller.canSendICE = false
            delegateCaller.sentIceCandidates = []

            do {
                Logger.debug("Test: Invoking call()...")

                // Define some CallData for simulation. This is defined in a block
                // so that we validate that it is retained correctly and accessible
                // outside this block.
                let call = OpaqueCallData(value: delegateCaller.expectedValue, remote: calleeAddress)

                try callManagerCaller?.placeCall(call: call, callMediaType: .audioCall, localDevice: callerDevice)
            } catch {
                XCTFail("Call Manager call() failed: \(error)")
                return
            }

            expect(delegateCaller.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
            delegateCaller.startOutgoingCallInvoked = false

            // This may not be proper...
            let callId = delegateCaller.recentCallId

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerCaller?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateCaller.shouldSendOfferInvoked).toEventually(equal(true), timeout: 1)
            delegateCaller.shouldSendOfferInvoked = false

            // We sent the offer! Let's give it to our callees.
            do {
                // Give the offer to all callees at the same time (simulate replication).
                for element in calleeDevices {
                    // Define some CallData for simulation. This is defined in a block
                    // so that we validate that it is retained correctly and accessible
                    // outside this block.
                    let call = OpaqueCallData(value: element.delegate.expectedValue, remote: callerAddress)

                    Logger.debug("Test: Invoking receivedOffer()...")

                    // @note We are specifying multiple devices as primary, but it shouldn't
                    // matter for this type of testing.
                    try element.callManager.receivedOffer(call: call, sourceDevice: callerDevice, callId: callId, opaque: nil, sdp: delegateCaller.sentOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: element.deviceId, remoteSupportsMultiRing: true, isLocalDevicePrimary: true)
                }
            } catch {
                XCTFail("Call Manager receivedOffer() failed: \(error)")
                return
            }

            // We've given the offer to each callee device, let's let ICE flow from caller as well.
            // @note Some ICE may flow starting now.
            Logger.debug("Starting ICE flow for caller...")
            delegateCaller.canSendICE = true
            delegateCaller.tryToSendIceCandidates(callId: callId, destinationDeviceId: nil, candidates: [])

            // Let the callees proceed with the call. We'll ensure they got the start call notification first.
            do {
                for element in calleeDevices {
                    if scenario == .calleeBusy {
                        // Skip the busy callee.
                        if element.deviceId == busyCallee.deviceId {
                            continue
                        }
                    }

                    expect(element.delegate.startIncomingCallInvoked).toEventually(equal(true), timeout: 1)
                    element.delegate.startIncomingCallInvoked = false

                    Logger.debug("Test: Invoking proceed()...")
                    _ = try element.callManager.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
                }
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            // Wait for all callees to send the answer.
            // We will provide them to the caller as we get them (in-order!).
            // @note There might be a more async way to wait for answers from each callee
            // deliver to the caller so that the behavior is more random...
            do {
                for element in calleeDevices {
                    if scenario == .calleeBusy {
                        if element.deviceId == busyCallee.deviceId {
                            // The busy callee should be sending Busy, which we'll give to the caller.
                            // @todo Make another busy type and give it to Caller after ringing...

                            expect(element.delegate.shouldSendBusyInvoked).toEventually(equal(true), timeout: 2)
                            element.delegate.shouldSendBusyInvoked = false

                            expect(element.delegate.recentBusyCallId).to(equal(callId))

                            Logger.debug("Test: Invoking receivedBusy()...")
                            try callManagerCaller?.receivedBusy(sourceDevice: element.deviceId, callId: callId)

                            continue
                        }
                    }

                    expect(element.delegate.shouldSendAnswerInvoked).toEventually(equal(true), timeout: 2)
                    element.delegate.shouldSendAnswerInvoked = false

                    expect(element.delegate.recentCallId).to(equal(callId))

                    Logger.debug("Test: Invoking receivedAnswer()...")
                    try callManagerCaller?.receivedAnswer(sourceDevice: element.deviceId, callId: callId, opaque: nil, sdp: element.delegate.sentAnswer, remoteSupportsMultiRing: true)
                }
            } catch {
                XCTFail("Call Manager receivedAnswer() failed: \(error)")
                return
            }

            if scenario != .calleeBusy {
                // The caller should get to ringing state when the first connection is made with
                // any of the callees.
                expect(delegateCaller.eventRemoteRingingInvoked).toEventually(equal(true), timeout: 2)
                delegateCaller.eventRemoteRingingInvoked = false

                // Now make sure all the callees get to a ringing state.
                for element in calleeDevices {
                    expect(element.delegate.eventLocalRingingInvoked).toEventually(equal(true), timeout: 1)
                    element.delegate.eventLocalRingingInvoked = false
                }
            }

            switch scenario {
            case .callerEnds:
                Logger.debug("Scenario: The caller will cancel the outgoing call.")

                do {
                    Logger.debug("Test: Invoking hangup()...")
                    _ = try callManagerCaller?.hangup()
                } catch {
                    XCTFail("Call Manager hangup() failed: \(error)")
                    return
                }

                expect(delegateCaller.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: 2)
                delegateCaller.shouldSendHangupNormalInvoked = false

                // Since all callees are connected to the caller, the hangup should go
                // over the data channel, so there is no need to send the signaling
                // version, although we could just to make sure the signaling messages
                // get ignored. But not now.

                // Now make sure all the callees get hungup.
                for element in calleeDevices {
                    expect(element.delegate.eventEndedRemoteHangup).toEventually(equal(true), timeout: 2)
                    element.delegate.eventEndedRemoteHangup = false
                }

            case .calleeDeclines:
                Logger.debug("Scenario: The first callee will decline the incoming call.")

                let decliningCallee = calleeDevices[0]

                do {
                    Logger.debug("Test: Invoking hangup(callee)...")
                    _ = try decliningCallee.callManager.hangup()
                } catch {
                    XCTFail("Call Manager hangup(callee) failed: \(error)")
                    return
                }

                // Callee sends normal hangup to the caller.
                expect(decliningCallee.delegate.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: 2)
                decliningCallee.delegate.shouldSendHangupNormalInvoked = false

                // Give the hangup to the caller.
                do {
                    Logger.debug("Test: Invoking hangup(caller)...")
                    _ = try callManagerCaller?.receivedHangup(sourceDevice: decliningCallee.deviceId, callId: callId, hangupType: .normal, deviceId: 0)
                } catch {
                    XCTFail("Call Manager hangup(caller) failed: \(error)")
                    return
                }

                // The caller will send hangup/declined.
                expect(delegateCaller.shouldSendHangupDeclinedInvoked).toEventually(equal(true), timeout: 1)
                delegateCaller.shouldSendHangupDeclinedInvoked = false

                // Since all callees are connected to the caller, the hangup should go
                // over the data channel, so there is no need to send the signaling
                // version, although we could just to make sure the signaling messages
                // get ignored. But not now.

                // Now make sure all the callees get proper hangup indication.
                for element in calleeDevices {
                    // Skip over the declining callee...
                    if element.deviceId != decliningCallee.deviceId {
                        expect(element.delegate.eventEndedRemoteHangupDeclined).toEventually(equal(true), timeout: 1)
                        element.delegate.eventEndedRemoteHangupDeclined = false
                    }
                }

            case .calleeBusy:
                Logger.debug("Scenario: The first callee is busy.")

                // We have given Busy to the Caller and all other devices have given
                // an Answer.

                // Caller should end with remote busy
                expect(delegateCaller.eventEndedRemoteBusy).toEventually(equal(true), timeout: 1)
                delegateCaller.eventEndedRemoteBusy = false

                // Caller should send out a hangup/busy.
                expect(delegateCaller.shouldSendHangupBusyInvoked).toEventually(equal(true), timeout: 1)
                delegateCaller.shouldSendHangupBusyInvoked = false

                do {
                    // Give each callee the hangup/busy.
                    for element in calleeDevices {
                        Logger.debug("Test: Invoking receivedHangup()...")
                        _ = try element.callManager.receivedHangup(sourceDevice: delegateCaller.localDevice, callId: callId, hangupType: .busy, deviceId: delegateCaller.hangupDeviceId ?? 0)
                    }
                } catch {
                    XCTFail("Call Manager receivedHangup() failed: \(error)")
                    return
                }

                // Each callee should end with hangup/busy event, except the one that was busy.
                for element in calleeDevices {
                    if element.deviceId == busyCallee.deviceId {

                        expect(element.delegate.eventEndedReceivedOfferWhileActive).toEventually(equal(true), timeout: 2)
                        element.delegate.eventEndedReceivedOfferWhileActive = false

                        // The busy callee should not have ended their existing call.
                        expect(element.delegate.eventGeneralEnded).to(equal(false))

                        continue
                    }

                    expect(element.delegate.eventEndedRemoteHangupBusy).toEventually(equal(true), timeout: 1)
                    element.delegate.eventEndedRemoteHangupBusy = false
                }

            case .calleeAccepts:
                Logger.debug("Scenario: The first callee accepts the call.")

                let acceptingCallee = calleeDevices[0]

                do {
                    Logger.debug("Test: Invoking accept()...")
                    _ = try acceptingCallee.callManager.accept(callId: callId)
                } catch {
                    XCTFail("Call Manager accept() failed: \(error)")
                    return
                }

                // The connect message would go over the data channel.

                // Both the callee and caller should be in a connected state.
                expect(acceptingCallee.delegate.eventLocalConnectedInvoked).toEventually(equal(true), timeout: 1)
                acceptingCallee.delegate.eventLocalConnectedInvoked = false
                expect(delegateCaller.eventRemoteConnectedInvoked).toEventually(equal(true), timeout: 1)
                delegateCaller.eventRemoteConnectedInvoked = false

                // The caller will send hangup/accepted.
                expect(delegateCaller.shouldSendHangupAcceptedInvoked).toEventually(equal(true), timeout: 1)
                delegateCaller.shouldSendHangupAcceptedInvoked = false

                // Since all callees are connected to the caller, the hangup should go
                // over the data channel, so there is no need to send the signaling
                // version, although we could just to make sure the signaling messages
                // get ignored. But not now.

                // Now make sure all the callees get proper hangup indication.
                for element in calleeDevices {
                    // Skip over the accepting callee...
                    if element.deviceId != acceptingCallee.deviceId {
                        expect(element.delegate.eventEndedRemoteHangupAccepted).toEventually(equal(true), timeout: 1)
                        element.delegate.eventEndedRemoteHangupAccepted = false
                    }
                }

                // Short delay to actually be in a call.
                delay(interval: 0.5)

                do {
                    Logger.debug("Test: Invoking hangup()...")
                    _ = try callManagerCaller?.hangup()
                } catch {
                    XCTFail("Call Manager hangup() failed: \(error)")
                    return
                }

                expect(delegateCaller.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: 1)
                delegateCaller.shouldSendHangupNormalInvoked = false

                // Give the hangup to the callee.
                do {
                    Logger.debug("Test: Invoking hangup(callee)...")
                    _ = try acceptingCallee.callManager.receivedHangup(sourceDevice: callerDevice, callId: callId, hangupType: .normal, deviceId: 0)
                } catch {
                    XCTFail("Call Manager hangup(callee) failed: \(error)")
                    return
                }

                expect(acceptingCallee.delegate.eventEndedRemoteHangup).toEventually(equal(true), timeout: 1)
                acceptingCallee.delegate.eventEndedRemoteHangup = false

                // The other callees would get a hangup, but they are already
                // hungup, so we won't simulate that now.
            }

            Logger.debug("Test: End of test loop...")
        }

        Logger.debug("Test: Done with test loop...")

        // Delay the end of the test to give Logger time to catch up.
        delay(interval: 1.0)

        // Release the Call Managers (but there still might be references in the delegates!).
        callManagerExtra = nil
        callManagerCaller = nil
        calleeDevices = []

        // See what clears up after closing the Call Manager...
        delay(interval: 1.0)

        Logger.debug("Test: Exiting test function...")
    }

    func testMultiRing() {
        multiRingTesting(calleeDeviceCount: 2, loopIterations: 2, scenario: .callerEnds)
    }

    func testMultiRingDeclined() {
        multiRingTesting(calleeDeviceCount: 2, loopIterations: 2, scenario: .calleeDeclines)
    }

    func testMultiRingBusy() {
        multiRingTesting(calleeDeviceCount: 2, loopIterations: 2, scenario: .calleeBusy)
    }

    func testMultiRingAccepted() {
        multiRingTesting(calleeDeviceCount: 2, loopIterations: 2, scenario: .calleeAccepts)
    }

    enum MultiRingGlareScenario {
        case normalGlareOfferBeforeBusy  /// A1 calls B1 and B2; at the same time, B1 calls A1; B1 offer arrives before B1 busy
        case normalGlareBusyBeforeOffer  /// A1 calls B1 and B2; at the same time, B1 calls A1; B1 busy arrives before B1 offer (rare in practice)
        case differentDevice             /// A1 is in call with B1; A2 calls B, should ring on B2
    }

    func multiRingGlareTesting(scenario: MultiRingGlareScenario) {
        Logger.debug("Test: Testing multi-ring glare for scenario: \(scenario)...")

        let aAddress: Int32 = 888888

        let delegateA1 = TestDelegate()
        var callManagerA1: CallManager<OpaqueCallData, TestDelegate>?
        callManagerA1 = CallManager()
        callManagerA1?.delegate = delegateA1
        expect(callManagerA1).toNot(beNil())
        delegateA1.expectedValue = 12345
        let a1Device: UInt32 = 1

        let delegateA2 = TestDelegate()
        var callManagerA2: CallManager<OpaqueCallData, TestDelegate>?
        callManagerA2 = CallManager()
        callManagerA2?.delegate = delegateA2
        expect(callManagerA2).toNot(beNil())
        delegateA2.expectedValue = 54321
        let a2Device: UInt32 = 2

        let bAddress: Int32 = 111111

        let delegateB1 = TestDelegate()
        var callManagerB1: CallManager<OpaqueCallData, TestDelegate>?
        callManagerB1 = CallManager()
        callManagerB1?.delegate = delegateB1
        expect(callManagerB1).toNot(beNil())
        delegateB1.expectedValue = 11111
        let b1Device: UInt32 = 1

        let delegateB2 = TestDelegate()
        var callManagerB2: CallManager<OpaqueCallData, TestDelegate>?
        callManagerB2 = CallManager()
        callManagerB2?.delegate = delegateB2
        expect(callManagerB2).toNot(beNil())
        delegateB2.expectedValue = 22222
        let b2Device: UInt32 = 2

        // For now, these variables will be common to both Call Managers.
        let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
        let useTurnOnly = false

        let videoCaptureController = VideoCaptureController()

        // A1 starts to call B.
        do {
            Logger.debug("Test: A1 calls B...")
            let call = OpaqueCallData(value: delegateA1.expectedValue, remote: bAddress)
            try callManagerA1?.placeCall(call: call, callMediaType: .audioCall, localDevice: a1Device)
        } catch {
            XCTFail("Call Manager call() failed: \(error)")
            return
        }

        expect(delegateA1.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
        delegateA1.startOutgoingCallInvoked = false
        let callIdA1toB = delegateA1.recentCallId

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManagerA1?.proceed(callId: callIdA1toB, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        expect(delegateA1.shouldSendOfferInvoked).toEventually(equal(true), timeout: 1)
        delegateA1.shouldSendOfferInvoked = false

        if scenario == .normalGlareOfferBeforeBusy || scenario == .normalGlareBusyBeforeOffer {
            // @note Not using A2 for this case.

            // B starts to call A.
            do {
                Logger.debug("Test:B calls A...")
                let call = OpaqueCallData(value: delegateB1.expectedValue, remote: aAddress)
                try callManagerB1?.placeCall(call: call, callMediaType: .audioCall, localDevice: b1Device)
            } catch {
                XCTFail("Call Manager call() failed: \(error)")
                return
            }

            expect(delegateB1.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
            delegateB1.startOutgoingCallInvoked = false
            let callIdB1toA = delegateB1.recentCallId

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerB1?.proceed(callId: callIdB1toA, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateB1.shouldSendOfferInvoked).toEventually(equal(true), timeout: 2)
            delegateB1.shouldSendOfferInvoked = false

            // Give the offer from A1 to B1 & B2.
            do {
                Logger.debug("Test: Invoking B*.receivedOffer(A1)...")
                let callA1toB1 = OpaqueCallData(value: delegateB1.expectedValue, remote: aAddress)
                try callManagerB1?.receivedOffer(call: callA1toB1, sourceDevice: a1Device, callId: callIdA1toB, opaque: nil, sdp: delegateA1.sentOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: b1Device, remoteSupportsMultiRing: true, isLocalDevicePrimary: true)
                let callA1toB2 = OpaqueCallData(value: delegateB2.expectedValue, remote: aAddress)
                try callManagerB2?.receivedOffer(call: callA1toB2, sourceDevice: a1Device, callId: callIdA1toB, opaque: nil, sdp: delegateA1.sentOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: b2Device, remoteSupportsMultiRing: true, isLocalDevicePrimary: false)
            } catch {
                XCTFail("Call Manager receivedOffer() failed: \(error)")
                return
            }

            if scenario == .normalGlareOfferBeforeBusy {
                // Give the offer from B1 to A1.
                do {
                    Logger.debug("Test: Invoking A1.receivedOffer(B1)...")
                    let call = OpaqueCallData(value: delegateA1.expectedValue, remote: bAddress)
                    try callManagerA1?.receivedOffer(call: call, sourceDevice: b1Device, callId: callIdB1toA, opaque: nil, sdp: delegateB1.sentOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: a1Device, remoteSupportsMultiRing: true, isLocalDevicePrimary: true)
                } catch {
                    XCTFail("Call Manager receivedOffer() failed: \(error)")
                    return
                }

                // A1 behavior:

                expect(delegateA1.eventEndedReceivedOfferWhileActive).toEventually(equal(true), timeout: 1)
                delegateA1.eventEndedReceivedOfferWhileActive = false

                // Busy is for the incoming offer.
                expect(delegateA1.shouldSendBusyInvoked).toEventually(equal(true), timeout: 1)
                delegateA1.shouldSendBusyInvoked = false

                expect(delegateA1.eventEndedRemoteGlare).toEventually(equal(true), timeout: 1)
                delegateA1.eventEndedRemoteGlare = false

                // Hangup is for the outgoing offer.
                expect(delegateA1.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: 2)
                delegateA1.shouldSendHangupNormalInvoked = false
            }

            // B1 behavior:

            expect(delegateB1.eventEndedReceivedOfferWhileActive).toEventually(equal(true), timeout: 1)
            delegateB1.eventEndedReceivedOfferWhileActive = false

            // Busy is for the incoming offer.
            expect(delegateB1.shouldSendBusyInvoked).toEventually(equal(true), timeout: 1)
            delegateB1.shouldSendBusyInvoked = false

            expect(delegateB1.eventEndedRemoteGlare).toEventually(equal(true), timeout: 1)
            delegateB1.eventEndedRemoteGlare = false

            // Hangup is for the outgoing offer.
            expect(delegateB1.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: 2)
            delegateB1.shouldSendHangupNormalInvoked = false

            // Reset B1 general detection (to check later).
            delegateB1.generalInvocationDetected = false

            // B2 behavior:

            expect(delegateB2.startIncomingCallInvoked).toEventually(equal(true), timeout: 1)
            delegateB2.startIncomingCallInvoked = false

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerB2?.proceed(callId: callIdA1toB, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateB2.shouldSendAnswerInvoked).toEventually(equal(true), timeout: 2)
            delegateB2.shouldSendAnswerInvoked = false

            // We won't move anything from B2 for this test.

            if scenario == .normalGlareOfferBeforeBusy {
                // Hangup B because glare detection caused the normal hangup.
                do {
                    Logger.debug("Test: Invoking B*.receivedHangup(A1)...")
                    try callManagerB1?.receivedHangup(sourceDevice: a1Device, callId: callIdA1toB, hangupType: .normal, deviceId: 0)
                    try callManagerB2?.receivedHangup(sourceDevice: a1Device, callId: callIdA1toB, hangupType: .normal, deviceId: 0)
                } catch {
                    XCTFail("Call Manager receivedBusy() failed: \(error)")
                    return
                }

                expect(delegateB2.eventEndedRemoteHangup).toEventually(equal(true), timeout: 1)
                delegateA1.eventEndedRemoteHangup = false
            } else if scenario == .normalGlareBusyBeforeOffer {
                // Deliver the Busy from B to A to make sure it is handled correctly.

                do {
                    Logger.debug("Test: Invoking A1.receivedBusy(B1)...")
                    try callManagerA1?.receivedBusy(sourceDevice: b1Device, callId: callIdA1toB)
                } catch {
                    XCTFail("Call Manager receivedBusy() failed: \(error)")
                    return
                }

                expect(delegateA1.eventEndedRemoteBusy).toEventually(equal(true), timeout: 1)
                delegateA1.eventEndedRemoteBusy = false

                // Now the multi-ring calls should be cancelled with hangup/busy.
                expect(delegateA1.shouldSendHangupBusyInvoked).toEventually(equal(true), timeout: 1)
                delegateA1.shouldSendHangupBusyInvoked = false
                expect(delegateA1.recentCallId).to(equal(callIdA1toB))
                expect(delegateA1.hangupDeviceId).to(equal(b1Device))

                // Hangup B.
                do {
                    Logger.debug("Test: Invoking B*.receivedHangup(A1)...")
                    try callManagerB1?.receivedHangup(sourceDevice: a1Device, callId: callIdA1toB, hangupType: .busy, deviceId: delegateA1.hangupDeviceId ?? 0)
                    try callManagerB2?.receivedHangup(sourceDevice: a1Device, callId: callIdA1toB, hangupType: .busy, deviceId: delegateA1.hangupDeviceId ?? 0)
                } catch {
                    XCTFail("Call Manager receivedBusy() failed: \(error)")
                    return
                }

                expect(delegateB2.eventEndedRemoteHangupBusy).toEventually(equal(true), timeout: 1)
                delegateA1.eventEndedRemoteHangupBusy = false
            }

            // B1 shouldn't have done anything.
            expect(delegateB1.generalInvocationDetected).to(equal(false))

            // Deliver the Hangup from B1 to A to make sure it is handled correctly.
            delegateA1.generalInvocationDetected = false

            do {
                Logger.debug("Test: Invoking A1.receivedHangup(B1)...")
                try callManagerA1?.receivedHangup(sourceDevice: b1Device, callId: callIdB1toA, hangupType: .normal, deviceId: 0)
            } catch {
                XCTFail("Call Manager receivedHangup() failed: \(error)")
                return
            }

            // A1 shouldn't have done anything.
            expect(delegateA1.generalInvocationDetected).to(equal(false))
        } else if scenario == .differentDevice {
            // @todo Place A/B connection establishment code in its own class.

            // Get A1 and B1 in to a call.

            // Give the offer from A1 to B1 & B2.
            do {
                Logger.debug("Test: Invoking B*.receivedOffer(A1)...")
                let callA1toB1 = OpaqueCallData(value: delegateB1.expectedValue, remote: aAddress)
                try callManagerB1?.receivedOffer(call: callA1toB1, sourceDevice: a1Device, callId: callIdA1toB, opaque: nil, sdp: delegateA1.sentOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: b1Device, remoteSupportsMultiRing: true, isLocalDevicePrimary: true)
                let callA1toB2 = OpaqueCallData(value: delegateB2.expectedValue, remote: aAddress)
                try callManagerB2?.receivedOffer(call: callA1toB2, sourceDevice: a1Device, callId: callIdA1toB, opaque: nil, sdp: delegateA1.sentOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: b2Device, remoteSupportsMultiRing: true, isLocalDevicePrimary: false)
            } catch {
                XCTFail("Call Manager receivedOffer() failed: \(error)")
                return
            }

            expect(delegateB1.startIncomingCallInvoked).toEventually(equal(true), timeout: 1)
            delegateB1.startIncomingCallInvoked = false

            expect(delegateB2.startIncomingCallInvoked).toEventually(equal(true), timeout: 1)
            delegateB2.startIncomingCallInvoked = false

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerB1?.proceed(callId: callIdA1toB, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
                _ = try callManagerB2?.proceed(callId: callIdA1toB, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateB1.shouldSendAnswerInvoked).toEventually(equal(true), timeout: 1)
            delegateB1.shouldSendAnswerInvoked = false
            expect(delegateB2.shouldSendAnswerInvoked).toEventually(equal(true), timeout: 1)
            delegateB2.shouldSendAnswerInvoked = false

            // We also expect ICE candidates to be ready for A1 and B1.
            expect(delegateA1.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: 1)
            delegateA1.shouldSendIceCandidatesInvoked = false
            expect(delegateB1.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: 1)
            delegateB1.shouldSendIceCandidatesInvoked = false

            // Send answer and candidates between A1 and B1.
            do {
                Logger.debug("Test: Invoking received*()...")
                try callManagerA1?.receivedAnswer(sourceDevice: b1Device, callId: callIdA1toB, opaque: nil, sdp: delegateB1.sentAnswer, remoteSupportsMultiRing: true)
                try callManagerB1?.receivedIceCandidates(sourceDevice: a1Device, callId: callIdA1toB, candidates: delegateA1.sentIceCandidates)
                try callManagerA1?.receivedIceCandidates(sourceDevice: b1Device, callId: callIdA1toB, candidates: delegateB1.sentIceCandidates)
            } catch {
                XCTFail("Call Manager received*() failed: \(error)")
                return
            }

            // Should get to ringing.
            expect(delegateA1.eventRemoteRingingInvoked).toEventually(equal(true), timeout: 1)
            delegateA1.eventRemoteRingingInvoked = false
            expect(delegateB1.eventLocalRingingInvoked).toEventually(equal(true), timeout: 1)
            delegateB1.eventLocalRingingInvoked = false

            // Accept on B1.
            do {
                Logger.debug("Test: Invoking accept()...")
                try callManagerB1?.accept(callId: callIdA1toB)
            } catch {
                XCTFail("Call Manager accept() failed: \(error)")
                return
            }

            // Should get connected.
            expect(delegateA1.eventRemoteConnectedInvoked).toEventually(equal(true), timeout: 1)
            delegateA1.eventRemoteConnectedInvoked = false
            expect(delegateB1.eventLocalConnectedInvoked).toEventually(equal(true), timeout: 1)
            delegateB1.eventLocalConnectedInvoked = false

            // Should get hangup/Accepted to be sent to B.
            expect(delegateA1.shouldSendHangupAcceptedInvoked).toEventually(equal(true), timeout: 1)
            delegateA1.shouldSendHangupAcceptedInvoked = false

            // Send hangup/Accepted to B1 and B2.
            do {
                Logger.debug("Test: Invoking receivedHangup()...")
                try callManagerB1?.receivedHangup(sourceDevice: a1Device, callId: callIdA1toB, hangupType: .accepted, deviceId: delegateA1.hangupDeviceId ?? 0)
                try callManagerB2?.receivedHangup(sourceDevice: a1Device, callId: callIdA1toB, hangupType: .accepted, deviceId: delegateA1.hangupDeviceId ?? 0)
            } catch {
                XCTFail("Call Manager accept() failed: \(error)")
                return
            }

            // B2 should be ended.
            expect(delegateB2.eventEndedRemoteHangupAccepted).toEventually(equal(true), timeout: 1)
            delegateB2.eventEndedRemoteHangupAccepted = false

            // B1 should not be ended.
            expect(delegateB1.eventGeneralEnded).to(equal(false))

            // Finally, get to the actual test. A2 should be able to call B2.

            // Clear any state.
            // @todo Make a reset() function for delegates.
            delegateB2.sentIceCandidates = []

            do {
                Logger.debug("Test: A2 calls B...")
                let call = OpaqueCallData(value: delegateA2.expectedValue, remote: bAddress)
                try callManagerA2?.placeCall(call: call, callMediaType: .audioCall, localDevice: a2Device)
            } catch {
                XCTFail("Call Manager call() failed: \(error)")
                return
            }

            expect(delegateA2.startOutgoingCallInvoked).toEventually(equal(true), timeout: 1)
            delegateA2.startOutgoingCallInvoked = false
            let callIdA2toB = delegateA2.recentCallId

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerA2?.proceed(callId: callIdA2toB, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateA2.shouldSendOfferInvoked).toEventually(equal(true), timeout: 1)
            delegateA2.shouldSendOfferInvoked = false

            // Give the offer from A2 to B1 & B2.
            do {
                Logger.debug("Test: Invoking B*.receivedOffer(A2)...")
                let callA2toB1 = OpaqueCallData(value: delegateB1.expectedValue, remote: aAddress)
                try callManagerB1?.receivedOffer(call: callA2toB1, sourceDevice: a2Device, callId: callIdA2toB, opaque: nil, sdp: delegateA2.sentOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: b1Device, remoteSupportsMultiRing: true, isLocalDevicePrimary: true)
                let callA2toB2 = OpaqueCallData(value: delegateB2.expectedValue, remote: aAddress)
                try callManagerB2?.receivedOffer(call: callA2toB2, sourceDevice: a2Device, callId: callIdA2toB, opaque: nil, sdp: delegateA2.sentOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: b2Device, remoteSupportsMultiRing: true, isLocalDevicePrimary: false)
            } catch {
                XCTFail("Call Manager receivedOffer() failed: \(error)")
                return
            }

            // B1 behavior:

            expect(delegateB1.eventEndedReceivedOfferWhileActive).toEventually(equal(true), timeout: 1)
            delegateB1.eventEndedReceivedOfferWhileActive = false

            // Busy is for the incoming offer.
            expect(delegateB1.shouldSendBusyInvoked).toEventually(equal(true), timeout: 1)
            delegateB1.shouldSendBusyInvoked = false

            // B2 behavior:

            expect(delegateB2.startIncomingCallInvoked).toEventually(equal(true), timeout: 1)
            delegateB2.startIncomingCallInvoked = false

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerB2?.proceed(callId: callIdA2toB, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateB2.shouldSendAnswerInvoked).toEventually(equal(true), timeout: 2)
            delegateB2.shouldSendAnswerInvoked = false

            // We also expect ICE candidates to be ready for A2 and B2.
            expect(delegateA2.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: 1)
            delegateA2.shouldSendIceCandidatesInvoked = false
            expect(delegateB2.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: 1)
            delegateB2.shouldSendIceCandidatesInvoked = false

            // Give the busy from B1 back to A.
            do {
                Logger.debug("Test: Invoking A*.receivedBusy(B1)...")
                try callManagerA1?.receivedBusy(sourceDevice: b1Device, callId: callIdA2toB)
                try callManagerA2?.receivedBusy(sourceDevice: b1Device, callId: callIdA2toB)
            } catch {
                XCTFail("Call Manager receivedBusy() failed: \(error)")
                return
            }

            // Just make sure A2 ends and generates hangup/busy.
            expect(delegateA2.eventEndedRemoteBusy).toEventually(equal(true), timeout: 1)
            delegateA2.eventEndedRemoteBusy = false
            expect(delegateA2.shouldSendHangupBusyInvoked).toEventually(equal(true), timeout: 1)
            delegateA2.shouldSendHangupBusyInvoked = false
        }

        // Release the Call Managers (but there still might be references in the delegates!).
        callManagerA1 = nil
        callManagerA2 = nil
        callManagerB1 = nil
        callManagerB2 = nil

        // See what clears up after closing the Call Manager...
        delay(interval: 1.0)

        Logger.debug("Test: Exiting test function...")
    }

    func testMultiRingGlareNormalOfferFirst() {
        multiRingGlareTesting(scenario: .normalGlareOfferBeforeBusy)
    }

    func testMultiRingGlareNormalBusyFirst() {
        multiRingGlareTesting(scenario: .normalGlareBusyBeforeOffer)
    }

    func testMultiRingGlareDifferentDevice() {
        multiRingGlareTesting(scenario: .differentDevice)
    }

    // MARK: - Constants

    let audioOffer =
        "v=0\r\n" +
        "o=- 6814183694769985039 2 IN IP4 127.0.0.1\r\n" +
        "s=-\r\n" +
        "t=0 0\r\n" +
        "a=group:BUNDLE audio data\r\n" +
        "a=msid-semantic: WMS ARDAMS\r\n" +
        "m=audio 9 UDP/TLS/RTP/SAVPF 111\r\n" +
        "c=IN IP4 0.0.0.0\r\n" +
        "a=rtcp:9 IN IP4 0.0.0.0\r\n" +
        "a=ice-ufrag:VLSN\r\n" +
        "a=ice-pwd:9i7G0u4UW2NBi+HFScgTi9PF\r\n" +
        "a=ice-options:trickle renomination\r\n" +
        "a=fingerprint:sha-256 71:CB:D2:0B:59:35:DA:C6:E0:DD:B8:86:E0:97:F7:44:C2:8D:ED:D3:C7:75:1D:F2:0C:2D:A7:B0:D9:29:33:95\r\n" +
        "a=setup:actpass\r\n" +
        "a=mid:audio\r\n" +
        "a=extmap:1 http://www.webrtc.org/experiments/rtp-hdrext/abs-send-time\r\n" +
        "a=extmap:2 http://www.ietf.org/id/draft-holmer-rmcat-transport-wide-cc-extensions-01\r\n" +
        "a=sendrecv\r\n" +
        "a=rtcp-mux\r\n" +
        "a=rtpmap:111 opus/48000/2\r\n" +
        "a=rtcp-fb:111 transport-cc\r\n" +
        "a=fmtp:111 cbr=1;minptime=10;useinbandfec=1\r\n" +
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
