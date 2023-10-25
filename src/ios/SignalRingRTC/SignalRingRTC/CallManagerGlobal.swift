//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import SignalRingRTC.RingRTC
import WebRTC
import SignalCoreKit

// Global singleton to guarantee certain things are only invoked
// once...
@available(iOSApplicationExtension, unavailable)
public class CallManagerGlobal {

    // CallManagerGlobal is a singleton.
    static let shared = CallManagerGlobal()

    let webRtcLogger: RTCCallbackLogger
    private let lock = NSLock()
    private var hasInitializedFieldTrials = false

    // MARK: Object Lifetime

    private init() {
        // This initialization will be done only once per application lifetime.

        let maxLogLevel: LogLevel
        #if DEBUG
        if let overrideLogLevelString = ProcessInfo().environment["RINGRTC_MAX_LOG_LEVEL"],
           let overrideLogLevelRaw = UInt8(overrideLogLevelString),
           let overrideLogLevel = LogLevel(rawValue: overrideLogLevelRaw) {
            maxLogLevel = overrideLogLevel
        } else {
            maxLogLevel = .trace
        }
        #else
        maxLogLevel = .trace
        #endif

        initLogging(maxLogLevel: maxLogLevel)

        // Don't write WebRTC logs to stdout.
        RTCSetMinDebugLogLevel(.none)

        // Show WebRTC logs via application Logger.
        webRtcLogger = RTCCallbackLogger()

        let webRtcLogLevel: LogLevel
        #if DEBUG
        webRtcLogLevel = min(maxLogLevel, .info)
        #else
        webRtcLogLevel = min(maxLogLevel, .warn)
        #endif

        webRtcLogger.severity = webRtcLogLevel.toWebRTC

        webRtcLogger.start { (message, severity) in
            let message = message.replacingOccurrences(of: "::", with: ":")
            switch severity {
            case .verbose:
                OWSLogger.verbose(message)
            case .info:
                OWSLogger.info(message)
            case .warning:
                OWSLogger.warn(message)
            case .error:
                OWSLogger.error(message)
            case .none:
                // should not happen
                break
            @unknown default:
                break
            }
        }

        Logger.debug("object! CallManagerGlobal created... \(ObjectIdentifier(self))")
    }

    static func initialize(fieldTrials: [String: String]) {
        // Implicitly initialize the shared instance, then use it to track whether we've set up the field trials.
        Self.shared.initFieldTrials(fieldTrials)
    }

    private func initFieldTrials(_ fieldTrials: [String: String]) {
        lock.lock()
        defer { lock.unlock() }

        guard !hasInitializedFieldTrials else {
            return
        }
        hasInitializedFieldTrials = true

        let fieldTrialsWithDefaults = fieldTrials.merging([
            "RingRTC-AnyAddressPortsKillSwitch": "Enabled",
            "WebRTC-Audio-OpusSetSignalVoiceWithDtx": "Enabled",
        ]) { (provided, _) in provided }
        RTCInitFieldTrialDictionary(fieldTrialsWithDefaults)
        Logger.info("Initialized field trials with \(fieldTrialsWithDefaults)")
    }

    deinit {
        webRtcLogger.stop()

        Logger.debug("object! CallManagerGlobal destroyed. \(ObjectIdentifier(self))")
    }
}
