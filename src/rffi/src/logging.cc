/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#include "rffi/api/logging.h"

namespace webrtc {
namespace rffi {

RUSTEXPORT void Rust_setLogger(LoggerCallbacks* cbs_borrowed, rtc::LoggingSeverity min_sev) {
  Logger* logger_owned = new Logger(cbs_borrowed);
  // LEAK: it's only called once, so it shouldn't matter.
  Logger* logger_borrowed = logger_owned;
  // Stores the sink, but does not delete it.
  rtc::LogMessage::AddLogToStream(logger_borrowed, min_sev);
}

} // namespace rffi
} // namespace webrtc