/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#include "rffi/api/logging.h"

namespace webrtc {
namespace rffi {

RUSTEXPORT void Rust_setLogger(LoggerCallbacks* cbs, rtc::LoggingSeverity min_sev) {
  // Just let it leak.  Who cares.  This should only be called once.
  Logger* logger = new Logger(cbs);
  rtc::LogMessage::AddLogToStream(logger, min_sev);
}

} // namespace rffi
} // namespace webrtc