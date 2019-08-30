//
//  Copyright (c) 2019 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC
import SignalCoreKit

public enum CallConnectionLogLevel: Int8 {
    case error = 1
    case warn = 2
    case info = 3
    case debug = 4
    case trace = 5
}

class CallConnectionLogger {

    // MARK: Object Lifetime

    init() {
        Logger.debug("object! CallConnectionLogger created... \(ObjectIdentifier(self))")
    }

    deinit {
        Logger.debug("object! CallConnectionLogger destroyed. \(ObjectIdentifier(self))")
    }

    // MARK: API Functions

//    typedef struct {
//      void *object;
//      void (*destroy)(void *object);
//      void (*log)(void *object, IOSByteSlice message, IOSByteSlice file, IOSByteSlice function, int32_t line, int8_t level);
//    } IOSLogger;

    func getWrapper() -> IOSLogger {
        return IOSLogger(
            object: UnsafeMutableRawPointer(Unmanaged.passRetained(self).toOpaque()),
            destroy: callConnectionLoggerDestroy,
            log: callConnectionLoggerCallback)
    }

    // MARK: Utility Functions

    func logHandler(message: String, file: String, function: String, line: Int, rawLevel: Int8) {
        let rtcLevel: CallConnectionLogLevel
        if let validLevel = CallConnectionLogLevel(rawValue: rawLevel) {
            rtcLevel = validLevel
        } else {
            owsFailDebug("invalid log level: \(rawLevel)")
            rtcLevel = .error
        }

        switch (rtcLevel) {
        case CallConnectionLogLevel.error: Logger.error(message, file: file, function: function, line: line)
        case CallConnectionLogLevel.warn: Logger.warn(message, file: file, function: function, line: line)
        case CallConnectionLogLevel.debug: Logger.debug(message, file: file, function: function, line: line)
        case CallConnectionLogLevel.info: Logger.info(message, file: file, function: function, line: line)
        case CallConnectionLogLevel.trace: Logger.verbose(message, file: file, function: function, line: line)
        }
    }
}

func callConnectionLoggerDestroy(object: UnsafeMutableRawPointer?) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }

    _ = Unmanaged<CallConnectionLogger>.fromOpaque(object).takeRetainedValue()
    // @note There should not be any retainers left for the object
    // so deinit should be called implicitly.
}

func callConnectionLoggerCallback(object: UnsafeMutableRawPointer?, message: IOSByteSlice, file: IOSByteSlice, function: IOSByteSlice, line: Int32, level: Int8) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }
    let obj: CallConnectionLogger = Unmanaged.fromOpaque(object).takeUnretainedValue()

    // @note .asString() returns a new String object, so there is
    // no lifetime issue after callback returns.

    let messageString = message.asString() ?? ""
    let fileString = file.asString() ?? ""
    let functionString = function.asString() ?? ""

    obj.logHandler(message: messageString, file: fileString, function: functionString, line: Int(truncatingIfNeeded: line), rawLevel: level)
}
