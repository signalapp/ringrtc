//
// Copyright 2019-2022 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

// FFI that allows RingRTC to make logging requests through
// a Delegate implemented by the application.

import SignalRingRTC.RingRTC
import WebRTC

public enum RingRTCLogLevel: UInt8, Comparable {
    // These values correspond to values from log::Level (in the log crate)
    case error = 1
    case warn = 2
    case info = 3
    case debug = 4
    case trace = 5

    static func fromRingRtc(_ rtcLevel: UInt8) -> Self? {
        return RingRTCLogLevel(rawValue: rtcLevel)
    }

    var toWebRTC: RTCLoggingSeverity {
        switch self {
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

    public static func < (lhs: RingRTCLogLevel, rhs: RingRTCLogLevel) -> Bool {
        // Lower log levels == less log output
        return lhs.rawValue < rhs.rawValue
    }
}

// Same as rust log::Record (nicer version of rtc_log_Record)
internal struct LogRecord {
    public let message: String
    public let file: String
    public let line: Int
    public let level: RingRTCLogLevel

    static func fromRtc(_ rtcRecord: rtc_log_Record) -> Self? {
        if rtcRecord.level == 0 {
            // The log crate uses 0 for "off"
            return nil
        }

        let level = RingRTCLogLevel.fromRingRtc(rtcRecord.level)
        if level == nil {
            failDebug("Invalid log level: \(rtcRecord.level).  Using Error")
        }

        return LogRecord(
            message: rtcRecord.message.toString() ?? "",
            file: rtcRecord.file.toString() ?? "",
            line: Int(truncatingIfNeeded: rtcRecord.line.asUInt32() ?? 0),
            level: level ?? .error
        )
    }
}

public protocol RingRTCLogger: Sendable {
    /// Requests that a log message be output at the given log level.
    ///
    /// This method may be called on any thread, and will be called synchronously from the middle of complicated operations; endeavor to make it quick!
    func log(level: RingRTCLogLevel, file: String, function: String, line: UInt32, message: String)

    /// Requests that the log be flushed.
    ///
    /// This may be called before a fatal error, so it should be handled synchronously if possible, even if that causes a delay.
    ///
    /// This method may be called on any thread.
    func flush()
}

extension RingRTCLogger {
    /// Can only be called once in the lifetime of a program; later calls will result in a warning and will not change the active logger.
    public func setUpRingRTCLogging(maxLogLevel: RingRTCLogLevel = .info) {
        let logger = Logger(logger: self)
        let opaqueLogger = Unmanaged.passRetained(logger)
        let success = rtc_log_init(
            rtc_log_Delegate(
                ctx: opaqueLogger.toOpaque(),
                log: { ctx, rtcRecord in
                    guard let record = LogRecord.fromRtc(rtcRecord) else {
                        return
                    }

                    let logger: Logger = Unmanaged.fromOpaque(ctx!).takeUnretainedValue()

                    let message = record.message
                    let file = (record.file as NSString).lastPathComponent
                    let line = record.line

                    logger.logger.log(level: record.level, file: file, function: "ringrtc", line: UInt32(line), message: message)
                },
                flush: { ctx in
                    let logger: Logger = Unmanaged.fromOpaque(ctx!).takeUnretainedValue()
                    logger.logger.flush()
                }
            ),
            maxLogLevel.rawValue
        )
        if success {
            // We save this for use within the Swift code as well, but only if
            // it was registered as the Rust logger successfully.
            Logger.shared = logger
        } else {
            // Balance the `passRetained` from above.
            opaqueLogger.release()
        }
    }

}

/// A context-pointer-compatible wrapper around a logger.
internal class Logger {
    let logger: any RingRTCLogger
    init(logger: any RingRTCLogger) {
        self.logger = logger
    }

    private static var globalLoggerLock = NSLock()
    private static var _globalLogger: Logger? = nil

    internal fileprivate(set) static var shared: Logger? {
        get {
            globalLoggerLock.withLock {
                return _globalLogger
            }
        }
        set {
            globalLoggerLock.withLock {
                _globalLogger = newValue
            }
        }
    }

    internal static func verbose(_ message: String, file: StaticString = #fileID, function: StaticString = #function, line: UInt32 = #line) {
        log(level: .trace, file: file, function: function, line: line, message: message)
    }

    internal static func debug(_ message: String, file: StaticString = #fileID, function: StaticString = #function, line: UInt32 = #line) {
        log(level: .debug, file: file, function: function, line: line, message: message)
    }

    internal static func info(_ message: String, file: StaticString = #fileID, function: StaticString = #function, line: UInt32 = #line) {
        log(level: .info, file: file, function: function, line: line, message: message)
    }

    internal static func warn(_ message: String, file: StaticString = #fileID, function: StaticString = #function, line: UInt32 = #line) {
        log(level: .warn, file: file, function: function, line: line, message: message)
    }

    internal static func error(_ message: String, file: StaticString = #fileID, function: StaticString = #function, line: UInt32 = #line) {
        log(level: .error, file: file, function: function, line: line, message: message)
    }

    private static func log(level: RingRTCLogLevel, file: StaticString, function: StaticString, line: UInt32, message: String) {
        guard let Logger = Logger.shared else {
            return
        }

        Logger.logger.log(level: level, file: (String(describing: file) as NSString).lastPathComponent, function: String(describing: function), line: line, message: message)
    }

    internal static func flush() {
        guard let Logger = Logger.shared else {
            return
        }

        Logger.logger.flush()
    }
}
