//
//  Copyright (c) 2019 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC
import WebRTC
import SignalCoreKit

public enum CallEvent: Int32 {
    /**
     * The call is being established, generally meaning that Ice has changed to
     * a connected state.
     */
    case ringing = 0

    /**
     * The remote peer indicates connection success, where the remote is the
     * callee and is accepting the call via a data channel message.
     */
    case remoteConnected = 1

    /**
     * The remote peer indicates its video stream is enabled (sending).
     */
    case remoteVideoEnable = 2

    /**
     * The remote peer indicates its video stream is disabled (not sending).
     */
    case remoteVideoDisable = 3

    /**
     * The remote peer indicates it is terminating the call.
     */
    case remoteHangup = 4

    /**
     * The call failed to connect during the call setup phase.
     */
    case connectionFailed = 5

    /**
     * Unable to establish the call within a resonable amount of time, as
     * either the caller or the callee.
     */
    case callTimeout = 6

    /**
     * The call dropped while connected and is now reconnecting.
     */
    case callReconnecting = 7
}

protocol CallConnectionObserverDelegate: class {
    func onCallEvent(_ callConnectionObserver: CallConnectionObserver, callId: Int64, callEvent: CallEvent)
    func onCallError(_ callConnectionObserver: CallConnectionObserver, callId: Int64, errorString: String)
    func onAddStream(_ callConnectionObserver: CallConnectionObserver, callId: Int64, stream: RTCMediaStream)
}

class CallConnectionObserver {

    private weak var delegate: CallConnectionObserverDelegate?

    // MARK: Object Lifetime

    init(delegate: CallConnectionObserverDelegate) {
        self.delegate = delegate

        Logger.debug("object! CallConnectionObserver created... \(ObjectIdentifier(self))")
    }

    deinit {
        Logger.debug("object! CallConnectionObserver destroyed. \(ObjectIdentifier(self))")
    }

    // MARK: API Functions

    //typedef struct {
    //  void *object;
    //  void (*destroy)(void *object);
    //  void (*onCallEvent)(void *object);
    //  void (*onCallError)(void *object);
    //  void (*onAddStream)(void *object);
    //} IOSObserver;

    func getWrapper() -> IOSObserver {
        return IOSObserver(
            object: UnsafeMutableRawPointer(Unmanaged.passRetained(self).toOpaque()),
            destroy: callConnectionObserverDestroy,
            onCallEvent: callConnectionObserverOnCallEvent,
            onCallError: callConnectionObserverOnCallError,
            onAddStream: callConnectionObserverOnAddStream)
    }

    // MARK: Delegate Handlers

    func onCallEvent(callId: Int64, callEvent: Int32) {
        guard let delegate = self.delegate else {
            return
        }

        if let validCallEvent = CallEvent(rawValue: callEvent) {
            delegate.onCallEvent(self, callId: callId, callEvent: validCallEvent)
        } else {
            owsFailDebug("invalid callEvent: \(callEvent)")
        }
    }

    func onCallError(callId: Int64, errorString: String) {
        guard let delegate = self.delegate else {
            return
        }

        delegate.onCallError(self, callId: callId, errorString: errorString)
    }

    func onAddStream(callId: Int64, stream: RTCMediaStream) {
        guard let delegate = self.delegate else {
            return
        }

        delegate.onAddStream(self, callId: callId, stream: stream)
    }
}

func callConnectionObserverDestroy(object: UnsafeMutableRawPointer?) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }

    _ = Unmanaged<CallConnectionObserver>.fromOpaque(object).takeRetainedValue()
    // @note There should not be any retainers left for the object
    // so deinit should be called implicitly.
}

func callConnectionObserverOnCallEvent(object: UnsafeMutableRawPointer?, callId: Int64, callEvent: Int32) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }
    let obj: CallConnectionObserver = Unmanaged.fromOpaque(object).takeUnretainedValue()

    obj.onCallEvent(callId: callId, callEvent: callEvent)
}

func callConnectionObserverOnCallError(object: UnsafeMutableRawPointer?, callId: Int64, errorString: IOSByteSlice) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }
    let obj: CallConnectionObserver = Unmanaged.fromOpaque(object).takeUnretainedValue()

    guard let string = errorString.asString() else {
        owsFailDebug("unexpected string")
        return
    }

    obj.onCallError(callId: callId, errorString: string)
}

func callConnectionObserverOnAddStream(object: UnsafeMutableRawPointer?, callId: Int64, stream: UnsafeMutableRawPointer?) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }

    guard let stream = stream else {
        owsFailDebug("stream was unexpectedly nil")
        return
    }

    let obj: CallConnectionObserver = Unmanaged.fromOpaque(object).takeUnretainedValue()
    let mediaStream: RTCMediaStream = Unmanaged.fromOpaque(stream).takeUnretainedValue()

    obj.onAddStream(callId: callId, stream: mediaStream)
}
