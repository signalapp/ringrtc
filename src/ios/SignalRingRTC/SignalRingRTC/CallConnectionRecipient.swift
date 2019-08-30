//
//  Copyright (c) 2019 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC
import WebRTC
import SignalCoreKit

protocol CallConnectionRecipientDelegate: class {
    func onSendOffer(_ callConnectionRecipient: CallConnectionRecipient, callId: Int64, offer: String)
    func onSendAnswer(_ callConnectionRecipient: CallConnectionRecipient, callId: Int64, answer: String)
    func onSendIceCandidates(_ callConnectionRecipient: CallConnectionRecipient, callId: Int64, candidates: [RTCIceCandidate])
    func onSendHangup(_ callConnectionRecipient: CallConnectionRecipient, callId: Int64)
}

class CallConnectionRecipient {

    private weak var delegate: CallConnectionRecipientDelegate?

    // MARK: Object Lifetime

    init(delegate: CallConnectionRecipientDelegate) {
        self.delegate = delegate

        Logger.debug("object! CallConnectionRecipient created... \(ObjectIdentifier(self))")
    }

    deinit {
        Logger.debug("object! CallConnectionRecipient destroyed. \(ObjectIdentifier(self))")
    }

    // MARK: API Functions

//    typedef struct {
//      void *object;
//      void (*destroy)(void *object);
//      void (*onSendOffer)(void *object, int64_t callId, IOSByteSlice offer);
//      void (*onSendAnswer)(void *object, int64_t callId, IOSByteSlice answer);
//      void (*onSendIceCandidate)(void *object, int64_t callId, IOSIceCandidate iceCandidate);
//      void (*onSendHangup)(void *object, int64_t callId);
//      void (*onSendBusy)(void *object, int64_t callId);
//    } IOSRecipient;

    func getWrapper() -> IOSRecipient {
        return IOSRecipient(
            object: UnsafeMutableRawPointer(Unmanaged.passRetained(self).toOpaque()),
            destroy: callConnectionRecipientDestroy,
            onSendOffer: callConnectionRecipientOnSendOffer,
            onSendAnswer: callConnectionRecipientOnSendAnswer,
            onSendIceCandidates: callConnectionRecipientOnSendIceCandidates,
            onSendHangup: callConnectionRecipientOnSendHangup,
            onSendBusy: callConnectionRecipientOnSendBusy)
    }

    // MARK: Delegate Handlers

    func onSendOffer(callId: Int64, offer: String) {
        guard let delegate = self.delegate else {
            return
        }

        delegate.onSendOffer(self, callId: callId, offer: offer)
    }

    func onSendAnswer(callId: Int64, answer: String) {
        guard let delegate = self.delegate else {
            return
        }

        delegate.onSendAnswer(self, callId: callId, answer: answer)
    }

    func onSendIceCandidates(callId: Int64, candidates: [RTCIceCandidate]) {
        guard let delegate = self.delegate else {
            return
        }

        delegate.onSendIceCandidates(self, callId: callId, candidates: candidates)
    }

    func onSendHangup(callId: Int64) {
        guard let delegate = self.delegate else {
            return
        }

        delegate.onSendHangup(self, callId: callId)
    }
}

func callConnectionRecipientDestroy(object: UnsafeMutableRawPointer?) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }

    _ = Unmanaged<CallConnectionRecipient>.fromOpaque(object).takeRetainedValue()
    // @note There should not be any retainers left for the object
    // so deinit should be called implicitly.
}

func callConnectionRecipientOnSendOffer(object: UnsafeMutableRawPointer?, callId: Int64, offer: IOSByteSlice) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }
    let obj: CallConnectionRecipient = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let string = offer.asString() else {
        owsFailDebug("unexpected offer string")
        return
    }

    obj.onSendOffer(callId: callId, offer: string)
}

func callConnectionRecipientOnSendAnswer(object: UnsafeMutableRawPointer?, callId: Int64, answer: IOSByteSlice) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }
    let obj: CallConnectionRecipient = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let string = answer.asString() else {
        owsFailDebug("unexpected answer string")
        return
    }

    obj.onSendAnswer(callId: callId, answer: string)
}

func callConnectionRecipientOnSendIceCandidates(object: UnsafeMutableRawPointer?, callId: Int64, candidates: UnsafePointer<IOSIceCandidateArray>?) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }

    guard let candidates = candidates else {
        owsFailDebug("candidates was unexpectedly nil")
        return
    }

    let obj: CallConnectionRecipient = Unmanaged.fromOpaque(object).takeUnretainedValue()

    let iceCandidates = UnsafePointer<IOSIceCandidateArray>(candidates)
    let count = iceCandidates.pointee.count

    // Form the application level Ice Candidate array with
    // copies of all strings before returning.
    var finalIceCandidates: [RTCIceCandidate] = []

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

        finalIceCandidates.append(RTCIceCandidate(sdp: sdpString, sdpMLineIndex: iceCandidate.sdpMLineIndex, sdpMid: sdpMidString))
    }

    obj.onSendIceCandidates(callId: callId, candidates: finalIceCandidates)
}

func callConnectionRecipientOnSendHangup(object: UnsafeMutableRawPointer?, callId: Int64) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }
    let obj: CallConnectionRecipient = Unmanaged.fromOpaque(object).takeUnretainedValue()

    obj.onSendHangup(callId: callId)
}

func callConnectionRecipientOnSendBusy(object: UnsafeMutableRawPointer?, callId: Int64) {
    // "SendBusy" is not supported on iOS.
}
