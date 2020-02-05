//
//  Copyright (c) 2020 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC
import SignalCoreKit

public enum CallManagerLogLevel: Int8 {
    case error = 1
    case warn = 2
    case info = 3
    case debug = 4
    case trace = 5
}

class CallManagerLogger {

    // MARK: Object Lifetime

    init() {
        Logger.debug("object! CallManagerLogger created... \(ObjectIdentifier(self))")
    }

    deinit {
        Logger.debug("object! CallManagerLogger destroyed. \(ObjectIdentifier(self))")
    }

    // MARK: API Functions

    func getWrapper() -> IOSLogger {
        return IOSLogger(
            object: UnsafeMutableRawPointer(Unmanaged.passRetained(self).toOpaque()),
            destroy: callManagerLoggerDestroy,
            log: callManagerLoggerCallback)
    }

    // MARK: Utility Functions

    func logHandler(message: String, file: String, function: String, line: Int, rawLevel: Int8) {
        let rtcLevel: CallManagerLogLevel
        if let validLevel = CallManagerLogLevel(rawValue: rawLevel) {
            rtcLevel = validLevel
        } else {
            owsFailDebug("invalid log level: \(rawLevel)")
            rtcLevel = .error
        }

        switch (rtcLevel) {
        case CallManagerLogLevel.error: Logger.error(message, file: file, function: function, line: line)
        case CallManagerLogLevel.warn: Logger.warn(message, file: file, function: function, line: line)
        case CallManagerLogLevel.debug: Logger.debug(message, file: file, function: function, line: line)
        case CallManagerLogLevel.info: Logger.info(message, file: file, function: function, line: line)
        case CallManagerLogLevel.trace: Logger.verbose(message, file: file, function: function, line: line)
        }
    }
}

func callManagerLoggerDestroy(object: UnsafeMutableRawPointer?) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }

    _ = Unmanaged<CallManagerLogger>.fromOpaque(object).takeRetainedValue()
    // @note There should not be any retainers left for the object
    // so deinit should be called implicitly.
}

func callManagerLoggerCallback(object: UnsafeMutableRawPointer?, message: AppByteSlice, file: AppByteSlice, function: AppByteSlice, line: Int32, level: Int8) {
    guard let object = object else {
        owsFailDebug("object was unexpectedly nil")
        return
    }
    let obj: CallManagerLogger = Unmanaged.fromOpaque(object).takeUnretainedValue()

    // @note .asString() returns a new String object, so there is
    // no lifetime issue after callback returns.

    let messageString = message.asString() ?? ""
    let fileString = file.asString() ?? ""
    let functionString = function.asString() ?? ""

    obj.logHandler(message: messageString, file: fileString, function: functionString, line: Int(truncatingIfNeeded: line), rawLevel: level)
}
