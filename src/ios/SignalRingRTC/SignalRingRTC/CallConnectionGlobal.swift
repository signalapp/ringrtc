//
//  Copyright (c) 2019 Open Whisper Systems. All rights reserved.
//

import SignalRingRTC.RingRTC
import SignalCoreKit

public class CallConnectionGlobal {

    // CallConnectionGlobal is a singleton.
    static let shared = CallConnectionGlobal()

    // MARK: Object Lifetime

    init() {
        // This initialization should be done only once per application
        // lifetime.

        // Create a logger object and transfer ownership to RingRTC.
        let logger = CallConnectionLogger()

        let retPtr = ringRtcInitialize(logger.getWrapper())
        if retPtr == nil {
            owsFailDebug("ringRtcInitialize failure")
        }

        Logger.debug("object! CallConnectionGlobal created... \(ObjectIdentifier(self))")
    }

    deinit {
        Logger.debug("object! CallConnectionGlobal destroyed. \(ObjectIdentifier(self))")
    }
}
