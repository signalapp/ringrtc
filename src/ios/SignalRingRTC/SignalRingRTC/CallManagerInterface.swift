//
//  Copyright (c) 2020 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC
import WebRTC
import SignalCoreKit

protocol CallManagerInterfaceDelegate: class {
    func onStartCall(remote: UnsafeRawPointer, callId: UInt64, isOutgoing: Bool, callMediaType: CallMediaType)
    func onEvent(remote: UnsafeRawPointer, event: CallManagerEvent)
    func onSendOffer(callId: UInt64, remote: UnsafeRawPointer, destinationDeviceId: UInt32?, opaque: Data?, sdp: String?, callMediaType: CallMediaType)
    func onSendAnswer(callId: UInt64, remote: UnsafeRawPointer, destinationDeviceId: UInt32?, opaque: Data?, sdp: String?)
    func onSendIceCandidates(callId: UInt64, remote: UnsafeRawPointer, destinationDeviceId: UInt32?, candidates: [CallManagerIceCandidate])
    func onSendHangup(callId: UInt64, remote: UnsafeRawPointer, destinationDeviceId: UInt32?, hangupType: HangupType, deviceId: UInt32, useLegacyHangupMessage: Bool)
    func onSendBusy(callId: UInt64, remote: UnsafeRawPointer, destinationDeviceId: UInt32?)
    func onCreateConnection(pcObserver: UnsafeMutableRawPointer?, deviceId: UInt32, appCallContext: CallContext, enableDtls: Bool, enableRtpDataChannel: Bool) -> (connection: Connection, pc: UnsafeMutableRawPointer?)
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

    func onStartCall(remote: UnsafeRawPointer, callId: UInt64, isOutgoing: Bool, callMediaType: CallMediaType) {
        guard let delegate = self.callManagerObserverDelegate else {
            return
        }

        delegate.onStartCall(remote: remote, callId: callId, isOutgoing: isOutgoing, callMediaType: callMediaType)
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

    func onSendOffer(callId: UInt64, remote: UnsafeRawPointer, destinationDeviceId: UInt32?, opaque: Data?, sdp: String?, callMediaType: CallMediaType) {
        guard let delegate = self.callManagerObserverDelegate else {
            return
        }

        delegate.onSendOffer(callId: callId, remote: remote, destinationDeviceId: destinationDeviceId, opaque: opaque, sdp: sdp, callMediaType: callMediaType)
    }

    func onSendAnswer(callId: UInt64, remote: UnsafeRawPointer, destinationDeviceId: UInt32?, opaque: Data?, sdp: String?) {
        guard let delegate = self.callManagerObserverDelegate else {
            return
        }

        delegate.onSendAnswer(callId: callId, remote: remote, destinationDeviceId: destinationDeviceId, opaque: opaque, sdp: sdp)
    }

    func onSendIceCandidates(callId: UInt64, remote: UnsafeRawPointer, destinationDeviceId: UInt32?, candidates: [CallManagerIceCandidate]) {
        guard let delegate = self.callManagerObserverDelegate else {
            return
        }

        delegate.onSendIceCandidates(callId: callId, remote: remote, destinationDeviceId: destinationDeviceId, candidates: candidates)
    }

    func onSendHangup(callId: UInt64, remote: UnsafeRawPointer, destinationDeviceId: UInt32?, hangupType: HangupType, deviceId: UInt32, useLegacyHangupMessage: Bool) {
        guard let delegate = self.callManagerObserverDelegate else {
            return
        }

        delegate.onSendHangup(callId: callId, remote: remote, destinationDeviceId: destinationDeviceId, hangupType: hangupType, deviceId: deviceId, useLegacyHangupMessage: useLegacyHangupMessage)
    }

    func onSendBusy(callId: UInt64, remote: UnsafeRawPointer, destinationDeviceId: UInt32?) {
        guard let delegate = self.callManagerObserverDelegate else {
            return
        }

        delegate.onSendBusy(callId: callId, remote: remote, destinationDeviceId: destinationDeviceId)
    }

    func onCreateConnection(pcObserver: UnsafeMutableRawPointer?, deviceId: UInt32, appCallContext: CallContext, enableDtls: Bool, enableRtpDataChannel: Bool) -> (connection: Connection, pc: UnsafeMutableRawPointer?)? {
        guard let delegate = self.callManagerObserverDelegate else {
            return nil
        }

        return delegate.onCreateConnection(pcObserver: pcObserver, deviceId: deviceId, appCallContext: appCallContext, enableDtls: enableDtls, enableRtpDataChannel: enableRtpDataChannel)
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

func callManagerInterfaceOnStartCall(object: UnsafeMutableRawPointer?, remote: UnsafeRawPointer?, callId: UInt64, isOutgoing: Bool, mediaType: Int32) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }
    let obj: CallManagerInterface = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let remote = remote else {
        owsFailDebug("remote was unexpectedly nil")
        return
    }

    let callMediaType: CallMediaType
    if let validMediaType = CallMediaType(rawValue: mediaType) {
        callMediaType = validMediaType
    } else {
        owsFailDebug("unexpected call media type")
        return
    }

    obj.onStartCall(remote: remote, callId: callId, isOutgoing: isOutgoing, callMediaType: callMediaType)
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

func callManagerInterfaceOnSendOffer(object: UnsafeMutableRawPointer?, callId: UInt64, remote: UnsafeRawPointer?, destinationDeviceId: UInt32, broadcast: Bool, opaque: AppByteSlice, sdp: AppByteSlice, mediaType: Int32) {
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
    var destinationDeviceId: UInt32? = destinationDeviceId
    if broadcast {
        destinationDeviceId = nil
    }

    let callMediaType: CallMediaType
    if let validMediaType = CallMediaType(rawValue: mediaType) {
        callMediaType = validMediaType
    } else {
        owsFailDebug("unexpected call media type")
        return
    }

    obj.onSendOffer(callId: callId, remote: remote, destinationDeviceId: destinationDeviceId, opaque: opaque.asData(), sdp: sdp.asString(), callMediaType: callMediaType)
}

func callManagerInterfaceOnSendAnswer(object: UnsafeMutableRawPointer?, callId: UInt64, remote: UnsafeRawPointer?, destinationDeviceId: UInt32, broadcast: Bool, opaque: AppByteSlice, sdp: AppByteSlice) {
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
    var destinationDeviceId: UInt32? = destinationDeviceId
    if broadcast {
        destinationDeviceId = nil
    }

    obj.onSendAnswer(callId: callId, remote: remote, destinationDeviceId: destinationDeviceId, opaque: opaque.asData(), sdp: sdp.asString())
}

func callManagerInterfaceOnSendIceCandidates(object: UnsafeMutableRawPointer?, callId: UInt64, remote: UnsafeRawPointer?, destinationDeviceId: UInt32, broadcast: Bool, candidates: UnsafePointer<AppIceCandidateArray>?) {
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
        finalCandidates.append(CallManagerIceCandidate(opaque: iceCandidate.opaque.asData(), sdp: iceCandidate.sdp.asString()))
    }

    // If we will broadcast this message, ignore the deviceId.
    var destinationDeviceId: UInt32? = destinationDeviceId
    if broadcast {
        destinationDeviceId = nil
    }

    obj.onSendIceCandidates(callId: callId, remote: remote, destinationDeviceId: destinationDeviceId, candidates: finalCandidates)
}

func callManagerInterfaceOnSendHangup(object: UnsafeMutableRawPointer?, callId: UInt64, remote: UnsafeRawPointer?, destinationDeviceId: UInt32, broadcast: Bool, type: Int32, deviceId: UInt32, useLegacyHangupMessage: Bool) {
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
    var destinationDeviceId: UInt32? = destinationDeviceId
    if broadcast {
        destinationDeviceId = nil
    }

    let hangupType: HangupType
    if let validHangupType = HangupType(rawValue: type) {
        hangupType = validHangupType
    } else {
        owsFailDebug("unexpected hangup type")
        return
    }

    obj.onSendHangup(callId: callId, remote: remote, destinationDeviceId: destinationDeviceId, hangupType: hangupType, deviceId: deviceId, useLegacyHangupMessage: useLegacyHangupMessage)
}

func callManagerInterfaceOnSendBusy(object: UnsafeMutableRawPointer?, callId: UInt64, remote: UnsafeRawPointer?, destinationDeviceId: UInt32, broadcast: Bool) {
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
    var destinationDeviceId: UInt32? = destinationDeviceId
    if broadcast {
        destinationDeviceId = nil
    }

    obj.onSendBusy(callId: callId, remote: remote, destinationDeviceId: destinationDeviceId)
}

func callManagerInterfaceOnCreateConnectionInterface(object: UnsafeMutableRawPointer?, observer: UnsafeMutableRawPointer?, deviceId: UInt32, context: UnsafeMutableRawPointer?, enableDtls: Bool, enableRtpDataChannel: Bool) -> AppConnectionInterface {
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

    if let connectionDetails = obj.onCreateConnection(pcObserver: observer, deviceId: deviceId, appCallContext: appCallContext, enableDtls: enableDtls, enableRtpDataChannel: enableRtpDataChannel) {
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

    let _: CallManagerInterface = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let appConnection = connection else {
        owsFailDebug("appConnection was unexpectedly nil")

        // Swift was problematic to pass back some nullable structure, so we
        // now pass an empty structure back.
        return AppMediaStreamInterface(
            object: nil,
            destroy: nil,
            createMediaStream: nil)
    }

    let connection: Connection = Unmanaged.fromOpaque(appConnection).takeUnretainedValue()

    // For this function, we don't need the Call Manager object to anything, so we
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
