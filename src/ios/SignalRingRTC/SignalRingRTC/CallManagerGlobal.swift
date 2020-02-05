//
//  Copyright (c) 2020 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC
import SignalCoreKit

// Global singleton to guarantee certain things are only invoked
// once...
public class CallManagerGlobal {

    // CallManagerGlobal is a singleton.
    static let shared = CallManagerGlobal()

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

        Logger.debug("object! CallManagerGlobal created... \(ObjectIdentifier(self))")
    }

    deinit {
        Logger.debug("object! CallManagerGlobal destroyed. \(ObjectIdentifier(self))")
    }
}
