//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import SignalRingRTC.RingRTC
import WebRTC

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

        let maxLogLevel: RingRTCLogLevel
        #if DEBUG
        if let overrideLogLevelString = ProcessInfo().environment["RINGRTC_MAX_LOG_LEVEL"],
           let overrideLogLevelRaw = UInt8(overrideLogLevelString),
           let overrideLogLevel = RingRTCLogLevel(rawValue: overrideLogLevelRaw) {
            maxLogLevel = overrideLogLevel
        } else {
            maxLogLevel = .trace
        }
        #else
        maxLogLevel = .trace
        #endif

        // Don't write WebRTC logs to stdout.
        RTCSetMinDebugLogLevel(.none)

        // Show WebRTC logs via application Logger.
        webRtcLogger = RTCCallbackLogger()

        let webRtcLogLevel: RingRTCLogLevel
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
                Logger.verbose(message, file: "", function: "", line: 0)
            case .info:
                Logger.info(message, file: "", function: "", line: 0)
            case .warning:
                Logger.warn(message, file: "", function: "", line: 0)
            case .error:
                Logger.error(message, file: "", function: "", line: 0)
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
            "RingRTC-PruneTurnPorts": "Enabled",
            "WebRTC-Bwe-ProbingConfiguration": "skip_if_est_larger_than_fraction_of_max:0.99",
            "WebRTC-IncreaseIceCandidatePriorityHostSrflx": "Enabled",
        ]) { (provided, _) in provided }
        RTCInitFieldTrialDictionary(fieldTrialsWithDefaults)
        Logger.info("Initialized field trials with \(fieldTrialsWithDefaults)")
    }

    deinit {
        webRtcLogger.stop()

        Logger.debug("object! CallManagerGlobal destroyed. \(ObjectIdentifier(self))")
    }
}
