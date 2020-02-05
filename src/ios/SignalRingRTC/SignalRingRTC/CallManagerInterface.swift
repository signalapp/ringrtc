//
//  Copyright (c) 2020 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC
import WebRTC
import SignalCoreKit

protocol CallManagerInterfaceDelegate: class {
    func onStartCall(remote: UnsafeRawPointer, callId: UInt64, isOutgoing: Bool)
    func onEvent(remote: UnsafeRawPointer, event: CallManagerEvent)
    func onSendOffer(callId: UInt64, remote: UnsafeRawPointer, deviceId: UInt32?, offer: String)
    func onSendAnswer(callId: UInt64, remote: UnsafeRawPointer, deviceId: UInt32?, answer: String)
    func onSendIceCandidates(callId: UInt64, remote: UnsafeRawPointer, deviceId: UInt32?, candidates: [CallManagerIceCandidate])
    func onSendHangup(callId: UInt64, remote: UnsafeRawPointer, deviceId: UInt32?)
    func onSendBusy(callId: UInt64, remote: UnsafeRawPointer, deviceId: UInt32?)
    func onCreateConnection(pcObserver: UnsafeMutableRawPointer?, deviceId: UInt32, appCallContext: CallContext) -> (connection: Connection, pc: UnsafeMutableRawPointer?)
    func onConnectMedia(remote: UnsafeRawPointer, appCallContext: CallContext, stream: RTCMediaStream)
    func onCompareRemotes(remote1: UnsafeRawPointer, remote2: UnsafeRawPointer) -> Bool
    func onCallConcluded(remote: UnsafeRawPointer)
}

class CallManagerInterface {

    private weak var callManagerObserverDelegate: CallManagerInterfaceDelegate?

    init(delegate: CallManagerInterfaceDelegate) {
        self.callManagerObserverDelegate = delegate

        Logger.debug("object! CallManagerInterface created... \(ObjectIdentifier(self))")
    }

    deinit {
        Logger.debug("object! CallManagerInterface destroyed. \(ObjectIdentifier(self))")
    }

    // MARK: API Functions

     func getWrapper() -> AppInterface {
         return AppInterface(
             object: UnsafeMutableRawPointer(Unmanaged.passRetained(self).toOpaque()),
             destroy: callManagerInterfaceDestroy,
             onStartCall: callManagerInterfaceOnStartCall,
             onEvent: callManagerInterfaceOnCallEvent,
             onSendOffer: callManagerInterfaceOnSendOffer,
             onSendAnswer: callManagerInterfaceOnSendAnswer,
             onSendIceCandidates: callManagerInterfaceOnSendIceCandidates,
             onSendHangup: callManagerInterfaceOnSendHangup,
             onSendBusy: callManagerInterfaceOnSendBusy,
             onCreateConnectionInterface: callManagerInterfaceOnCreateConnectionInterface,
             onCreateMediaStreamInterface: callManagerInterfaceOnCreateMediaStreamInterface,
             onConnectMedia: callManagerInterfaceOnConnectMedia,
             onCompareRemotes: callManagerInterfaceOnCompareRemotes,
             onCallConcluded: callManagerInterfaceOnCallConcluded)
     }

    // MARK: Delegate Handlers

    func onStartCall(remote: UnsafeRawPointer, callId: UInt64, isOutgoing: Bool) {
        guard let delegate = self.callManagerObserverDelegate else {
            return
        }

        delegate.onStartCall(remote: remote, callId: callId, isOutgoing: isOutgoing)
    }

    func onEvent(remote: UnsafeRawPointer, event: Int32) {
        guard let delegate = self.callManagerObserverDelegate else {
            return
        }

        if let validEvent = CallManagerEvent(rawValue: event) {
            delegate.onEvent(remote: remote, event: validEvent)
        } else {
            owsFailDebug("invalid event: \(event)")
        }
    }

    func onSendOffer(callId: UInt64, remote: UnsafeRawPointer, deviceId: UInt32?, offer: String) {
        guard let delegate = self.callManagerObserverDelegate else {
            return
        }

        delegate.onSendOffer(callId: callId, remote: remote, deviceId: deviceId, offer: offer)
    }

    func onSendAnswer(callId: UInt64, remote: UnsafeRawPointer, deviceId: UInt32?, answer: String) {
        guard let delegate = self.callManagerObserverDelegate else {
            return
        }

        delegate.onSendAnswer(callId: callId, remote: remote, deviceId: deviceId, answer: answer)
    }

    func onSendIceCandidates(callId: UInt64, remote: UnsafeRawPointer, deviceId: UInt32?, candidates: [CallManagerIceCandidate]) {
        guard let delegate = self.callManagerObserverDelegate else {
            return
        }

        delegate.onSendIceCandidates(callId: callId, remote: remote, deviceId: deviceId, candidates: candidates)
    }

    func onSendHangup(callId: UInt64, remote: UnsafeRawPointer, deviceId: UInt32?) {
        guard let delegate = self.callManagerObserverDelegate else {
            return
        }

        delegate.onSendHangup(callId: callId, remote: remote, deviceId: deviceId)
    }

    func onSendBusy(callId: UInt64, remote: UnsafeRawPointer, deviceId: UInt32?) {
        guard let delegate = self.callManagerObserverDelegate else {
            return
        }

        delegate.onSendBusy(callId: callId, remote: remote, deviceId: deviceId)
    }

    func onCreateConnection(pcObserver: UnsafeMutableRawPointer?, deviceId: UInt32, appCallContext: CallContext) -> (connection: Connection, pc: UnsafeMutableRawPointer?)? {
        guard let delegate = self.callManagerObserverDelegate else {
            return nil
        }

        return delegate.onCreateConnection(pcObserver: pcObserver, deviceId: deviceId, appCallContext: appCallContext)
    }

    func onConnectedMedia(remote: UnsafeRawPointer, appCallContext: CallContext, stream: RTCMediaStream) {
        guard let delegate = self.callManagerObserverDelegate else {
            return
        }

        delegate.onConnectMedia(remote: remote, appCallContext: appCallContext, stream: stream)
    }

    func onCompareRemotes(remote1: UnsafeRawPointer, remote2: UnsafeRawPointer) -> Bool {
        guard let delegate = self.callManagerObserverDelegate else {
            return false
        }

        return delegate.onCompareRemotes(remote1: remote1, remote2: remote2)
    }

    func onCallConcluded(remote: UnsafeRawPointer) {
        guard let delegate = self.callManagerObserverDelegate else {
            return
        }

        delegate.onCallConcluded(remote: remote)
    }
}

func callManagerInterfaceDestroy(object: UnsafeMutableRawPointer?) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }

    _ = Unmanaged<CallManagerInterface>.fromOpaque(object).takeRetainedValue()
    // @note There should not be any retainers left for the object
    // so deinit should be called implicitly.
}

func callManagerInterfaceOnStartCall(object: UnsafeMutableRawPointer?, remote: UnsafeRawPointer?, callId: UInt64, isOutgoing: Bool) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }
    let obj: CallManagerInterface = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let remote = remote else {
        owsFailDebug("remote was unexpectedly nil")
        return
    }

    obj.onStartCall(remote: remote, callId: callId, isOutgoing: isOutgoing)
}

func callManagerInterfaceOnCallEvent(object: UnsafeMutableRawPointer?, remote: UnsafeRawPointer?, event: Int32) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }
    let obj: CallManagerInterface = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let remote = remote else {
        owsFailDebug("remote was unexpectedly nil")
        return
    }

    obj.onEvent(remote: remote, event: event)
}

func callManagerInterfaceOnSendOffer(object: UnsafeMutableRawPointer?, callId: UInt64, remote: UnsafeRawPointer?, deviceId: UInt32, broadcast: Bool, offer: AppByteSlice) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }
    let obj: CallManagerInterface = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let remote = remote else {
        owsFailDebug("remote was unexpectedly nil")
        return
    }

    guard let string = offer.asString() else {
        owsFailDebug("unexpected offer string")
        return
    }

    // If we will broadcast this message, ignore the deviceId.
    var deviceId: UInt32? = deviceId
    if broadcast {
        deviceId = nil
    }

    obj.onSendOffer(callId: callId, remote: remote, deviceId: deviceId, offer: string)
}

func callManagerInterfaceOnSendAnswer(object: UnsafeMutableRawPointer?, callId: UInt64, remote: UnsafeRawPointer?, deviceId: UInt32, broadcast: Bool, answer: AppByteSlice) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }
    let obj: CallManagerInterface = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let remote = remote else {
        owsFailDebug("remote was unexpectedly nil")
        return
    }

    guard let string = answer.asString() else {
        owsFailDebug("unexpected answer string")
        return
    }

    // If we will broadcast this message, ignore the deviceId.
    var deviceId: UInt32? = deviceId
    if broadcast {
        deviceId = nil
    }

    obj.onSendAnswer(callId: callId, remote: remote, deviceId: deviceId, answer: string)
}

func callManagerInterfaceOnSendIceCandidates(object: UnsafeMutableRawPointer?, callId: UInt64, remote: UnsafeRawPointer?, deviceId: UInt32, broadcast: Bool, candidates: UnsafePointer<AppIceCandidateArray>?) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }
    let obj: CallManagerInterface = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let remote = remote else {
        owsFailDebug("remote was unexpectedly nil")
        return
    }

    guard let candidates = candidates else {
        owsFailDebug("candidates was unexpectedly nil")
        return
    }

    let iceCandidates = UnsafePointer<AppIceCandidateArray>(candidates)
    let count = iceCandidates.pointee.count

    // Form the application level Ice Candidate array with
    // copies of all strings before returning.
    var finalCandidates: [CallManagerIceCandidate] = []

    for index in 0..<count {
        let iceCandidate = iceCandidates.pointee.candidates[index]

        guard let sdpString = iceCandidate.sdp.asString() else {
            owsFailDebug("unexpected string")

            // @note We prefer to ignore this array item.
            continue
        }

        guard let sdpMidString = iceCandidate.sdpMid.asString() else {
            owsFailDebug("unexpected string")

            // @note We prefer to ignore this array item.
            continue
        }

        finalCandidates.append(CallManagerIceCandidate(sdp: sdpString, sdpMLineIndex: iceCandidate.sdpMLineIndex, sdpMid: sdpMidString))
    }

    // If we will broadcast this message, ignore the deviceId.
    var deviceId: UInt32? = deviceId
    if broadcast {
        deviceId = nil
    }

    obj.onSendIceCandidates(callId: callId, remote: remote, deviceId: deviceId, candidates: finalCandidates)
}

func callManagerInterfaceOnSendHangup(object: UnsafeMutableRawPointer?, callId: UInt64, remote: UnsafeRawPointer?, deviceId: UInt32, broadcast: Bool) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }
    let obj: CallManagerInterface = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let remote = remote else {
        owsFailDebug("remote was unexpectedly nil")
        return
    }

    // If we will broadcast this message, ignore the deviceId.
    var deviceId: UInt32? = deviceId
    if broadcast {
        deviceId = nil
    }

    obj.onSendHangup(callId: callId, remote: remote, deviceId: deviceId)
}

func callManagerInterfaceOnSendBusy(object: UnsafeMutableRawPointer?, callId: UInt64, remote: UnsafeRawPointer?, deviceId: UInt32, broadcast: Bool) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }
    let obj: CallManagerInterface = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let remote = remote else {
        owsFailDebug("remote was unexpectedly nil")
        return
    }

    // If we will broadcast this message, ignore the deviceId.
    var deviceId: UInt32? = deviceId
    if broadcast {
        deviceId = nil
    }

    obj.onSendBusy(callId: callId, remote: remote, deviceId: deviceId)
}

func callManagerInterfaceOnCreateConnectionInterface(object: UnsafeMutableRawPointer?, observer: UnsafeMutableRawPointer?, deviceId: UInt32, context: UnsafeMutableRawPointer?) -> AppConnectionInterface {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")

        // Swift was problematic to pass back some nullable structure, so we
        // now pass an empty structure back. Check pc for now to validate.
        return AppConnectionInterface(
            object: nil,
            pc: nil,
            destroy: nil)
    }

    let obj: CallManagerInterface = Unmanaged.fromOpaque(object).takeUnretainedValue()

    // @todo Make sure there is a pcObserver.

    guard let callContext = context else {
        owsFailDebug("context was unexpectedly nil")

        // Swift was problematic to pass back some nullable structure, so we
        // now pass an empty structure back. Check pc for now to validate.
        return AppConnectionInterface(
            object: nil,
            pc: nil,
            destroy: nil)
    }

    let appCallContext: CallContext = Unmanaged.fromOpaque(callContext).takeUnretainedValue()

    if let connectionDetails = obj.onCreateConnection(pcObserver: observer, deviceId: deviceId, appCallContext: appCallContext) {
        return connectionDetails.connection.getWrapper(pc: connectionDetails.pc)
    } else {
        // Swift was problematic to pass back some nullable structure, so we
        // now pass an empty structure back. Check pc for now to validate.
        // @todo Should check object, not pc, for consistency. We will pass valid object if the whole thing is valid...
        return AppConnectionInterface(
            object: nil,
            pc: nil,
            destroy: nil)
    }
}

func callManagerInterfaceOnCreateMediaStreamInterface(object: UnsafeMutableRawPointer?, connection: UnsafeMutableRawPointer?) -> AppMediaStreamInterface {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")

        // Swift was problematic to pass back some nullable structure, so we
        // now pass an empty structure back.
        return AppMediaStreamInterface(
            object: nil,
            destroy: nil,
            createMediaStream: nil)
    }

    let obj: CallManagerInterface = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let appConnection = connection else {
        owsFailDebug("appConnection was unexpectedly nil")

        // Swift was problematic to pass back some nullable structure, so we
        // now pass an empty structure back.
        return AppMediaStreamInterface(
            object: nil,
            destroy: nil,
            createMediaStream: nil)
    }

    // @todo Maybe take the retained value and give it to the appMediaStream?
    let connection: Connection = Unmanaged.fromOpaque(appConnection).takeUnretainedValue()

    // @note For this function, we don't need the Call Manager object to anything, so we
    // will directly create a ConnectionMediaStream object and return it.

    let appMediaStream = ConnectionMediaStream(connection: connection)

    return appMediaStream.getWrapper()
}

func callManagerInterfaceOnConnectMedia(object: UnsafeMutableRawPointer?, remote: UnsafeRawPointer?, context: UnsafeMutableRawPointer?, stream: UnsafeRawPointer?) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }

    let obj: CallManagerInterface = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let remote = remote else {
        owsFailDebug("remote was unexpectedly nil")
        return
    }

    guard let callContext = context else {
        owsFailDebug("context was unexpectedly nil")
        return
    }

    let appCallContext: CallContext = Unmanaged.fromOpaque(callContext).takeUnretainedValue()

    guard let stream = stream else {
        owsFailDebug("stream was unexpectedly nil")
        return
    }

    let mediaStream: RTCMediaStream = Unmanaged.fromOpaque(stream).takeUnretainedValue()

    obj.onConnectedMedia(remote: remote, appCallContext: appCallContext, stream: mediaStream)
}

func callManagerInterfaceOnCompareRemotes(object: UnsafeMutableRawPointer?, remote1: UnsafeRawPointer?, remote2: UnsafeRawPointer?) -> Bool {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return false
    }
    let obj: CallManagerInterface = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let remote1 = remote1 else {
        owsFailDebug("remote1 was unexpectedly nil")
        return false
    }

    guard let remote2 = remote2 else {
        owsFailDebug("remote2 was unexpectedly nil")
        return false
    }

    return obj.onCompareRemotes(remote1: remote1, remote2: remote2)
}

func callManagerInterfaceOnCallConcluded(object: UnsafeMutableRawPointer?, remote: UnsafeRawPointer?) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }

    let obj: CallManagerInterface = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let remote = remote else {
        owsFailDebug("remote was unexpectedly nil")
        return
    }

    obj.onCallConcluded(remote: remote)
}
