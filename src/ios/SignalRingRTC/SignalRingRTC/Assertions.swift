//
// Copyright 2024 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

import SignalRingRTC.RingRTC

internal func fail(
    _ message: String,
    file: StaticString = #fileID,
    function: StaticString = #function,
    line: Int = #line
) -> Never {
    failDebug(message, file: file, function: function, line: line)

    Logger.error(Thread.callStackSymbols.joined(separator: "\n"))
    Logger.flush()

    fatalError(message)
}

internal func failDebug(
    _ message: String,
    file: StaticString = #fileID,
    function: StaticString = #function,
    line: Int = #line
) {
    Logger.error(message, file: file, function: function, line: UInt32(line))
    assertionFailure(message, file: file, line: UInt(line))
}
