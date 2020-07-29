//
//  Copyright (c) 2020 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC
import WebRTC
import SignalCoreKit

// Global singleton to guarantee certain things are only invoked
// once...
public class CallManagerGlobal {

    // CallManagerGlobal is a singleton.
    static let shared = CallManagerGlobal()

    #if DEBUG
    let webRtcLogger: RTCCallbackLogger
    #endif

    // MARK: Object Lifetime

    private init() {
        // This initialization will be done only once per application
        // lifetime.

        // Create a logger object and transfer ownership to RingRTC.
        let logger = CallManagerLogger()

        let retPtr = ringrtcInitialize(logger.getWrapper())
        if retPtr == nil {
            owsFailDebug("ringRtcInitialize failure")
        }

        // Don't write WebRTC logs to stdout.
        RTCSetMinDebugLogLevel(.none)

        #if DEBUG
        // Show WebRTC logs via application Logger while debugging.
        webRtcLogger = RTCCallbackLogger()

        webRtcLogger.severity = .info

        webRtcLogger.start { (message, severity) in
            let matches = message.match("^\\((.*)\\:(\\d*)\\):\\s*(.*)$")

            // Log the matched parts with the appropriate severity; Log the message as-is if regex didn't match.
            switch severity {
            case .verbose:
                if matches.count == 1 && matches[0].count == 4 {
                    Logger.verbose(matches[0][3], file: matches[0][1], function: "webrtc", line: Int(matches[0][2]) ?? 0)
                } else {
                    Logger.verbose(message)
                }
            case .info:
                if matches.count == 1 && matches[0].count == 4 {
                    Logger.info(matches[0][3], file: matches[0][1], function: "webrtc", line: Int(matches[0][2]) ?? 0)
                } else {
                    Logger.info(message)
                }
            case .warning:
                if matches.count == 1 && matches[0].count == 4 {
                    Logger.warn(matches[0][3], file: matches[0][1], function: "webrtc", line: Int(matches[0][2]) ?? 0)
                } else {
                    Logger.warn(message)
                }
            case .error:
                if matches.count == 1 && matches[0].count == 4 {
                    Logger.error(matches[0][3], file: matches[0][1], function: "webrtc", line: Int(matches[0][2]) ?? 0)
                } else {
                    Logger.error(message)
                }
            case .none:
                if matches.count == 1 && matches[0].count == 4 {
                    Logger.debug(matches[0][3], file: matches[0][1], function: "webrtc", line: Int(matches[0][2]) ?? 0)
                } else {
                    Logger.debug(message)
                }
            @unknown default:
                Logger.debug(message)
            }
        }
        #endif

        Logger.debug("object! CallManagerGlobal created... \(ObjectIdentifier(self))")
    }

    deinit {
        #if DEBUG
        webRtcLogger.stop()
        #endif

        Logger.debug("object! CallManagerGlobal destroyed. \(ObjectIdentifier(self))")
    }
}

// Reference: https://stackoverflow.com/a/56616990
private extension String {
    func match(_ regex: String) -> [[String]] {
        let nsString = self as NSString
        return (try? NSRegularExpression(pattern: regex, options: []))?.matches(in: self, options: [], range: NSMakeRange(0, count)).map { match in
            (0..<match.numberOfRanges).map { match.range(at: $0).location == NSNotFound ? "" : nsString.substring(with: match.range(at: $0)) }
        } ?? []
    }
}
