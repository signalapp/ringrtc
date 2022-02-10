//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import XCTest
@testable import SignalRingRTC
import WebRTC
import SignalCoreKit

import Nimble

typealias TestCallManager = CallManager<OpaqueCallData, TestDelegate>
func createCallManager(_ delegate: TestDelegate) -> TestCallManager? {
    let httpClient = HTTPClient(delegate: delegate)
    let call_manager = TestCallManager(httpClient: httpClient)
    call_manager.delegate = delegate
    return call_manager
}

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

final class TestDelegate: CallManagerDelegate & HTTPDelegate {
    public typealias CallManagerDelegateCallType = OpaqueCallData

    // Simulate the promise-like async handling of signaling messages.
    private let signalingQueue = DispatchQueue(label: "org.signal.signalingQueue")

    // Setup hooks.
    var doAutomaticProceed = false
    var videoCaptureController: VideoCaptureController?
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
    var shouldSendCallMessageInvoked = false
    var shouldSendCallMessageToGroupInvoked = false
    var shouldSendHttpRequestInvoked = false
    var didUpdateRingForGroupInvoked = false
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
    var eventEndedRemoteReCall = false
    var eventEndedSignalingFailure = false
    var eventEndedGlareHandlingFailure = false
    var eventEndedDropped = false
    var eventReconnecting = false
    var eventReconnected = false
    var eventReceivedOfferWhileActive = false
    var eventReceivedOfferWithGlare = false
    var eventIgnoreCallsFromNonMultiringCallers = false

    var eventGeneralEnded = false

    // When starting a call, if it was prevented from invoking proceed due to call concluded.
//    var callWasConcludedNoProceed = false

    // For object verification, the value expected in callData (i.e. the remote object).
    var expectedValue: Int32 = 0

    var messageSendingDelay: useconds_t = 150 * 1000

    // The most recent callId handled.
    var recentCallId: UInt64 = 0
    var recentBusyCallId: UInt64 = 0

    var sentOfferOpaque: Data?
    var sentAnswerOpaque: Data?
    var sentIceCandidates: [Data] = []

    var sentCallMessageRecipientUuid: UUID?
    var sentCallMessageMessage: Data?
    var sentCallMessageUrgency: CallMessageUrgency?

    var sentCallMessageToGroupGroupId: Data?
    var sentCallMessageToGroupMessage: Data?
    var sentCallMessageToGroupUrgency: CallMessageUrgency?

    var sentHttpRequestId: UInt32?
    var sentHttpRequestUrl: String?
    var sentHttpRequestMethod: HTTPMethod?
    var sentHttpRequestHeaders: [String: String]?
    var sentHttpRequestBody: Data?

    var didUpdateRingForGroupGroupId: Data?
    var didUpdateRingForGroupRingId: Int64?
    var didUpdateRingForGroupSender: UUID?
    var didUpdateRingForGroupUpdate: RingUpdate?

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
                            _ = try callManager.proceed(callId: callId, iceServers: self.iceServers, hideIp: self.useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
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

        case .endedRemoteReCall:
            Logger.debug("TestDelegate:endedRemoteReCall")
            eventGeneralEnded = true
            eventEndedRemoteReCall = true

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

        case .endedGlareHandlingFailure:
            Logger.debug("TestDelegate:endedGlareHandlingFailure")
            eventGeneralEnded = true
            eventEndedGlareHandlingFailure = true

        case .endedConnectionFailure:
            Logger.debug("TestDelegate:endedConnectionFailure")
            eventGeneralEnded = true

        case .endedDropped:
            Logger.debug("TestDelegate:endedDropped")
            eventGeneralEnded = true
            eventEndedDropped = true

        case .remoteVideoEnable:
            Logger.debug("TestDelegate:remoteVideoEnable")

        case .remoteVideoDisable:
            Logger.debug("TestDelegate:remoteVideoDisable")

        case .remoteSharingScreenEnable:
            Logger.debug("TestDelegate:remoteSharingScreenEnable")

        case .remoteSharingScreenDisable:
            Logger.debug("TestDelegate:remoteSharingScreenDisable")

        case .reconnecting:
            Logger.debug("TestDelegate:reconnecting")
            eventReconnecting = true

        case .reconnected:
            Logger.debug("TestDelegate:reconnected")
            eventReconnected = true

        case .receivedOfferExpired:
            Logger.debug("TestDelegate:receivedOfferExpired")

        case .receivedOfferWhileActive:
            Logger.debug("TestDelegate:receivedOfferWhileActive")
            eventReceivedOfferWhileActive = true

        case .receivedOfferWithGlare:
            Logger.debug("TestDelegate:receivedOfferWithGlare")
            eventReceivedOfferWithGlare = true
        }
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, onNetworkRouteChangedFor call: OpaqueCallData, networkRoute: NetworkRoute) {
        Logger.debug("TestDelegate:onNetworkRouteChangedFor - \(networkRoute.localAdapterType)")
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, onAudioLevelsFor call: OpaqueCallData, capturedLevel: UInt16, receivedLevel: UInt16) {
        Logger.debug("TestDelegate:onAudioLevelsFor - \(capturedLevel) \(receivedLevel)")
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendOffer callId: UInt64, call: OpaqueCallData, destinationDeviceId: UInt32?, opaque: Data, callMediaType: CallMediaType) {
        Logger.debug("TestDelegate:shouldSendOffer")
        generalInvocationDetected = true

        guard call.value == expectedValue else {
            XCTFail("call object not expected")
            return
        }

        recentCallId = callId

        // @todo Create a structure to hold offers by deviceId
        if destinationDeviceId == nil || destinationDeviceId == 1 {
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

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendAnswer callId: UInt64, call: OpaqueCallData, destinationDeviceId: UInt32?, opaque: Data) {
        Logger.debug("TestDelegate:shouldSendAnswer")
        generalInvocationDetected = true

        recentCallId = callId

        // @todo Create a structure to hold answers by deviceId
        if destinationDeviceId == nil || destinationDeviceId == 1 {
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

    func resetIceHandlingState() {
        canSendICE = false
        sentIceCandidates = []
        shouldSendIceCandidatesInvoked = false
    }

    func tryToSendIceCandidates(callId: UInt64, destinationDeviceId: UInt32?, candidates: [Data]) {
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

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendIceCandidates callId: UInt64, call: OpaqueCallData, destinationDeviceId: UInt32?, candidates: [Data]) {
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

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendHangup callId: UInt64, call: OpaqueCallData, destinationDeviceId: UInt32?, hangupType: HangupType, deviceId: UInt32) {
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

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendCallMessage recipientUuid: UUID, message: Data, urgency: CallMessageUrgency) {
        Logger.debug("TestDelegate:shouldSendCallMessage")
        generalInvocationDetected = true

        shouldSendCallMessageInvoked = true

        sentCallMessageRecipientUuid = recipientUuid
        sentCallMessageMessage = message
        sentCallMessageUrgency = urgency
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, shouldSendCallMessageToGroup groupId: Data, message: Data, urgency: CallMessageUrgency) {
        Logger.debug("TestDelegate:shouldSendCallMessageToGroup")
        generalInvocationDetected = true

        shouldSendCallMessageToGroupInvoked = true

        sentCallMessageToGroupGroupId = groupId
        sentCallMessageToGroupMessage = message
        sentCallMessageToGroupUrgency = urgency
    }
    func sendRequest(requestId: UInt32, request: HTTPRequest) {
        Logger.debug("TestDelegate:shouldSendHttpRequest")
        generalInvocationDetected = true

        shouldSendHttpRequestInvoked = true

        sentHttpRequestId = requestId
        sentHttpRequestUrl = request.url
        sentHttpRequestMethod = request.method
        sentHttpRequestHeaders = request.headers
        sentHttpRequestBody = request.body

        Logger.debug("requestId: \(requestId)")
        Logger.debug("url: \(request.url)")
        Logger.debug("method: \(request.method)")
        Logger.debug("headers:")
        request.headers.forEach { (header) in
            Logger.debug("key: \(header.key) value: \(header.value)")
        }
        Logger.debug("body: \(request.body)")
    }

    func callManager(_ callManager: CallManager<OpaqueCallData, TestDelegate>, didUpdateRingForGroup groupId: Data, ringId: Int64, sender: UUID, update: RingUpdate) {
        Logger.debug("TestDelegate:didUpdateRingForGroup")
        generalInvocationDetected = true

        didUpdateRingForGroupInvoked = true

        didUpdateRingForGroupGroupId = groupId
        didUpdateRingForGroupRingId = ringId
        didUpdateRingForGroupSender = sender
        didUpdateRingForGroupUpdate = update
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

    override class func setUp() {
        // Initialize logging, direct it to the console.
        DDLog.add(DDOSLogger.sharedInstance)

        // Allow as many file descriptors as possible.
        var limits = rlimit()
        if getrlimit(RLIMIT_NOFILE, &limits) == 0 {
            limits.rlim_cur = min(rlim_t(OPEN_MAX), limits.rlim_max)
            if setrlimit(RLIMIT_NOFILE, &limits) == 0 {
                Logger.info("number of open files allowed: \(limits.rlim_cur)")
            } else {
                Logger.error("failed to allow more open files: " + String(cString: strerror(errno)))
            }
        }
    }

    // Helper function to delay, without blocking the main thread.
    func delay(interval: TimeInterval) {
        var timerFlag = false
        Timer.scheduledTimer(withTimeInterval: interval, repeats: false, block: { (_) in
            timerFlag = true
        })
        // Wait for the timer to expire, and give expectation timeout in excess of delay.
        expect(timerFlag).toEventually(equal(true), timeout: .milliseconds(Int((interval + 1) * 1000)))
    }

    func testMinimalLifetime() {
        Logger.debug("Test: Minimal Lifetime...")

        // The Call Manager object itself is fairly lightweight, although its initializer
        // creates the global singleton and associated global logger in the RingRTC
        // Rust object.

        let delegate = TestDelegate()
        var callManager = createCallManager(delegate)
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
        var callManager = createCallManager(delegate)
        expect(callManager).toNot(beNil())
        expect(delegate.generalInvocationDetected).to(equal(false))
        callManager = nil

        callManager = createCallManager(delegate)
        expect(callManager).toNot(beNil())
        expect(delegate.generalInvocationDetected).to(equal(false))
        callManager = nil

        callManager = createCallManager(delegate)
        expect(callManager).toNot(beNil())
        expect(delegate.generalInvocationDetected).to(equal(false))
        callManager = nil

        callManager = createCallManager(delegate)
        expect(callManager).toNot(beNil())
        expect(delegate.generalInvocationDetected).to(equal(false))
        callManager = nil

        callManager = createCallManager(delegate)
        expect(callManager).toNot(beNil())
        expect(delegate.generalInvocationDetected).to(equal(false))
        callManager = nil

        // Delay the end of the test to give Logger time to catch up.
        delay(interval: 0.1)
    }

    func testShortLife() {
        Logger.debug("Test: Create the Call Manager and close it quickly...")

        let delegate = TestDelegate()

        // Create Call Manager object, which will create a WebRTC factory
        // and the RingRTC Rust Call Manager object(s).
        var callManager = createCallManager(delegate)
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

    func outgoingTesting(bandwidthMode: BandwidthMode) {
        Logger.debug("Test: Outgoing Call...")

        let delegate = TestDelegate()
        var callManager = createCallManager(delegate)
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

        expect(delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
        delegate.startOutgoingCallInvoked = false

        let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
        let useTurnOnly = false

        var callId = delegate.recentCallId

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManager?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: bandwidthMode, audioLevelsIntervalMillis: nil)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        expect(delegate.shouldSendOfferInvoked).toEventually(equal(true), timeout: .seconds(2))
        delegate.shouldSendOfferInvoked = false

        // We've sent an offer, so we should see some Ice candidates.
        // @todo Update now that we can send Ice candidates before receiving the Answer.

        let sourceDevice: UInt32 = 1

        do {
            Logger.debug("Test: Invoking receivedAnswer()...")
            try callManager?.receivedAnswer(sourceDevice: 1, callId: callId, opaque: exampleV4Answer, senderIdentityKey: dummyRemoteIdentityKey, receiverIdentityKey: dummyLocalIdentityKey)
        } catch {
            XCTFail("Call Manager receivedAnswer() failed: \(error)")
            return
        }

        // We don't care how many though. No need to reset the flag.
        expect(delegate.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: .seconds(1))

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

    func testOutgoingNormal() {
        outgoingTesting(bandwidthMode: .normal)
    }

    func testOutgoingLow() {
        outgoingTesting(bandwidthMode: .low)
    }

    func testOutgoingVeryLow() {
        outgoingTesting(bandwidthMode: .veryLow)
    }

    func testOutgoingSendOfferFail() {
        Logger.debug("Test: Outgoing Call Send Offer Fail...")

        let delegate = TestDelegate()
        var callManager = createCallManager(delegate)
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

        expect(delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
        delegate.startOutgoingCallInvoked = false

        let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
        let useTurnOnly = false

        let callId = delegate.recentCallId

        // Make sure the offer fails to send...
        delegate.doFailSendOffer = true

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManager?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        expect(delegate.shouldSendOfferInvoked).toEventually(equal(true), timeout: .seconds(1))

        // We should get the endedSignalingFailure event.
        expect(delegate.eventEndedSignalingFailure).toEventually(equal(true), timeout: .seconds(1))

        // We expect to get a hangup, because, the Call Manager doesn't make
        // any assumptions that the offer didn't really actually get out.
        // Just to be sure, it will send the hangup...
        expect(delegate.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: .seconds(1))

        // Release the Call Manager.
        callManager = nil

        Logger.debug("Test: Exiting test function...")
    }

    func testIncoming() {
        Logger.debug("Test: Incoming Call...")

        let delegate = TestDelegate()
        var callManager = createCallManager(delegate)
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

            try callManager?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, opaque: exampleV4V3V2Offer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: localDevice, isLocalDevicePrimary: true, senderIdentityKey: dummyRemoteIdentityKey, receiverIdentityKey: dummyLocalIdentityKey)
        } catch {
            XCTFail("Call Manager receivedOffer() failed: \(error)")
            return
        }

        expect(delegate.startIncomingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
        delegate.startIncomingCallInvoked = false

        let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
        let useTurnOnly = false

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManager?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        expect(delegate.shouldSendAnswerInvoked).toEventually(equal(true), timeout: .seconds(2))
        delegate.shouldSendAnswerInvoked = false

        expect(delegate.recentCallId).to(equal(callId))

        // We've sent an answer, so we should see some Ice Candidates.

        // We don't care how many though. No need to reset the flag.
        expect(delegate.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: .seconds(1))

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

    func testOutgoingMultiHangupMin() {
        Logger.debug("Test: MultiHangup Minimum...")

        let delegate = TestDelegate()
        var callManager = createCallManager(delegate)
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
        var callManager = createCallManager(delegate)
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

            expect(delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
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
        var callManager = createCallManager(delegate)
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

            expect(delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegate.startOutgoingCallInvoked = false

            let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
            let useTurnOnly = false

            let callId = delegate.recentCallId

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManager?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
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
        var callManager = createCallManager(delegate)
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

            expect(delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegate.startOutgoingCallInvoked = false

            let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
            let useTurnOnly = false

            let callId = delegate.recentCallId

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManager?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegate.shouldSendOfferInvoked).toEventually(equal(true), timeout: .seconds(2))
            delegate.shouldSendOfferInvoked = false

            // Try hanging up...
            do {
                Logger.debug("Test: Invoking hangup()...")
                try callManager?.hangup()
            } catch {
                XCTFail("Call Manager hangup() failed: \(error)")
                return
            }

            expect(delegate.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: .seconds(2))
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
        var callManager = createCallManager(delegate)
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

            try callManager?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, opaque: exampleV4V3V2Offer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: localDevice, isLocalDevicePrimary: true, senderIdentityKey: dummyRemoteIdentityKey, receiverIdentityKey: dummyLocalIdentityKey)
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
        var callManager = createCallManager(delegate)
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

            try callManager?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, opaque: exampleV4V3V2Offer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: localDevice, isLocalDevicePrimary: true, senderIdentityKey: dummyRemoteIdentityKey, receiverIdentityKey: dummyLocalIdentityKey)
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

    func multiCallTesting(loopIterations: Int) {
        Logger.debug("Test: MultiCall...")

        let delegateCaller = TestDelegate()
        var callManagerCaller = createCallManager(delegateCaller)
        expect(callManagerCaller).toNot(beNil())
        delegateCaller.expectedValue = 12345
        let callerAddress: Int32 = 888888
        let callerLocalDevice: UInt32 = 1

        let delegateCallee = TestDelegate()
        var callManagerCallee = createCallManager(delegateCallee)
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

            expect(delegateCaller.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateCaller.startOutgoingCallInvoked = false

            // This may not be proper...
            let callId = delegateCaller.recentCallId

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerCaller?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateCaller.shouldSendOfferInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateCaller.shouldSendOfferInvoked = false

            // We sent the offer! Let's give it to our callee!
            do {
                Logger.debug("Test: Invoking receivedOffer()...")

                // Define some CallData for simulation. This is defined in a block
                // so that we validate that it is retained correctly and accessible
                // outside this block.
                let call = OpaqueCallData(value: delegateCallee.expectedValue, remote: callerAddress)

                guard let opaque = delegateCaller.sentOfferOpaque else {
                    XCTFail("No sentOfferOpaque detected!")
                    return
                }

                try callManagerCallee?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, opaque: opaque, messageAgeSec: 0, callMediaType: .audioCall, localDevice: calleeLocalDevice, isLocalDevicePrimary: true, senderIdentityKey: dummyRemoteIdentityKey, receiverIdentityKey: dummyLocalIdentityKey)
            } catch {
                XCTFail("Call Manager receivedOffer() failed: \(error)")
                return
            }

            // We've given the offer to the callee device, let's let ICE flow from caller as well.
            // @note Some ICE may flow starting now.
            Logger.debug("Starting ICE flow for caller...")
            delegateCaller.canSendICE = true
            delegateCaller.tryToSendIceCandidates(callId: callId, destinationDeviceId: nil, candidates: [])

            expect(delegateCallee.startIncomingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateCallee.startIncomingCallInvoked = false

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerCallee?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateCallee.shouldSendAnswerInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateCallee.shouldSendAnswerInvoked = false

            expect(delegateCallee.recentCallId).to(equal(callId))

            // We have an answer, so give it back to the caller.

            do {
                Logger.debug("Test: Invoking receivedAnswer()...")

                guard let opaque = delegateCallee.sentAnswerOpaque else {
                    XCTFail("No sentAnswerOpaque detected!")
                    return
                }

                try callManagerCaller?.receivedAnswer(sourceDevice: sourceDevice, callId: callId, opaque: opaque, senderIdentityKey: dummyLocalIdentityKey, receiverIdentityKey: dummyRemoteIdentityKey)
            } catch {
                XCTFail("Call Manager receivedAnswer() failed: \(error)")
                return
            }

            // Should get to ringing.
            expect(delegateCaller.eventRemoteRingingInvoked).toEventually(equal(true), timeout: .seconds(2))
            delegateCaller.eventRemoteRingingInvoked = false
            expect(delegateCallee.eventLocalRingingInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateCallee.eventLocalRingingInvoked = false

            // Now we want to hangup the callee and start anew.
            do {
                Logger.debug("Test: Invoking hangup()...")
                _ = try callManagerCaller?.hangup()
            } catch {
                XCTFail("Call Manager hangup() failed: \(error)")
                return
            }

            expect(delegateCaller.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateCaller.shouldSendHangupNormalInvoked = false

            do {
                Logger.debug("Test: Invoking receivedHangup()...")
                _ = try callManagerCallee?.receivedHangup(sourceDevice: sourceDevice, callId: callId, hangupType: .normal, deviceId: 0)
            } catch {
                XCTFail("Call Manager hangup() failed: \(error)")
                return
            }

            expect(delegateCallee.eventEndedRemoteHangup).toEventually(equal(true), timeout: .seconds(1))
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

    func testMultiCallOpaque() {
        multiCallTesting(loopIterations: 2)
    }

    func testMultiCallFastIceCheck() {
        Logger.debug("Test: MultiCall check that immediate ICE message is handled...")

        let delegateCaller = TestDelegate()
        var callManagerCaller = createCallManager(delegateCaller)
        expect(callManagerCaller).toNot(beNil())

        let delegateCallee = TestDelegate()
        var callManagerCallee = createCallManager(delegateCallee)
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

        expect(delegateCaller.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
        delegateCaller.startOutgoingCallInvoked = false

        // For now, these variables will be common to both Call Managers.
        let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
        let useTurnOnly = false

        let callId = delegateCaller.recentCallId

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManagerCaller?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        // Wait for the offer.
        expect(delegateCaller.shouldSendOfferInvoked).toEventually(equal(true), timeout: .seconds(1))

        // Wait for the initial set of ICE candidates.
        expect(delegateCaller.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: .seconds(1))

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

            guard let opaque = delegateCaller.sentOfferOpaque else {
                XCTFail("No sentOfferOpaque detected!")
                return
            }

            // Send the ICE candidates right after the offer.
            try callManagerCallee?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callId, opaque: opaque, messageAgeSec: 0, callMediaType: .audioCall, localDevice: calleeLocalDevice, isLocalDevicePrimary: true, senderIdentityKey: dummyRemoteIdentityKey, receiverIdentityKey: dummyLocalIdentityKey)
            try callManagerCallee?.receivedIceCandidates(sourceDevice: sourceDevice, callId: callId, candidates: delegateCaller.sentIceCandidates)
        } catch {
            XCTFail("Call Manager receivedOffer() failed: \(error)")
            return
        }

        // Continue on with the call to see it get a connection.
        expect(delegateCallee.startIncomingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
        delegateCallee.startIncomingCallInvoked = false

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManagerCallee?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        expect(delegateCallee.shouldSendAnswerInvoked).toEventually(equal(true), timeout: .seconds(2))
        delegateCallee.shouldSendAnswerInvoked = false

        expect(delegateCallee.recentCallId).to(equal(callId))

        // We have an answer, so give it back to the caller.

        do {
            Logger.debug("Test: Invoking receivedAnswer()...")

            guard let opaque = delegateCallee.sentAnswerOpaque else {
                XCTFail("No sentOfferOpaque detected!")
                return
            }

            try callManagerCaller?.receivedAnswer(sourceDevice: sourceDevice, callId: callId, opaque: opaque, senderIdentityKey: dummyLocalIdentityKey, receiverIdentityKey: dummyRemoteIdentityKey)
        } catch {
            XCTFail("Call Manager receivedAnswer() failed: \(error)")
            return
        }

        // Delay to see if we can catch all Ice candidates being sent...
        delay(interval: 1.0)

        // We've sent an answer, so we should see some Ice Candidates.
        // We don't care how many though. No need to reset the flag.
        expect(delegateCallee.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: .seconds(1))

        // Give Ice candidates to one another.

        do {
            Logger.debug("Test: Invoking receivedIceCandidates()...")
            try callManagerCaller?.receivedIceCandidates(sourceDevice: sourceDevice, callId: callId, candidates: delegateCallee.sentIceCandidates)
        } catch {
            XCTFail("Call Manager receivedIceCandidates() failed: \(error)")
            return
        }

        // We should get to the ringing state in each client.
        expect(delegateCaller.eventRemoteRingingInvoked).toEventually(equal(true), timeout: .seconds(2))
        expect(delegateCallee.eventLocalRingingInvoked).toEventually(equal(true), timeout: .seconds(1))

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

    enum GlareCondition {
        case winner
        case loser
        case equal
    }

    func glareTesting(scenario: GlareScenario, condition: GlareCondition) {
        Logger.debug("Test: Testing glare for scenario: \(scenario) and condition: \(condition)...")

        let delegateA = TestDelegate()
        var callManagerA = createCallManager(delegateA)
        expect(callManagerA).toNot(beNil())
        delegateA.expectedValue = 12345
        let aAddress: Int32 = 888888

        let delegateB = TestDelegate()
        var callManagerB = createCallManager(delegateB)
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

        expect(delegateA.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
        delegateA.startOutgoingCallInvoked = false
        let callIdAtoB = delegateA.recentCallId

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManagerA?.proceed(callId: callIdAtoB, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        expect(delegateA.shouldSendOfferInvoked).toEventually(equal(true), timeout: .seconds(1))
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

        expect(delegateB.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
        delegateB.startOutgoingCallInvoked = false
        let callIdBtoA = delegateB.recentCallId

        if scenario == .afterProceed {
            // Proceed on the B side.
            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerB?.proceed(callId: callIdBtoA, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateB.shouldSendOfferInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB.shouldSendOfferInvoked = false
        }

        // What condition should B be in?
        let callIdAtoBOverride: UInt64
        switch condition {
        case .winner:
            expect(callIdBtoA).to(beGreaterThan(1), description: "Test case not valid if incoming call-id can't be smaller than the active call-id.")
            callIdAtoBOverride = callIdBtoA - 1
        case .loser:
            expect(callIdAtoB).to(beLessThan(UINT64_MAX), description: "Test case not valid if incoming call-id can't be greater than the active call-id.")
            callIdAtoBOverride = callIdBtoA + 1
        case .equal:
            callIdAtoBOverride = callIdBtoA
        }

        // Give the offer from A to B.
        do {
            Logger.debug("Test: Invoking B.receivedOffer(A)...")
            let call = OpaqueCallData(value: delegateB.expectedValue, remote: aAddress)

            guard let opaque = delegateA.sentOfferOpaque else {
                XCTFail("No sentOfferOpaque detected!")
                return
            }

            try callManagerB?.receivedOffer(call: call, sourceDevice: sourceDevice, callId: callIdAtoBOverride, opaque: opaque, messageAgeSec: 0, callMediaType: .audioCall, localDevice: localDevice, isLocalDevicePrimary: true, senderIdentityKey: dummyLocalIdentityKey, receiverIdentityKey: dummyRemoteIdentityKey)
        } catch {
            XCTFail("Call Manager receivedOffer() failed: \(error)")
            return
        }

        switch condition {
        case .winner:
            expect(delegateB.eventReceivedOfferWithGlare).toEventually(equal(true), timeout: .seconds(1))
            delegateB.eventReceivedOfferWithGlare = false

            expect(delegateB.shouldSendBusyInvoked).to(equal(false))
            expect(delegateB.eventEndedRemoteGlare).to(equal(false))
        case .loser:
            expect(delegateB.eventEndedRemoteGlare).toEventually(equal(true), timeout: .seconds(1))
            delegateB.eventEndedRemoteGlare = false

            if scenario == .afterProceed {
                // Hangup is for the outgoing offer.
                expect(delegateB.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: .seconds(1))
                delegateB.shouldSendHangupNormalInvoked = false
            }

            expect(delegateB.eventReceivedOfferWhileActive).to(equal(false))
            expect(delegateB.shouldSendBusyInvoked).to(equal(false))
        case .equal:
            expect(delegateB.eventEndedRemoteGlare).toEventually(equal(true), timeout: .seconds(1))
            delegateB.eventEndedRemoteGlare = false

            if scenario == .afterProceed {
                // Hangup is for the outgoing offer.
                expect(delegateB.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: .seconds(1))
                delegateB.shouldSendHangupNormalInvoked = false
            }

            expect(delegateB.eventEndedGlareHandlingFailure).toEventually(equal(true), timeout: .seconds(1))
            delegateB.eventEndedGlareHandlingFailure = false

            expect(delegateB.shouldSendBusyInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB.shouldSendBusyInvoked = false

            expect(delegateB.eventReceivedOfferWhileActive).to(equal(false))
        }

        // Operation on B should be the same on A, no further testing required.

        // Release the Call Managers (but there still might be references in the delegates!).
        callManagerA = nil
        callManagerB = nil

        // See what clears up after closing the Call Manager...
        delay(interval: 1.0)

        Logger.debug("Test: Exiting test function...")
    }

    func testGlareWinnerBeforeProceed() {
        glareTesting(scenario: .beforeProceed, condition: .winner)
    }

    func testGlareWinnerAfterProceed() {
        glareTesting(scenario: .afterProceed, condition: .winner)
    }

    func testGlareLoserBeforeProceed() {
        glareTesting(scenario: .beforeProceed, condition: .loser)
    }

    func testGlareLoserAfterProceed() {
        glareTesting(scenario: .afterProceed, condition: .loser)
    }

    func testGlareEqualBeforeProceed() {
        glareTesting(scenario: .beforeProceed, condition: .equal)
    }

    func testGlareEqualAfterProceed() {
        glareTesting(scenario: .afterProceed, condition: .equal)
    }

    enum ReCallScenario {
        // The "callee" here is for the device receiving the recall.
        case calleeStillInCall
        case calleeReconnecting
    }

    func reCallTesting(scenario: ReCallScenario) {
        Logger.debug("Test: Testing ReCall for scenario: \(scenario)...")

        let delegateCaller = TestDelegate()
        let callManagerCaller = createCallManager(delegateCaller)
        expect(callManagerCaller).toNot(beNil())

        let delegateA = TestDelegate()
        var callManagerA = createCallManager(delegateA)
        expect(callManagerA).toNot(beNil())
        delegateA.expectedValue = 12345
        let aAddress: Int32 = 888888

        let delegateB = TestDelegate()
        var callManagerB = createCallManager(delegateB)
        expect(callManagerB).toNot(beNil())
        delegateB.expectedValue = 11111
        let bAddress: Int32 = 777777

        // Setup the automatic ICE flow for the call.
        delegateA.callManagerICE = [(callManagerB!, delegateB, 1)]
        delegateB.callManagerICE = [(callManagerA!, delegateA, 1)]
        delegateA.doAutomaticICE = true
        delegateB.doAutomaticICE = true

        // For now, these variables will be common to both Call Managers.
        let iceServers = [RTCIceServer(urlStrings: ["stun:stun.l.google.com:19302"])]
        let useTurnOnly = false
        let localDevice: UInt32 = 1
        let sourceDevice: UInt32 = 1

        let videoCaptureController = VideoCaptureController()

        // Get A and B into a call.
        do {
            let callA = OpaqueCallData(value: delegateA.expectedValue, remote: bAddress)
            try callManagerA?.placeCall(call: callA, callMediaType: .audioCall, localDevice: localDevice)
            expect(delegateA.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateA.startOutgoingCallInvoked = false

            let callIdAtoB = delegateA.recentCallId
            _ = try callManagerA?.proceed(callId: callIdAtoB, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
            expect(delegateA.shouldSendOfferInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateA.shouldSendOfferInvoked = false

            let callB = OpaqueCallData(value: delegateB.expectedValue, remote: aAddress)

            guard let opaque = delegateA.sentOfferOpaque else {
                XCTFail("No sentOfferOpaque detected!")
                return
            }

            try callManagerB?.receivedOffer(call: callB, sourceDevice: sourceDevice, callId: callIdAtoB, opaque: opaque, messageAgeSec: 0, callMediaType: .audioCall, localDevice: localDevice, isLocalDevicePrimary: true, senderIdentityKey: dummyLocalIdentityKey, receiverIdentityKey: dummyRemoteIdentityKey)
            expect(delegateB.startIncomingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB.startIncomingCallInvoked = false

            try callManagerB?.proceed(callId: callIdAtoB, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
            expect(delegateB.shouldSendAnswerInvoked).toEventually(equal(true), timeout: .seconds(2))
            delegateB.shouldSendAnswerInvoked = false
            expect(delegateB.recentCallId).to(equal(callIdAtoB))

            guard let opaqueAnswer = delegateB.sentAnswerOpaque else {
                XCTFail("No sentAnswerOpaque detected!")
                return
            }

            try callManagerA?.receivedAnswer(sourceDevice: sourceDevice, callId: callIdAtoB, opaque: opaqueAnswer, senderIdentityKey: dummyRemoteIdentityKey, receiverIdentityKey: dummyLocalIdentityKey)

            expect(delegateA.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateA.canSendICE = true
            delegateA.tryToSendIceCandidates(callId: callIdAtoB, destinationDeviceId: nil, candidates: [])

            expect(delegateB.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB.canSendICE = true
            delegateB.tryToSendIceCandidates(callId: callIdAtoB, destinationDeviceId: nil, candidates: [])

            expect(delegateA.eventRemoteRingingInvoked).toEventually(equal(true), timeout: .seconds(5))
            delegateA.eventRemoteRingingInvoked = false

            expect(delegateB.eventLocalRingingInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB.eventLocalRingingInvoked = false

            delay(interval: 1.0)

            try callManagerB?.accept(callId: callIdAtoB)

            // Connected?
            expect(delegateB.eventLocalConnectedInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB.eventLocalConnectedInvoked = false;

            expect(delegateA.eventRemoteConnectedInvoked).toEventually(equal(true), timeout: .seconds(2))
            delegateA.eventRemoteConnectedInvoked = false;

            // We should see a hangup/accepted from the callee here, who was the caller in this case.
            expect(delegateA.shouldSendHangupAcceptedInvoked).toEventually(equal(true), timeout: .seconds(1))

            // Neither side should have ended the call.
            expect(delegateA.eventGeneralEnded).to(equal(false))
            expect(delegateB.eventGeneralEnded).to(equal(false))

            // Actual recall scenario starts now...
            delegateA.resetIceHandlingState()
            delegateB.resetIceHandlingState()

            // End B quietly (no hangup) to simulate B ending before A is aware of it.
            callManagerB?.drop(callId: callIdAtoB)

            expect(delegateB.eventEndedDropped).toEventually(equal(true), timeout: .seconds(1))
            delegateB.eventEndedDropped = false
            delegateB.eventGeneralEnded = false

            if scenario == .calleeReconnecting {
              // Give some time to get to the reconnecting state. There will be 10 seconds after this before ICE Failure.
              expect(delegateA.eventReconnecting).toEventually(equal(true), timeout: .seconds(10))
              delegateA.eventReconnecting = false
            }

            // Start the new call from B to A.
            let callB2 = OpaqueCallData(value: delegateB.expectedValue, remote: aAddress)
            try callManagerB?.placeCall(call: callB2, callMediaType: .audioCall, localDevice: localDevice)
            expect(delegateB.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB.startOutgoingCallInvoked = false

            let callIdB2toA = delegateB.recentCallId
            _ = try callManagerB?.proceed(callId: callIdB2toA, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
            expect(delegateB.shouldSendOfferInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB.shouldSendOfferInvoked = false

            let callA2 = OpaqueCallData(value: delegateA.expectedValue, remote: bAddress)

            guard let opaque = delegateB.sentOfferOpaque else {
                XCTFail("No sentOfferOpaque detected!")
                return
            }

            // Provide the offer to A for the new call.
            try callManagerA?.receivedOffer(call: callA2, sourceDevice: sourceDevice, callId: callIdB2toA, opaque: opaque, messageAgeSec: 0, callMediaType: .audioCall, localDevice: localDevice, isLocalDevicePrimary: true, senderIdentityKey: dummyLocalIdentityKey, receiverIdentityKey: dummyRemoteIdentityKey)

            // Existing call should end.
            expect(delegateA.eventEndedRemoteReCall).toEventually(equal(true), timeout: .seconds(1))
            delegateA.eventEndedRemoteReCall = false
            delegateA.eventGeneralEnded = false

            // New call should be started.
            expect(delegateA.startIncomingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateA.startIncomingCallInvoked = false

            // Simulate getting in to the new call (like before, but this time B is calling A).
            try callManagerA?.proceed(callId: callIdB2toA, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
            expect(delegateA.shouldSendAnswerInvoked).toEventually(equal(true), timeout: .seconds(2))
            delegateA.shouldSendAnswerInvoked = false
            expect(delegateA.recentCallId).to(equal(callIdB2toA))

            guard let opaqueAnswer2 = delegateA.sentAnswerOpaque else {
                XCTFail("No sentAnswerOpaque detected!")
                return
            }

            try callManagerB?.receivedAnswer(sourceDevice: sourceDevice, callId: callIdB2toA, opaque: opaqueAnswer2, senderIdentityKey: dummyRemoteIdentityKey, receiverIdentityKey: dummyLocalIdentityKey)

            expect(delegateB.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB.canSendICE = true
            delegateB.tryToSendIceCandidates(callId: callIdB2toA, destinationDeviceId: nil, candidates: [])

            expect(delegateA.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateA.canSendICE = true
            delegateA.tryToSendIceCandidates(callId: callIdB2toA, destinationDeviceId: nil, candidates: [])

            expect(delegateB.eventRemoteRingingInvoked).toEventually(equal(true), timeout: .seconds(5))
            expect(delegateA.eventLocalRingingInvoked).toEventually(equal(true), timeout: .seconds(1))

            delay(interval: 1.0)

            try callManagerA?.accept(callId: callIdB2toA)

            // Connected?
            expect(delegateA.eventLocalConnectedInvoked).toEventually(equal(true), timeout: .seconds(1))
            expect(delegateB.eventRemoteConnectedInvoked).toEventually(equal(true), timeout: .seconds(2))

            // We should see a hangup/accepted from the callee here, who was the caller in this case.
            expect(delegateB.shouldSendHangupAcceptedInvoked).toEventually(equal(true), timeout: .seconds(1))

            // Neither side should have ended the new call.
            expect(delegateB.eventGeneralEnded).to(equal(false))
            expect(delegateA.eventGeneralEnded).to(equal(false))

            delay(interval: 1.0)
        } catch {
           XCTFail("Scenario failed: \(error)")
           return
       }

        // Release the Call Managers (but there still might be references in the delegates!).
        callManagerA = nil
        callManagerB = nil

        // See what clears up after closing the Call Manager...
        delay(interval: 1.0)

        Logger.debug("Test: Exiting test function...")
    }

    func testRecallStillInCall() {
        reCallTesting(scenario: .calleeStillInCall)
    }

    func testRecallReconnecting() {
        reCallTesting(scenario: .calleeReconnecting)
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
        var callManagerCaller = createCallManager(delegateCaller)
        expect(callManagerCaller).toNot(beNil())
        delegateCaller.expectedValue = 12345
        let callerAddress: Int32 = 888888
        let callerDevice: UInt32 = 1

        let videoCaptureController = VideoCaptureController()

        // Build the callee structures, the Call Manager and delegate for each.
        var calleeDevices: [(callManager: CallManager<OpaqueCallData, TestDelegate>, delegate: TestDelegate, deviceId: UInt32)] = []
        for i in 1...calleeDeviceCount {
            let delegate = TestDelegate()
            let callManager = createCallManager(delegate)!
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

        // An extra Call Manager for some scenarios (such as busy).
        let delegateExtra = TestDelegate()
        var callManagerExtra = createCallManager(delegateExtra)
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
                expect(busyCallee.delegate.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
                busyCallee.delegate.startOutgoingCallInvoked = false

                let callId = busyCallee.delegate.recentCallId
                _ = try busyCallee.callManager.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
                expect(busyCallee.delegate.shouldSendOfferInvoked).toEventually(equal(true), timeout: .seconds(1))
                busyCallee.delegate.shouldSendOfferInvoked = false

                let callExtra = OpaqueCallData(value: delegateExtra.expectedValue, remote: calleeAddress)

                guard let opaqueOffer = busyCallee.delegate.sentOfferOpaque else {
                    XCTFail("No sentOfferOpaque detected!")
                    return
                }

                try callManagerExtra?.receivedOffer(call: callExtra, sourceDevice: busyCallee.deviceId, callId: callId, opaque: opaqueOffer, messageAgeSec: 0, callMediaType: .audioCall, localDevice: extraDevice, isLocalDevicePrimary: true, senderIdentityKey: dummyRemoteIdentityKey, receiverIdentityKey: dummyLocalIdentityKey)

                expect(busyCallee.delegate.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: .seconds(1))
                busyCallee.delegate.canSendICE = true
                busyCallee.delegate.tryToSendIceCandidates(callId: callId, destinationDeviceId: nil, candidates: [])

                expect(delegateExtra.startIncomingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
                delegateExtra.startIncomingCallInvoked = false

                try callManagerExtra?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)

                expect(delegateExtra.shouldSendAnswerInvoked).toEventually(equal(true), timeout: .seconds(2))
                delegateExtra.shouldSendAnswerInvoked = false
                expect(delegateExtra.recentCallId).to(equal(callId))

                guard let opaqueAnswer = delegateExtra.sentAnswerOpaque else {
                    XCTFail("No sentAnswerOpaque detected!")
                    return
                }

                try busyCallee.callManager.receivedAnswer(sourceDevice: extraDevice, callId: callId, opaque: opaqueAnswer, senderIdentityKey: dummyLocalIdentityKey, receiverIdentityKey: dummyRemoteIdentityKey)

                expect(delegateExtra.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: .seconds(1))

                expect(busyCallee.delegate.eventRemoteRingingInvoked).toEventually(equal(true), timeout: .seconds(2))
                expect(delegateExtra.eventLocalRingingInvoked).toEventually(equal(true), timeout: .seconds(1))

                try callManagerExtra?.accept(callId: callId)

                // Connected?
                expect(busyCallee.delegate.eventRemoteConnectedInvoked).toEventually(equal(true), timeout: .seconds(2))
                expect(delegateExtra.eventLocalConnectedInvoked).toEventually(equal(true), timeout: .seconds(1))

                // For fun, we should see a hangup/accepted from the callee here, who was the caller in this case.
                expect(busyCallee.delegate.shouldSendHangupAcceptedInvoked).toEventually(equal(true), timeout: .seconds(1))

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

            expect(delegateCaller.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateCaller.startOutgoingCallInvoked = false

            // This may not be proper...
            let callId = delegateCaller.recentCallId

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerCaller?.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateCaller.shouldSendOfferInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateCaller.shouldSendOfferInvoked = false

            // We sent the offer! Let's give it to our callees.
            do {
                // Give the offer to all callees at the same time (simulate replication).
                for element in calleeDevices {
                    // Define some CallData for simulation. This is defined in a block
                    // so that we validate that it is retained correctly and accessible
                    // outside this block.
                    let call = OpaqueCallData(value: element.delegate.expectedValue, remote: callerAddress)

                    guard let opaque = delegateCaller.sentOfferOpaque else {
                        XCTFail("No sentOfferOpaque detected!")
                        return
                    }

                    Logger.debug("Test: Invoking receivedOffer()...")

                    // @note We are specifying multiple devices as primary, but it shouldn't
                    // matter for this type of testing.
                    try element.callManager.receivedOffer(call: call, sourceDevice: callerDevice, callId: callId, opaque: opaque, messageAgeSec: 0, callMediaType: .audioCall, localDevice: element.deviceId, isLocalDevicePrimary: true, senderIdentityKey: dummyRemoteIdentityKey, receiverIdentityKey: dummyLocalIdentityKey)
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

                    expect(element.delegate.startIncomingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
                    element.delegate.startIncomingCallInvoked = false

                    Logger.debug("Test: Invoking proceed()...")
                    _ = try element.callManager.proceed(callId: callId, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
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

                            expect(element.delegate.shouldSendBusyInvoked).toEventually(equal(true), timeout: .seconds(2))
                            element.delegate.shouldSendBusyInvoked = false

                            expect(element.delegate.recentBusyCallId).to(equal(callId))

                            Logger.debug("Test: Invoking receivedBusy()...")
                            try callManagerCaller?.receivedBusy(sourceDevice: element.deviceId, callId: callId)

                            continue
                        }
                    }

                    expect(element.delegate.shouldSendAnswerInvoked).toEventually(equal(true), timeout: .seconds(2))
                    element.delegate.shouldSendAnswerInvoked = false

                    expect(element.delegate.recentCallId).to(equal(callId))

                    guard let opaque = element.delegate.sentAnswerOpaque else {
                        XCTFail("No sentAnswerOpaque detected!")
                        return
                    }

                    Logger.debug("Test: Invoking receivedAnswer()...")
                    try callManagerCaller?.receivedAnswer(sourceDevice: element.deviceId, callId: callId, opaque: opaque, senderIdentityKey: dummyLocalIdentityKey, receiverIdentityKey: dummyRemoteIdentityKey)
                }
            } catch {
                XCTFail("Call Manager receivedAnswer() failed: \(error)")
                return
            }

            if scenario != .calleeBusy {
                // The caller should get to ringing state when the first connection is made with
                // any of the callees.
                expect(delegateCaller.eventRemoteRingingInvoked).toEventually(equal(true), timeout: .seconds(2))
                delegateCaller.eventRemoteRingingInvoked = false

                // Now make sure all the callees get to a ringing state.
                for element in calleeDevices {
                    expect(element.delegate.eventLocalRingingInvoked).toEventually(equal(true), timeout: .seconds(2))
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

                expect(delegateCaller.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: .seconds(2))
                delegateCaller.shouldSendHangupNormalInvoked = false

                // Now make sure all the callees get hungup.
                for element in calleeDevices {
                    do {
                        try element.callManager.receivedHangup(sourceDevice: delegateCaller.localDevice, callId: callId, hangupType: .normal, deviceId: 0)
                    } catch {
                        XCTFail("Call Manager receivedHangup(caller) failed: \(error)")
                        return
                    }

                    expect(element.delegate.eventEndedRemoteHangup).toEventually(equal(true), timeout: .seconds(2))
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
                expect(decliningCallee.delegate.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: .seconds(2))
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
                expect(delegateCaller.shouldSendHangupDeclinedInvoked).toEventually(equal(true), timeout: .seconds(2))
                delegateCaller.shouldSendHangupDeclinedInvoked = false

                // Now make sure all the callees get proper hangup indication.
                for element in calleeDevices {
                    do {
                        try element.callManager.receivedHangup(sourceDevice: delegateCaller.localDevice, callId: callId, hangupType: .declined, deviceId: delegateCaller.hangupDeviceId ?? 0)
                    } catch {
                        XCTFail("Call Manager receivedHangup(caller) failed: \(error)")
                        return
                    }

                    // Skip over the declining callee...
                    if element.deviceId != decliningCallee.deviceId {
                        expect(element.delegate.eventEndedRemoteHangupDeclined).toEventually(equal(true), timeout: .seconds(2))
                        element.delegate.eventEndedRemoteHangupDeclined = false
                    }
                }

            case .calleeBusy:
                Logger.debug("Scenario: The first callee is busy.")

                // We have given Busy to the Caller and all other devices have given
                // an Answer.

                // Caller should end with remote busy
                expect(delegateCaller.eventEndedRemoteBusy).toEventually(equal(true), timeout: .seconds(1))
                delegateCaller.eventEndedRemoteBusy = false

                // Caller should send out a hangup/busy.
                expect(delegateCaller.shouldSendHangupBusyInvoked).toEventually(equal(true), timeout: .seconds(1))
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

                        expect(element.delegate.eventReceivedOfferWhileActive).toEventually(equal(true), timeout: .seconds(2))
                        element.delegate.eventReceivedOfferWhileActive = false

                        // The busy callee should not have ended their existing call.
                        expect(element.delegate.eventGeneralEnded).to(equal(false))

                        continue
                    }

                    expect(element.delegate.eventEndedRemoteHangupBusy).toEventually(equal(true), timeout: .seconds(1))
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

                // The connect message would go RTP data.

                // Both the callee and caller should be in a connected state.
                expect(acceptingCallee.delegate.eventLocalConnectedInvoked).toEventually(equal(true), timeout: .seconds(1))
                acceptingCallee.delegate.eventLocalConnectedInvoked = false
                expect(delegateCaller.eventRemoteConnectedInvoked).toEventually(equal(true), timeout: .seconds(1))
                delegateCaller.eventRemoteConnectedInvoked = false

                // The caller will send hangup/accepted.
                expect(delegateCaller.shouldSendHangupAcceptedInvoked).toEventually(equal(true), timeout: .seconds(1))
                delegateCaller.shouldSendHangupAcceptedInvoked = false

                // Now make sure all the callees get proper hangup indication.
                for element in calleeDevices {
                    do {
                        try element.callManager.receivedHangup(sourceDevice: delegateCaller.localDevice, callId: callId, hangupType: .accepted, deviceId: delegateCaller.hangupDeviceId ?? 0)
                    } catch {
                        XCTFail("Call Manager receivedHangup(caller) failed: \(error)")
                        return
                    }

                    // Skip over the accepting callee...
                    if element.deviceId != acceptingCallee.deviceId {
                        expect(element.delegate.eventEndedRemoteHangupAccepted).toEventually(equal(true), timeout: .seconds(1))
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

                expect(delegateCaller.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: .seconds(1))
                delegateCaller.shouldSendHangupNormalInvoked = false

                // Give the hangup to the callee.
                do {
                    Logger.debug("Test: Invoking hangup(callee)...")
                    _ = try acceptingCallee.callManager.receivedHangup(sourceDevice: callerDevice, callId: callId, hangupType: .normal, deviceId: 0)
                } catch {
                    XCTFail("Call Manager hangup(callee) failed: \(error)")
                    return
                }

                expect(acceptingCallee.delegate.eventEndedRemoteHangup).toEventually(equal(true), timeout: .seconds(1))
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
        multiRingTesting(calleeDeviceCount: 2, loopIterations: 1, scenario: .callerEnds)
    }

    func testMultiRingDeclined() {
        multiRingTesting(calleeDeviceCount: 2, loopIterations: 1, scenario: .calleeDeclines)
    }

    func testMultiRingBusy() {
        multiRingTesting(calleeDeviceCount: 2, loopIterations: 1, scenario: .calleeBusy)
    }

    func testMultiRingAccepted() {
        multiRingTesting(calleeDeviceCount: 2, loopIterations: 1, scenario: .calleeAccepts)
    }

    enum MultiRingGlareScenario {
        case primaryWinner    /// A1 calls B1 and B2; at the same time, B1 calls A1; B1 is the winner
        case primaryLoser     /// A1 calls B1 and B2; at the same time, B1 calls A1; B1 is the loser
        case primaryEqual     /// A1 calls B1 and B2; at the same time, B1 calls A1; call-ids are equal, B1 failure case with busy, B2 ends too
        case differentDevice  /// A1 is in call with B1; A2 calls B, should ring on B2
    }

    func multiRingGlareTesting(scenario: MultiRingGlareScenario) {
        Logger.debug("Test: Testing multi-ring glare for scenario: \(scenario)...")

        let aAddress: Int32 = 888888

        let delegateA1 = TestDelegate()
        var callManagerA1 = createCallManager(delegateA1)
        expect(callManagerA1).toNot(beNil())
        delegateA1.expectedValue = 12345
        let a1Device: UInt32 = 1

        let delegateA2 = TestDelegate()
        var callManagerA2 = createCallManager(delegateA2)
        expect(callManagerA2).toNot(beNil())
        delegateA2.expectedValue = 54321
        let a2Device: UInt32 = 2

        let bAddress: Int32 = 111111

        let delegateB1 = TestDelegate()
        var callManagerB1 = createCallManager(delegateB1)
        expect(callManagerB1).toNot(beNil())
        delegateB1.expectedValue = 11111
        let b1Device: UInt32 = 1

        let delegateB2 = TestDelegate()
        var callManagerB2 = createCallManager(delegateB2)
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

        expect(delegateA1.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
        delegateA1.startOutgoingCallInvoked = false
        let callIdA1toB = delegateA1.recentCallId

        do {
            Logger.debug("Test: Invoking proceed()...")
            _ = try callManagerA1?.proceed(callId: callIdA1toB, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
        } catch {
            XCTFail("Call Manager proceed() failed: \(error)")
            return
        }

        expect(delegateA1.shouldSendOfferInvoked).toEventually(equal(true), timeout: .seconds(2))
        delegateA1.shouldSendOfferInvoked = false

        if scenario == .primaryWinner || scenario == .primaryLoser || scenario == .primaryEqual {
            // @note Not using A2 for this case.

            // B1 starts to call A.
            do {
                Logger.debug("Test:B calls A...")
                let call = OpaqueCallData(value: delegateB1.expectedValue, remote: aAddress)
                try callManagerB1?.placeCall(call: call, callMediaType: .audioCall, localDevice: b1Device)
            } catch {
                XCTFail("Call Manager call() failed: \(error)")
                return
            }

            expect(delegateB1.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB1.startOutgoingCallInvoked = false
            let callIdB1toA = delegateB1.recentCallId

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerB1?.proceed(callId: callIdB1toA, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateB1.shouldSendOfferInvoked).toEventually(equal(true), timeout: .seconds(2))
            delegateB1.shouldSendOfferInvoked = false

            // Override the call-id for B's perspective to follow the scenario.
            var callIdA1toBOverride = callIdA1toB
            var callIdB1toAOverride = callIdB1toA

            if scenario == .primaryWinner {
                // B1 should win.
                if callIdB1toA <= callIdA1toB {
                    // Make sure B wins on both sides.
                    expect(callIdB1toA).to(beGreaterThan(1), description: "Test case not valid, try again.")
                    callIdA1toBOverride = callIdB1toA - 1
                    expect(callIdA1toB).to(beLessThan(UINT64_MAX), description: "Test case not valid, try again.")
                    callIdB1toAOverride = callIdA1toB + 1
                }
            } else if scenario == .primaryLoser {
                // B1 should lose.
                if callIdB1toA >= callIdA1toB {
                    // Make sure B loses on both sides.
                    expect(callIdB1toA).to(beLessThan(UINT64_MAX), description: "Test case not valid, try again.")
                    callIdA1toBOverride = callIdB1toA + 1
                    expect(callIdA1toB).to(beGreaterThan(1), description: "Test case not valid, try again.")
                    callIdB1toAOverride = callIdA1toB - 1
                }
            } else {
                // When sending to B, set with the call-id B is actually using.
                callIdA1toBOverride = callIdB1toA
                // When sending to A, set with the call-id A is actually using.
                callIdB1toAOverride = callIdA1toB
            }

            // Give the offer from A1 to B1 & B2.
            do {
                Logger.debug("Test: Invoking B*.receivedOffer(A1)...")
                let callA1toB1 = OpaqueCallData(value: delegateB1.expectedValue, remote: aAddress)

                guard let opaque = delegateA1.sentOfferOpaque else {
                    XCTFail("No sentOfferOpaque detected!")
                    return
                }

                try callManagerB1?.receivedOffer(call: callA1toB1, sourceDevice: a1Device, callId: callIdA1toBOverride, opaque: opaque, messageAgeSec: 0, callMediaType: .audioCall, localDevice: b1Device, isLocalDevicePrimary: true, senderIdentityKey: dummyLocalIdentityKey, receiverIdentityKey: dummyRemoteIdentityKey)
                let callA1toB2 = OpaqueCallData(value: delegateB2.expectedValue, remote: aAddress)
                try callManagerB2?.receivedOffer(call: callA1toB2, sourceDevice: a1Device, callId: callIdA1toBOverride, opaque: opaque, messageAgeSec: 0, callMediaType: .audioCall, localDevice: b2Device, isLocalDevicePrimary: false, senderIdentityKey: dummyLocalIdentityKey, receiverIdentityKey: dummyRemoteIdentityKey)
            } catch {
                XCTFail("Call Manager receivedOffer() failed: \(error)")
                return
            }

            // Give the offer from B1 to A1.
            do {
                Logger.debug("Test: Invoking A1.receivedOffer(B1)...")
                let call = OpaqueCallData(value: delegateA1.expectedValue, remote: bAddress)

                guard let opaque = delegateB1.sentOfferOpaque else {
                    XCTFail("No sentOfferOpaque detected!")
                    return
                }

                try callManagerA1?.receivedOffer(call: call, sourceDevice: b1Device, callId: callIdB1toAOverride, opaque: opaque, messageAgeSec: 0, callMediaType: .audioCall, localDevice: a1Device, isLocalDevicePrimary: true, senderIdentityKey: dummyLocalIdentityKey, receiverIdentityKey: dummyRemoteIdentityKey)
            } catch {
                XCTFail("Call Manager receivedOffer() failed: \(error)")
                return
            }

            // B2 behavior:

            expect(delegateB2.startIncomingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB2.startIncomingCallInvoked = false

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerB2?.proceed(callId: callIdA1toBOverride, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateB2.shouldSendAnswerInvoked).toEventually(equal(true), timeout: .seconds(2))
            delegateB2.shouldSendAnswerInvoked = false

            // A1 behavior:

            if scenario == .primaryWinner {
                // A should lose.
                expect(delegateA1.eventEndedRemoteGlare).toEventually(equal(true), timeout: .seconds(1))
                delegateA1.eventEndedRemoteGlare = false

                // Hangup is for the outgoing offer.
                expect(delegateA1.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: .seconds(1))
                delegateA1.shouldSendHangupNormalInvoked = false

                expect(delegateA1.eventReceivedOfferWhileActive).to(equal(false))
                expect(delegateA1.shouldSendBusyInvoked).to(equal(false))
            } else if scenario == .primaryLoser {
                // A should win.
                expect(delegateA1.eventReceivedOfferWithGlare).toEventually(equal(true), timeout: .seconds(2))
                delegateA1.eventReceivedOfferWithGlare = false

                expect(delegateA1.shouldSendBusyInvoked).to(equal(false))
                expect(delegateA1.eventEndedRemoteGlare).to(equal(false))
            } else {
                expect(delegateA1.eventEndedRemoteGlare).toEventually(equal(true), timeout: .seconds(2))
                delegateA1.eventEndedRemoteGlare = false

                // Hangup is for the outgoing offer.
                expect(delegateA1.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: .seconds(1))
                delegateA1.shouldSendHangupNormalInvoked = false

                expect(delegateA1.eventEndedGlareHandlingFailure).toEventually(equal(true), timeout: .seconds(1))
                delegateA1.eventEndedGlareHandlingFailure = false

                expect(delegateA1.shouldSendBusyInvoked).toEventually(equal(true), timeout: .seconds(1))
                delegateA1.shouldSendBusyInvoked = false

                expect(delegateA1.eventReceivedOfferWhileActive).to(equal(false))
            }

            // B1 behavior:

            if scenario == .primaryWinner {
                expect(delegateB1.eventReceivedOfferWithGlare).toEventually(equal(true), timeout: .seconds(1))
                delegateB1.eventReceivedOfferWithGlare = false

                expect(delegateB1.shouldSendBusyInvoked).to(equal(false))
                expect(delegateB1.eventEndedRemoteGlare).to(equal(false))
            } else if scenario == .primaryLoser {
                expect(delegateB1.eventEndedRemoteGlare).toEventually(equal(true), timeout: .seconds(1))
                delegateB1.eventEndedRemoteGlare = false

                // Hangup is for the outgoing offer.
                expect(delegateB1.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: .seconds(1))
                delegateB1.shouldSendHangupNormalInvoked = false

                expect(delegateB1.eventReceivedOfferWhileActive).to(equal(false))
                expect(delegateB1.shouldSendBusyInvoked).to(equal(false))
            } else {
                expect(delegateB1.eventEndedRemoteGlare).toEventually(equal(true), timeout: .seconds(1))
                delegateB1.eventEndedRemoteGlare = false

                // Hangup is for the outgoing offer.
                expect(delegateB1.shouldSendHangupNormalInvoked).toEventually(equal(true), timeout: .seconds(2))
                delegateB1.shouldSendHangupNormalInvoked = false

                expect(delegateB1.eventEndedGlareHandlingFailure).toEventually(equal(true), timeout: .seconds(1))
                delegateB1.eventEndedGlareHandlingFailure = false

                expect(delegateB1.shouldSendBusyInvoked).toEventually(equal(true), timeout: .seconds(1))
                delegateB1.shouldSendBusyInvoked = false

                expect(delegateB1.eventReceivedOfferWhileActive).to(equal(false))
            }

            // Reset A1 general detection (to check later).
            delegateA1.generalInvocationDetected = false

            // Reset B1 general detection (to check later).
            delegateB1.generalInvocationDetected = false

            // Now look at consequences and proper cleanup.

            if scenario == .primaryWinner {
                // Deliver Hangup from A1 to B.
                delegateB2.eventEndedRemoteHangup = false

                do {
                    Logger.debug("Test: Invoking B*.receivedHangup(A1)...")
                    try callManagerB1?.receivedHangup(sourceDevice: a1Device, callId: callIdA1toBOverride, hangupType: .normal, deviceId: 0)
                    try callManagerB2?.receivedHangup(sourceDevice: a1Device, callId: callIdA1toBOverride, hangupType: .normal, deviceId: 0)
                } catch {
                    XCTFail("Call Manager receivedHangup() failed: \(error)")
                    return
                }

                expect(delegateB2.eventEndedRemoteHangup).toEventually(equal(true), timeout: .seconds(2))
                delegateB2.eventEndedRemoteHangup = false

                // B1 shouldn't have done anything.
                expect(delegateB1.generalInvocationDetected).to(equal(false))
            } else if scenario == .primaryLoser {
                // Deliver Hangup from B1 to A.
                do {
                    Logger.debug("Test: Invoking A1.receivedHangup(B1)...")
                    try callManagerA1?.receivedHangup(sourceDevice: b1Device, callId: callIdB1toAOverride, hangupType: .normal, deviceId: 0)
                } catch {
                    XCTFail("Call Manager receivedHangup() failed: \(error)")
                    return
                }

                // A1 shouldn't have done anything.
                expect(delegateA1.generalInvocationDetected).to(equal(false))
            } else {
                // Deliver Hangup from A1 to B.
                delegateB2.eventEndedRemoteHangup = false

                do {
                    Logger.debug("Test: Invoking B*.receivedHangup(A1)...")
                    try callManagerB1?.receivedHangup(sourceDevice: a1Device, callId: callIdA1toBOverride, hangupType: .normal, deviceId: 0)
                    try callManagerB2?.receivedHangup(sourceDevice: a1Device, callId: callIdA1toBOverride, hangupType: .normal, deviceId: 0)
                } catch {
                    XCTFail("Call Manager receivedHangup() failed: \(error)")
                    return
                }

                expect(delegateB2.eventEndedRemoteHangup).toEventually(equal(true), timeout: .seconds(2))
                delegateB2.eventEndedRemoteHangup = false

                // Reset B2 general detection (to check later).
                delegateB2.generalInvocationDetected = false

                // Deliver Busy from A1 to B.
                do {
                    Logger.debug("Test: Invoking B*.receivedBusy(A1)...")
                    try callManagerB1?.receivedBusy(sourceDevice: a1Device, callId: callIdB1toA)
                    try callManagerB2?.receivedBusy(sourceDevice: a1Device, callId: callIdB1toA)
                } catch {
                    XCTFail("Call Manager receivedBusy() failed: \(error)")
                    return
                }

                // Deliver Hangup from B1 to A.
                do {
                    Logger.debug("Test: Invoking A1.receivedHangup(B1)...")
                    try callManagerA1?.receivedHangup(sourceDevice: b1Device, callId: callIdB1toAOverride, hangupType: .normal, deviceId: 0)
                } catch {
                    XCTFail("Call Manager receivedHangup() failed: \(error)")
                    return
                }

                // Deliver Busy from B1 to A.
                do {
                    Logger.debug("Test: Invoking A*.receivedBusy(B1)...")
                    try callManagerA1?.receivedBusy(sourceDevice: a1Device, callId: callIdA1toB)
                } catch {
                    XCTFail("Call Manager receivedBusy() failed: \(error)")
                    return
                }

                // A1 shouldn't have done anything.
                expect(delegateA1.generalInvocationDetected).to(equal(false))

                // B1 shouldn't have done anything.
                expect(delegateB1.generalInvocationDetected).to(equal(false))

                // B2 shouldn't have done anything.
                expect(delegateB2.generalInvocationDetected).to(equal(false))
            }
        } else if scenario == .differentDevice {
            // @todo Place A/B connection establishment code in its own class.

            // Get A1 and B1 in to a call.

            // Give the offer from A1 to B1 & B2.
            do {
                Logger.debug("Test: Invoking B*.receivedOffer(A1)...")
                let callA1toB1 = OpaqueCallData(value: delegateB1.expectedValue, remote: aAddress)

                guard let opaque = delegateA1.sentOfferOpaque else {
                    XCTFail("No sentOfferOpaque detected!")
                    return
                }

                try callManagerB1?.receivedOffer(call: callA1toB1, sourceDevice: a1Device, callId: callIdA1toB, opaque: opaque, messageAgeSec: 0, callMediaType: .audioCall, localDevice: b1Device, isLocalDevicePrimary: true, senderIdentityKey: dummyRemoteIdentityKey, receiverIdentityKey: dummyLocalIdentityKey)
                let callA1toB2 = OpaqueCallData(value: delegateB2.expectedValue, remote: aAddress)
                try callManagerB2?.receivedOffer(call: callA1toB2, sourceDevice: a1Device, callId: callIdA1toB, opaque: opaque, messageAgeSec: 0, callMediaType: .audioCall, localDevice: b2Device, isLocalDevicePrimary: false, senderIdentityKey: dummyRemoteIdentityKey, receiverIdentityKey: dummyLocalIdentityKey)
            } catch {
                XCTFail("Call Manager receivedOffer() failed: \(error)")
                return
            }

            expect(delegateB1.startIncomingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB1.startIncomingCallInvoked = false

            expect(delegateB2.startIncomingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB2.startIncomingCallInvoked = false

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerB1?.proceed(callId: callIdA1toB, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
                _ = try callManagerB2?.proceed(callId: callIdA1toB, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateB1.shouldSendAnswerInvoked).toEventually(equal(true), timeout: .seconds(2))
            delegateB1.shouldSendAnswerInvoked = false
            expect(delegateB2.shouldSendAnswerInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB2.shouldSendAnswerInvoked = false

            // We also expect ICE candidates to be ready for A1 and B1.
            expect(delegateA1.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateA1.shouldSendIceCandidatesInvoked = false
            expect(delegateB1.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB1.shouldSendIceCandidatesInvoked = false

            // Send answer and candidates between A1 and B1.
            do {
                Logger.debug("Test: Invoking received*()...")

                guard let opaque = delegateB1.sentAnswerOpaque else {
                    XCTFail("No sentAnswerOpaque detected!")
                    return
                }

                try callManagerA1?.receivedAnswer(sourceDevice: b1Device, callId: callIdA1toB, opaque: opaque, senderIdentityKey: dummyLocalIdentityKey, receiverIdentityKey: dummyRemoteIdentityKey)
                try callManagerB1?.receivedIceCandidates(sourceDevice: a1Device, callId: callIdA1toB, candidates: delegateA1.sentIceCandidates)
                try callManagerA1?.receivedIceCandidates(sourceDevice: b1Device, callId: callIdA1toB, candidates: delegateB1.sentIceCandidates)
            } catch {
                XCTFail("Call Manager received*() failed: \(error)")
                return
            }

            // Should get to ringing.
            expect(delegateA1.eventRemoteRingingInvoked).toEventually(equal(true), timeout: .seconds(2))
            delegateA1.eventRemoteRingingInvoked = false
            expect(delegateB1.eventLocalRingingInvoked).toEventually(equal(true), timeout: .seconds(1))
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
            expect(delegateA1.eventRemoteConnectedInvoked).toEventually(equal(true), timeout: .seconds(2))
            delegateA1.eventRemoteConnectedInvoked = false
            expect(delegateB1.eventLocalConnectedInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB1.eventLocalConnectedInvoked = false

            // Should get hangup/Accepted to be sent to B.
            expect(delegateA1.shouldSendHangupAcceptedInvoked).toEventually(equal(true), timeout: .seconds(1))
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
            expect(delegateB2.eventEndedRemoteHangupAccepted).toEventually(equal(true), timeout: .seconds(1))
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

            expect(delegateA2.startOutgoingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateA2.startOutgoingCallInvoked = false
            let callIdA2toB = delegateA2.recentCallId

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerA2?.proceed(callId: callIdA2toB, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateA2.shouldSendOfferInvoked).toEventually(equal(true), timeout: .seconds(2))
            delegateA2.shouldSendOfferInvoked = false

            // Give the offer from A2 to B1 & B2.
            do {
                Logger.debug("Test: Invoking B*.receivedOffer(A2)...")
                let callA2toB1 = OpaqueCallData(value: delegateB1.expectedValue, remote: aAddress)

                guard let opaque = delegateA2.sentOfferOpaque else {
                    XCTFail("No sentOfferOpaque detected!")
                    return
                }

                try callManagerB1?.receivedOffer(call: callA2toB1, sourceDevice: a2Device, callId: callIdA2toB, opaque: opaque, messageAgeSec: 0, callMediaType: .audioCall, localDevice: b1Device, isLocalDevicePrimary: true, senderIdentityKey: dummyRemoteIdentityKey, receiverIdentityKey: dummyLocalIdentityKey)
                let callA2toB2 = OpaqueCallData(value: delegateB2.expectedValue, remote: aAddress)
                try callManagerB2?.receivedOffer(call: callA2toB2, sourceDevice: a2Device, callId: callIdA2toB, opaque: opaque, messageAgeSec: 0, callMediaType: .audioCall, localDevice: b2Device, isLocalDevicePrimary: false, senderIdentityKey: dummyRemoteIdentityKey, receiverIdentityKey: dummyLocalIdentityKey)
            } catch {
                XCTFail("Call Manager receivedOffer() failed: \(error)")
                return
            }

            // B1 behavior:

            expect(delegateB1.eventReceivedOfferWhileActive).toEventually(equal(true), timeout: .seconds(2))
            delegateB1.eventReceivedOfferWhileActive = false

            // Busy is for the incoming offer.
            expect(delegateB1.shouldSendBusyInvoked).toEventually(equal(true), timeout: .seconds(2))
            delegateB1.shouldSendBusyInvoked = false

            // B2 behavior:

            expect(delegateB2.startIncomingCallInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateB2.startIncomingCallInvoked = false

            do {
                Logger.debug("Test: Invoking proceed()...")
                _ = try callManagerB2?.proceed(callId: callIdA2toB, iceServers: iceServers, hideIp: useTurnOnly, videoCaptureController: videoCaptureController, bandwidthMode: .normal, audioLevelsIntervalMillis: nil)
            } catch {
                XCTFail("Call Manager proceed() failed: \(error)")
                return
            }

            expect(delegateB2.shouldSendAnswerInvoked).toEventually(equal(true), timeout: .seconds(2))
            delegateB2.shouldSendAnswerInvoked = false

            // We also expect ICE candidates to be ready for A2 and B2.
            expect(delegateA2.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: .seconds(1))
            delegateA2.shouldSendIceCandidatesInvoked = false
            expect(delegateB2.shouldSendIceCandidatesInvoked).toEventually(equal(true), timeout: .seconds(1))
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
            expect(delegateA2.eventEndedRemoteBusy).toEventually(equal(true), timeout: .seconds(1))
            delegateA2.eventEndedRemoteBusy = false
            expect(delegateA2.shouldSendHangupBusyInvoked).toEventually(equal(true), timeout: .seconds(1))
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

    func testMultiRingGlarePrimaryWinner() {
        multiRingGlareTesting(scenario: .primaryWinner)
    }

    func testMultiRingGlarePrimaryLoser() {
        multiRingGlareTesting(scenario: .primaryLoser)
    }

    func testMultiRingGlarePrimaryEqual() {
        multiRingGlareTesting(scenario: .primaryEqual)
    }

    func testMultiRingGlareDifferentDevice() {
        multiRingGlareTesting(scenario: .differentDevice)
    }

    // MARK: - Constants

    let exampleV4V3V2Offer = Data(
        [18, 132, 25, 10, 223, 24, 118, 61, 48, 13, 10, 111, 61, 45, 32, 54, 49, 53, 53, 49, 54, 53, 54, 57, 52, 57, 57, 56, 54, 48, 55, 54, 54, 49, 32, 50, 32, 73, 78, 32, 73, 80, 52, 32, 49, 50, 55, 46, 48, 46, 48, 46, 49, 13, 10, 115, 61, 45, 13, 10, 116, 61, 48, 32, 48, 13, 10, 97, 61, 103, 114, 111, 117, 112, 58, 66, 85, 78, 68, 76, 69, 32, 97, 117, 100, 105, 111, 32, 118, 105, 100, 101, 111, 32, 100, 97, 116, 97, 13, 10, 97, 61, 109, 115, 105, 100, 45, 115, 101, 109, 97, 110, 116, 105, 99, 58, 32, 87, 77, 83, 32, 65, 82, 68, 65, 77, 83, 13, 10, 109, 61, 97, 117, 100, 105, 111, 32, 57, 32, 85, 68, 80, 47, 84, 76, 83, 47, 82, 84, 80, 47, 83, 65, 86, 80, 70, 32, 49, 49, 49, 13, 10, 99, 61, 73, 78, 32, 73, 80, 52, 32, 48, 46, 48, 46, 48, 46, 48, 13, 10, 97, 61, 114, 116, 99, 112, 58, 57, 32, 73, 78, 32, 73, 80, 52, 32, 48, 46, 48, 46, 48, 46, 48, 13, 10, 97, 61, 105, 99, 101, 45, 117, 102, 114, 97, 103, 58, 53, 84, 67, 89, 13, 10, 97, 61, 105, 99, 101, 45, 112, 119, 100, 58, 50, 112, 116, 56, 43, 111, 50, 43, 97, 48, 86, 53, 84, 105, 65, 121, 121, 49, 68, 121, 113, 99, 120, 115, 13, 10, 97, 61, 105, 99, 101, 45, 111, 112, 116, 105, 111, 110, 115, 58, 116, 114, 105, 99, 107, 108, 101, 32, 114, 101, 110, 111, 109, 105, 110, 97, 116, 105, 111, 110, 13, 10, 97, 61, 102, 105, 110, 103, 101, 114, 112, 114, 105, 110, 116, 58, 115, 104, 97, 45, 50, 53, 54, 32, 68, 56, 58, 65, 48, 58, 66, 66, 58, 65, 54, 58, 55, 52, 58, 65, 70, 58, 70, 50, 58, 55, 53, 58, 56, 55, 58, 51, 68, 58, 55, 65, 58, 70, 52, 58, 65, 51, 58, 70, 65, 58, 51, 56, 58, 52, 57, 58, 51, 52, 58, 49, 56, 58, 67, 51, 58, 57, 65, 58, 51, 69, 58, 70, 52, 58, 48, 57, 58, 55, 53, 58, 54, 54, 58, 57, 55, 58, 50, 57, 58, 68, 67, 58, 70, 65, 58, 54, 65, 58, 70, 48, 58, 68, 54, 13, 10, 97, 61, 115, 101, 116, 117, 112, 58, 97, 99, 116, 112, 97, 115, 115, 13, 10, 97, 61, 109, 105, 100, 58, 97, 117, 100, 105, 111, 13, 10, 97, 61, 101, 120, 116, 109, 97, 112, 58, 49, 32, 104, 116, 116, 112, 58, 47, 47, 119, 119, 119, 46, 119, 101, 98, 114, 116, 99, 46, 111, 114, 103, 47, 101, 120, 112, 101, 114, 105, 109, 101, 110, 116, 115, 47, 114, 116, 112, 45, 104, 100, 114, 101, 120, 116, 47, 97, 98, 115, 45, 115, 101, 110, 100, 45, 116, 105, 109, 101, 13, 10, 97, 61, 101, 120, 116, 109, 97, 112, 58, 50, 32, 104, 116, 116, 112, 58, 47, 47, 119, 119, 119, 46, 105, 101, 116, 102, 46, 111, 114, 103, 47, 105, 100, 47, 100, 114, 97, 102, 116, 45, 104, 111, 108, 109, 101, 114, 45, 114, 109, 99, 97, 116, 45, 116, 114, 97, 110, 115, 112, 111, 114, 116, 45, 119, 105, 100, 101, 45, 99, 99, 45, 101, 120, 116, 101, 110, 115, 105, 111, 110, 115, 45, 48, 49, 13, 10, 97, 61, 115, 101, 110, 100, 114, 101, 99, 118, 13, 10, 97, 61, 114, 116, 99, 112, 45, 109, 117, 120, 13, 10, 97, 61, 114, 116, 112, 109, 97, 112, 58, 49, 49, 49, 32, 111, 112, 117, 115, 47, 52, 56, 48, 48, 48, 47, 50, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 49, 49, 49, 32, 116, 114, 97, 110, 115, 112, 111, 114, 116, 45, 99, 99, 13, 10, 97, 61, 102, 109, 116, 112, 58, 49, 49, 49, 32, 99, 98, 114, 61, 49, 59, 109, 105, 110, 112, 116, 105, 109, 101, 61, 49, 48, 59, 117, 115, 101, 105, 110, 98, 97, 110, 100, 102, 101, 99, 61, 49, 13, 10, 97, 61, 115, 115, 114, 99, 58, 51, 52, 52, 55, 54, 48, 52, 51, 50, 55, 32, 99, 110, 97, 109, 101, 58, 100, 110, 98, 73, 66, 121, 98, 110, 89, 47, 90, 53, 110, 118, 108, 108, 13, 10, 97, 61, 115, 115, 114, 99, 58, 51, 52, 52, 55, 54, 48, 52, 51, 50, 55, 32, 109, 115, 105, 100, 58, 65, 82, 68, 65, 77, 83, 32, 97, 117, 100, 105, 111, 49, 13, 10, 97, 61, 115, 115, 114, 99, 58, 51, 52, 52, 55, 54, 48, 52, 51, 50, 55, 32, 109, 115, 108, 97, 98, 101, 108, 58, 65, 82, 68, 65, 77, 83, 13, 10, 97, 61, 115, 115, 114, 99, 58, 51, 52, 52, 55, 54, 48, 52, 51, 50, 55, 32, 108, 97, 98, 101, 108, 58, 97, 117, 100, 105, 111, 49, 13, 10, 109, 61, 118, 105, 100, 101, 111, 32, 57, 32, 85, 68, 80, 47, 84, 76, 83, 47, 82, 84, 80, 47, 83, 65, 86, 80, 70, 32, 57, 54, 32, 57, 55, 32, 57, 56, 32, 57, 57, 32, 49, 48, 48, 32, 49, 48, 49, 32, 49, 48, 50, 32, 49, 48, 51, 32, 49, 48, 52, 13, 10, 99, 61, 73, 78, 32, 73, 80, 52, 32, 48, 46, 48, 46, 48, 46, 48, 13, 10, 97, 61, 114, 116, 99, 112, 58, 57, 32, 73, 78, 32, 73, 80, 52, 32, 48, 46, 48, 46, 48, 46, 48, 13, 10, 97, 61, 105, 99, 101, 45, 117, 102, 114, 97, 103, 58, 53, 84, 67, 89, 13, 10, 97, 61, 105, 99, 101, 45, 112, 119, 100, 58, 50, 112, 116, 56, 43, 111, 50, 43, 97, 48, 86, 53, 84, 105, 65, 121, 121, 49, 68, 121, 113, 99, 120, 115, 13, 10, 97, 61, 105, 99, 101, 45, 111, 112, 116, 105, 111, 110, 115, 58, 116, 114, 105, 99, 107, 108, 101, 32, 114, 101, 110, 111, 109, 105, 110, 97, 116, 105, 111, 110, 13, 10, 97, 61, 102, 105, 110, 103, 101, 114, 112, 114, 105, 110, 116, 58, 115, 104, 97, 45, 50, 53, 54, 32, 68, 56, 58, 65, 48, 58, 66, 66, 58, 65, 54, 58, 55, 52, 58, 65, 70, 58, 70, 50, 58, 55, 53, 58, 56, 55, 58, 51, 68, 58, 55, 65, 58, 70, 52, 58, 65, 51, 58, 70, 65, 58, 51, 56, 58, 52, 57, 58, 51, 52, 58, 49, 56, 58, 67, 51, 58, 57, 65, 58, 51, 69, 58, 70, 52, 58, 48, 57, 58, 55, 53, 58, 54, 54, 58, 57, 55, 58, 50, 57, 58, 68, 67, 58, 70, 65, 58, 54, 65, 58, 70, 48, 58, 68, 54, 13, 10, 97, 61, 115, 101, 116, 117, 112, 58, 97, 99, 116, 112, 97, 115, 115, 13, 10, 97, 61, 109, 105, 100, 58, 118, 105, 100, 101, 111, 13, 10, 97, 61, 101, 120, 116, 109, 97, 112, 58, 49, 52, 32, 117, 114, 110, 58, 105, 101, 116, 102, 58, 112, 97, 114, 97, 109, 115, 58, 114, 116, 112, 45, 104, 100, 114, 101, 120, 116, 58, 116, 111, 102, 102, 115, 101, 116, 13, 10, 97, 61, 101, 120, 116, 109, 97, 112, 58, 49, 32, 104, 116, 116, 112, 58, 47, 47, 119, 119, 119, 46, 119, 101, 98, 114, 116, 99, 46, 111, 114, 103, 47, 101, 120, 112, 101, 114, 105, 109, 101, 110, 116, 115, 47, 114, 116, 112, 45, 104, 100, 114, 101, 120, 116, 47, 97, 98, 115, 45, 115, 101, 110, 100, 45, 116, 105, 109, 101, 13, 10, 97, 61, 101, 120, 116, 109, 97, 112, 58, 51, 32, 117, 114, 110, 58, 51, 103, 112, 112, 58, 118, 105, 100, 101, 111, 45, 111, 114, 105, 101, 110, 116, 97, 116, 105, 111, 110, 13, 10, 97, 61, 101, 120, 116, 109, 97, 112, 58, 50, 32, 104, 116, 116, 112, 58, 47, 47, 119, 119, 119, 46, 105, 101, 116, 102, 46, 111, 114, 103, 47, 105, 100, 47, 100, 114, 97, 102, 116, 45, 104, 111, 108, 109, 101, 114, 45, 114, 109, 99, 97, 116, 45, 116, 114, 97, 110, 115, 112, 111, 114, 116, 45, 119, 105, 100, 101, 45, 99, 99, 45, 101, 120, 116, 101, 110, 115, 105, 111, 110, 115, 45, 48, 49, 13, 10, 97, 61, 115, 101, 110, 100, 114, 101, 99, 118, 13, 10, 97, 61, 114, 116, 99, 112, 45, 109, 117, 120, 13, 10, 97, 61, 114, 116, 99, 112, 45, 114, 115, 105, 122, 101, 13, 10, 97, 61, 114, 116, 112, 109, 97, 112, 58, 57, 54, 32, 72, 50, 54, 52, 47, 57, 48, 48, 48, 48, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 57, 54, 32, 103, 111, 111, 103, 45, 114, 101, 109, 98, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 57, 54, 32, 116, 114, 97, 110, 115, 112, 111, 114, 116, 45, 99, 99, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 57, 54, 32, 99, 99, 109, 32, 102, 105, 114, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 57, 54, 32, 110, 97, 99, 107, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 57, 54, 32, 110, 97, 99, 107, 32, 112, 108, 105, 13, 10, 97, 61, 102, 109, 116, 112, 58, 57, 54, 32, 108, 101, 118, 101, 108, 45, 97, 115, 121, 109, 109, 101, 116, 114, 121, 45, 97, 108, 108, 111, 119, 101, 100, 61, 49, 59, 112, 97, 99, 107, 101, 116, 105, 122, 97, 116, 105, 111, 110, 45, 109, 111, 100, 101, 61, 49, 59, 112, 114, 111, 102, 105, 108, 101, 45, 108, 101, 118, 101, 108, 45, 105, 100, 61, 54, 52, 48, 99, 49, 102, 13, 10, 97, 61, 114, 116, 112, 109, 97, 112, 58, 57, 55, 32, 114, 116, 120, 47, 57, 48, 48, 48, 48, 13, 10, 97, 61, 102, 109, 116, 112, 58, 57, 55, 32, 97, 112, 116, 61, 57, 54, 13, 10, 97, 61, 114, 116, 112, 109, 97, 112, 58, 57, 56, 32, 72, 50, 54, 52, 47, 57, 48, 48, 48, 48, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 57, 56, 32, 103, 111, 111, 103, 45, 114, 101, 109, 98, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 57, 56, 32, 116, 114, 97, 110, 115, 112, 111, 114, 116, 45, 99, 99, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 57, 56, 32, 99, 99, 109, 32, 102, 105, 114, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 57, 56, 32, 110, 97, 99, 107, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 57, 56, 32, 110, 97, 99, 107, 32, 112, 108, 105, 13, 10, 97, 61, 102, 109, 116, 112, 58, 57, 56, 32, 108, 101, 118, 101, 108, 45, 97, 115, 121, 109, 109, 101, 116, 114, 121, 45, 97, 108, 108, 111, 119, 101, 100, 61, 49, 59, 112, 97, 99, 107, 101, 116, 105, 122, 97, 116, 105, 111, 110, 45, 109, 111, 100, 101, 61, 49, 59, 112, 114, 111, 102, 105, 108, 101, 45, 108, 101, 118, 101, 108, 45, 105, 100, 61, 52, 50, 101, 48, 49, 102, 13, 10, 97, 61, 114, 116, 112, 109, 97, 112, 58, 57, 57, 32, 114, 116, 120, 47, 57, 48, 48, 48, 48, 13, 10, 97, 61, 102, 109, 116, 112, 58, 57, 57, 32, 97, 112, 116, 61, 57, 56, 13, 10, 97, 61, 114, 116, 112, 109, 97, 112, 58, 49, 48, 48, 32, 86, 80, 56, 47, 57, 48, 48, 48, 48, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 49, 48, 48, 32, 103, 111, 111, 103, 45, 114, 101, 109, 98, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 49, 48, 48, 32, 116, 114, 97, 110, 115, 112, 111, 114, 116, 45, 99, 99, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 49, 48, 48, 32, 99, 99, 109, 32, 102, 105, 114, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 49, 48, 48, 32, 110, 97, 99, 107, 13, 10, 97, 61, 114, 116, 99, 112, 45, 102, 98, 58, 49, 48, 48, 32, 110, 97, 99, 107, 32, 112, 108, 105, 13, 10, 97, 61, 114, 116, 112, 109, 97, 112, 58, 49, 48, 49, 32, 114, 116, 120, 47, 57, 48, 48, 48, 48, 13, 10, 97, 61, 102, 109, 116, 112, 58, 49, 48, 49, 32, 97, 112, 116, 61, 49, 48, 48, 13, 10, 97, 61, 114, 116, 112, 109, 97, 112, 58, 49, 48, 50, 32, 114, 101, 100, 47, 57, 48, 48, 48, 48, 13, 10, 97, 61, 114, 116, 112, 109, 97, 112, 58, 49, 48, 51, 32, 114, 116, 120, 47, 57, 48, 48, 48, 48, 13, 10, 97, 61, 102, 109, 116, 112, 58, 49, 48, 51, 32, 97, 112, 116, 61, 49, 48, 50, 13, 10, 97, 61, 114, 116, 112, 109, 97, 112, 58, 49, 48, 52, 32, 117, 108, 112, 102, 101, 99, 47, 57, 48, 48, 48, 48, 13, 10, 97, 61, 115, 115, 114, 99, 45, 103, 114, 111, 117, 112, 58, 70, 73, 68, 32, 51, 52, 49, 49, 52, 49, 53, 52, 51, 48, 32, 56, 52, 54, 50, 54, 53, 48, 55, 48, 13, 10, 97, 61, 115, 115, 114, 99, 58, 51, 52, 49, 49, 52, 49, 53, 52, 51, 48, 32, 99, 110, 97, 109, 101, 58, 100, 110, 98, 73, 66, 121, 98, 110, 89, 47, 90, 53, 110, 118, 108, 108, 13, 10, 97, 61, 115, 115, 114, 99, 58, 51, 52, 49, 49, 52, 49, 53, 52, 51, 48, 32, 109, 115, 105, 100, 58, 65, 82, 68, 65, 77, 83, 32, 118, 105, 100, 101, 111, 49, 13, 10, 97, 61, 115, 115, 114, 99, 58, 51, 52, 49, 49, 52, 49, 53, 52, 51, 48, 32, 109, 115, 108, 97, 98, 101, 108, 58, 65, 82, 68, 65, 77, 83, 13, 10, 97, 61, 115, 115, 114, 99, 58, 51, 52, 49, 49, 52, 49, 53, 52, 51, 48, 32, 108, 97, 98, 101, 108, 58, 118, 105, 100, 101, 111, 49, 13, 10, 97, 61, 115, 115, 114, 99, 58, 56, 52, 54, 50, 54, 53, 48, 55, 48, 32, 99, 110, 97, 109, 101, 58, 100, 110, 98, 73, 66, 121, 98, 110, 89, 47, 90, 53, 110, 118, 108, 108, 13, 10, 97, 61, 115, 115, 114, 99, 58, 56, 52, 54, 50, 54, 53, 48, 55, 48, 32, 109, 115, 105, 100, 58, 65, 82, 68, 65, 77, 83, 32, 118, 105, 100, 101, 111, 49, 13, 10, 97, 61, 115, 115, 114, 99, 58, 56, 52, 54, 50, 54, 53, 48, 55, 48, 32, 109, 115, 108, 97, 98, 101, 108, 58, 65, 82, 68, 65, 77, 83, 13, 10, 97, 61, 115, 115, 114, 99, 58, 56, 52, 54, 50, 54, 53, 48, 55, 48, 32, 108, 97, 98, 101, 108, 58, 118, 105, 100, 101, 111, 49, 13, 10, 109, 61, 97, 112, 112, 108, 105, 99, 97, 116, 105, 111, 110, 32, 57, 32, 85, 68, 80, 47, 84, 76, 83, 47, 82, 84, 80, 47, 83, 65, 86, 80, 70, 32, 49, 48, 57, 13, 10, 99, 61, 73, 78, 32, 73, 80, 52, 32, 48, 46, 48, 46, 48, 46, 48, 13, 10, 98, 61, 65, 83, 58, 51, 48, 13, 10, 97, 61, 114, 116, 99, 112, 58, 57, 32, 73, 78, 32, 73, 80, 52, 32, 48, 46, 48, 46, 48, 46, 48, 13, 10, 97, 61, 105, 99, 101, 45, 117, 102, 114, 97, 103, 58, 53, 84, 67, 89, 13, 10, 97, 61, 105, 99, 101, 45, 112, 119, 100, 58, 50, 112, 116, 56, 43, 111, 50, 43, 97, 48, 86, 53, 84, 105, 65, 121, 121, 49, 68, 121, 113, 99, 120, 115, 13, 10, 97, 61, 105, 99, 101, 45, 111, 112, 116, 105, 111, 110, 115, 58, 116, 114, 105, 99, 107, 108, 101, 32, 114, 101, 110, 111, 109, 105, 110, 97, 116, 105, 111, 110, 13, 10, 97, 61, 102, 105, 110, 103, 101, 114, 112, 114, 105, 110, 116, 58, 115, 104, 97, 45, 50, 53, 54, 32, 68, 56, 58, 65, 48, 58, 66, 66, 58, 65, 54, 58, 55, 52, 58, 65, 70, 58, 70, 50, 58, 55, 53, 58, 56, 55, 58, 51, 68, 58, 55, 65, 58, 70, 52, 58, 65, 51, 58, 70, 65, 58, 51, 56, 58, 52, 57, 58, 51, 52, 58, 49, 56, 58, 67, 51, 58, 57, 65, 58, 51, 69, 58, 70, 52, 58, 48, 57, 58, 55, 53, 58, 54, 54, 58, 57, 55, 58, 50, 57, 58, 68, 67, 58, 70, 65, 58, 54, 65, 58, 70, 48, 58, 68, 54, 13, 10, 97, 61, 115, 101, 116, 117, 112, 58, 97, 99, 116, 112, 97, 115, 115, 13, 10, 97, 61, 109, 105, 100, 58, 100, 97, 116, 97, 13, 10, 97, 61, 115, 101, 110, 100, 114, 101, 99, 118, 13, 10, 97, 61, 114, 116, 99, 112, 45, 109, 117, 120, 13, 10, 97, 61, 114, 116, 112, 109, 97, 112, 58, 49, 48, 57, 32, 103, 111, 111, 103, 108, 101, 45, 100, 97, 116, 97, 47, 57, 48, 48, 48, 48, 13, 10, 97, 61, 115, 115, 114, 99, 58, 52, 50, 54, 54, 56, 53, 57, 53, 51, 49, 32, 99, 110, 97, 109, 101, 58, 100, 110, 98, 73, 66, 121, 98, 110, 89, 47, 90, 53, 110, 118, 108, 108, 13, 10, 97, 61, 115, 115, 114, 99, 58, 52, 50, 54, 54, 56, 53, 57, 53, 51, 49, 32, 109, 115, 105, 100, 58, 115, 105, 103, 110, 97, 108, 105, 110, 103, 32, 115, 105, 103, 110, 97, 108, 105, 110, 103, 13, 10, 97, 61, 115, 115, 114, 99, 58, 52, 50, 54, 54, 56, 53, 57, 53, 51, 49, 32, 109, 115, 108, 97, 98, 101, 108, 58, 115, 105, 103, 110, 97, 108, 105, 110, 103, 13, 10, 97, 61, 115, 115, 114, 99, 58, 52, 50, 54, 54, 56, 53, 57, 53, 51, 49, 32, 108, 97, 98, 101, 108, 58, 115, 105, 103, 110, 97, 108, 105, 110, 103, 13, 10, 18, 32, 137, 138, 251, 85, 15, 240, 215, 1, 33, 233, 51, 132, 97, 36, 254, 111, 60, 105, 207, 137, 41, 137, 38, 41, 250, 225, 143, 74, 85, 182, 172, 9, 34, 82, 10, 32, 137, 138, 251, 85, 15, 240, 215, 1, 33, 233, 51, 132, 97, 36, 254, 111, 60, 105, 207, 137, 41, 137, 38, 41, 250, 225, 143, 74, 85, 182, 172, 9, 18, 4, 53, 84, 67, 89, 26, 24, 50, 112, 116, 56, 43, 111, 50, 43, 97, 48, 86, 53, 84, 105, 65, 121, 121, 49, 68, 121, 113, 99, 120, 115, 34, 4, 8, 46, 16, 31, 34, 4, 8, 40, 16, 31, 34, 2, 8, 8])

    let exampleV4Answer = Data([34, 82, 10, 32, 60, 93, 207, 142, 18, 208, 151, 187, 125, 151, 77, 86, 197, 145, 136, 202, 197, 146, 173, 45, 125, 106, 161, 170, 46, 112, 192, 50, 103, 106, 207, 122, 18, 4, 109, 88, 56, 112, 26, 24, 113, 48, 117, 53, 80, 90, 119, 111, 67, 106, 115, 52, 113, 52, 110, 57, 76, 56, 89, 50, 100, 114, 86, 99, 34, 4, 8, 46, 16, 31, 34, 4, 8, 40, 16, 31, 34, 2, 8, 8])

    let dummyLocalIdentityKey = Data(_: [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07])
    let dummyRemoteIdentityKey = Data(_: [0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f])

}
