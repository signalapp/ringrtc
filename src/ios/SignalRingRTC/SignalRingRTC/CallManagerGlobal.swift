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
        // This initialization will be done only once per application
        // lifetime.
        initLogging()

        // Don't write WebRTC logs to stdout.
        RTCSetMinDebugLogLevel(.none)

        // Show WebRTC logs via application Logger.
        webRtcLogger = RTCCallbackLogger()

        #if DEBUG
        webRtcLogger.severity = .info

        webRtcLogger.start { (message, severity) in
            if severity == .info {
                OWSLogger.info(message)
            } else if severity == .warning {
                OWSLogger.warn(message)
            } else if severity == .error {
                OWSLogger.error(message)
            }
        }
        #else
        webRtcLogger.severity = .warning

        webRtcLogger.start { (message, severity) in
            if severity == .warning {
                OWSLogger.warn(message)
            } else if severity == .error {
                OWSLogger.error(message)
            }
        }
        #endif

        Logger.debug("object! CallManagerGlobal created... \(ObjectIdentifier(self))")
    }

    static func initialize(fieldTrials: [String: Bool]) {
        // Implicitly initialize the shared instance, then use it to track whether we've set up the field trials.
        Self.shared.initFieldTrials(fieldTrials)
    }

    private func initFieldTrials(_ fieldTrials: [String: Bool]) {
        lock.lock()
        defer { lock.unlock() }

        guard !hasInitializedFieldTrials else {
            return
        }
        hasInitializedFieldTrials = true

        RTCInitFieldTrialDictionary(fieldTrials.mapValues({ $0 ? "Enabled" : "Disabled" }))
        Logger.info("Initialized field trials with \(fieldTrials)")
    }

    deinit {
        webRtcLogger.stop()

        Logger.debug("object! CallManagerGlobal destroyed. \(ObjectIdentifier(self))")
    }
}
