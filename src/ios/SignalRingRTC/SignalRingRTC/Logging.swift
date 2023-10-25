//
// Copyright 2019-2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

// FFI that allows RingRTC to make logging requests through
// a Delegate implemented by the application.

import SignalRingRTC.RingRTC
import SignalCoreKit
import WebRTC

// Same as rust log::Level (from the log crate)
public enum LogLevel: UInt8, Comparable {
    case off = 0
    case error = 1
    case warn = 2
    case info = 3
    case debug = 4
    case trace = 5

    static func fromRingRtc(_ rtcLevel: UInt8) -> Self? {
        return LogLevel(rawValue: rtcLevel)
    }

    static func fromSignal() -> Self {
        if ShouldLogVerbose() {
            return .trace
        } else if ShouldLogDebug() {
            return .debug
        } else if ShouldLogInfo() {
            return .info
        } else if ShouldLogWarning() {
            return .warn
        } else if ShouldLogError() {
            return .error
        } else {
            return .off
        }
    }

    var toWebRTC: RTCLoggingSeverity {
        switch self {
        case .off:
            return .none
        case .error:
            return .error
        case .warn:
            return .warning
        case .info:
            return .info
        case .debug, .trace:
            return .verbose
        }
    }

    public static func < (lhs: LogLevel, rhs: LogLevel) -> Bool {
        // Lower log levels == less log output
        return lhs.rawValue < rhs.rawValue
    }
}

// Same as rust log::Record (nicer version of rtc_log_Record)
public struct LogRecord {
    public let message: String
    public let file: String
    public let line: Int
    public let level: LogLevel

    static func fromRtc(_ rtcRecord: rtc_log_Record) -> Self {
        let level = LogLevel.fromRingRtc(rtcRecord.level)
        if level == nil {
            owsFailDebug("Invalid log level: \(rtcRecord.level).  Using Error")
        }

        return LogRecord(
            message: rtcRecord.message.toString() ?? "",
            file: rtcRecord.file.toString() ?? "",
            line: Int(truncatingIfNeeded: rtcRecord.line.asUInt32() ?? 0),
            level: level ?? .error
        )
    }
}

// The application doesn't need to do anything and there's no state,
// so we don't need a LogDelegate or LogDelegateWrapper.
public func initLogging(maxLogLevel: LogLevel = .trace) {
    let maxLevel = min(maxLogLevel, LogLevel.fromSignal())
    rtc_log_init(
        rtc_log_Delegate(
            log: { (rtcRecord: rtc_log_Record) in
                let record = LogRecord.fromRtc(rtcRecord)
                let message = record.message
                let file = record.file
                let line = record.line

                switch record.level {
                case .error: Logger.error(message, file: file, function: "ringrtc", line: line)
                case .warn: Logger.warn(message, file: file, function: "ringrtc", line: line)
                case .info: Logger.info(message, file: file, function: "ringrtc", line: line)
                case .debug: Logger.debug(message, file: file, function: "ringrtc", line: line)
                case .trace: Logger.verbose(message, file: file, function: "ringrtc", line: line)
                case .off:
                    // Do nothing.  This doesn't really come from Rust.
                    break
                }
            }
        ),
        maxLevel.rawValue
    )
}

