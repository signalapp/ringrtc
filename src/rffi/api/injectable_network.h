
/*
 *
 *  Copyright (C) 2020 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */

#ifndef RFFI_API_INJECTABLE_NETWORK_H__
#define RFFI_API_INJECTABLE_NETWORK_H__

#include "p2p/base/port_allocator.h"
#include "rtc_base/thread.h"
#include "rffi/api/rffi_defs.h"
namespace webrtc {
namespace rffi {

// A simplified version of rtc::IpAddress
typedef struct {
  // If v6 == false, only use the first 4 bytes.
  bool v6;
  uint8_t address[16];
} Ip;

// As simplified version of rtc::SocketAddress
typedef struct {
  Ip ip;
  uint16_t port;
} IpPort;

typedef struct {
  rust_object object;
  int (*SendUdp)(rust_object, IpPort source, IpPort dest, const uint8_t*, size_t);
  int (*Release)(rust_object);
} InjectableNetworkSender;

// This is a class that acts like a PortAllocator + PacketSocketFactory + NetworkManager
// to the network stack and allows simulated or injected networks to control the flow
// of packets and which network interfaces come up and down.
class InjectableNetwork {
 public:
  virtual ~InjectableNetwork() = default;

  // This is what the network stack sees.
  // The PacketSocketFactory and NetworkManager are referenced by the PortAllocator.
  virtual std::unique_ptr<cricket::PortAllocator> CreatePortAllocator() = 0;

  // This is what the "driver" of the network sees: control of packets, 
  // network interfaces, etc.
  virtual void SetSender(const InjectableNetworkSender* sender) = 0;
  virtual void AddInterface(
    const char* name, rtc::AdapterType type, Ip ip, int preference) = 0;
  virtual void RemoveInterface(const char* name) = 0;
  virtual void ReceiveUdp(
    IpPort source, IpPort dest, const uint8_t* data, size_t size) = 0;

  // These are more for internal use, not external, which is why the types
  // aren't the external types.
  virtual int SendUdp(
    const rtc::SocketAddress& local_address,
    const rtc::SocketAddress& remote_address,
    const uint8_t* data,
    size_t size) = 0;
  virtual void ForgetUdp(const rtc::SocketAddress& local_address) = 0;
};

std::unique_ptr<InjectableNetwork> CreateInjectableNetwork(rtc::Thread* network_thread);

RUSTEXPORT void Rust_InjectableNetwork_SetSender(
    InjectableNetwork* network,
    const InjectableNetworkSender* sender);

RUSTEXPORT void Rust_InjectableNetwork_AddInterface(
  InjectableNetwork* network,
  const char* name,
  rtc::AdapterType type,
  Ip ip,
  int preference);

RUSTEXPORT void Rust_InjectableNetwork_RemoveInterface(
  InjectableNetwork* network, const char* name);

RUSTEXPORT void Rust_InjectableNetwork_ReceiveUdp(
  InjectableNetwork* network,
  IpPort source,
  IpPort dest,
  const uint8_t* data,
  size_t size);

}  // namespace rffi

}  // namespace webrtc

#endif /* RFFI_API_INJECTABLE_NETWORK_H__ */
