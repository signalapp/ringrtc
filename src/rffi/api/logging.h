/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#ifndef RFFI_LOGGING_H__
#define RFFI_LOGGING_H__

#include "rffi/api/rffi_defs.h"
#include "rtc_base/logging.h"

typedef struct {
  void (*onLogMessage)(rtc::LoggingSeverity severity, const char* message_borrowed);
} LoggerCallbacks;

namespace webrtc {
namespace rffi {

// As simple implementation of rtc::LogSink that just passes the message
// to Rust.
class Logger : public rtc::LogSink {
 public:
  Logger(LoggerCallbacks* cbs) : cbs_(*cbs) {}

  void OnLogMessage(const std::string& message) override {
    OnLogMessage(message, rtc::LS_NONE);
  }

  void OnLogMessage(const std::string& message, rtc::LoggingSeverity severity) override {
    cbs_.onLogMessage(severity, message.c_str());
  }

 private:
  LoggerCallbacks cbs_;
};

// Should only be called once.
RUSTEXPORT void Rust_setLogger(LoggerCallbacks* cbs_borrowed, rtc::LoggingSeverity min_sev);

} // namespace rffi
} // namespace webrtc

#endif /* RFFI_LOGGING_H__ */