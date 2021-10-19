/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#ifndef RFFI_API_INJECTABLE_NETWORK_H__
#define RFFI_API_INJECTABLE_NETWORK_H__

#include "p2p/base/port_allocator.h"
#include "rtc_base/thread.h"
#include "rffi/api/network.h"
#include "rffi/api/rffi_defs.h"

namespace webrtc {

namespace rffi {

typedef struct {
  void* object_owned;
  int (*SendUdp)(void* object_borrowed, IpPort source, IpPort dest, const uint8_t* data_borrowed, size_t);
  int (*Delete)(void* object_owned);
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
    InjectableNetwork* network_borrowed,
    const InjectableNetworkSender* sender_borrowed);

RUSTEXPORT void Rust_InjectableNetwork_AddInterface(
  InjectableNetwork* network_borrowed,
  const char* name_borrowed,
  rtc::AdapterType type,
  Ip ip,
  int preference);

RUSTEXPORT void Rust_InjectableNetwork_RemoveInterface(
  InjectableNetwork* network_borrowed, 
  const char* name_borrowed);

RUSTEXPORT void Rust_InjectableNetwork_ReceiveUdp(
  InjectableNetwork* network_borrowed,
  IpPort source,
  IpPort dest,
  const uint8_t* data_borrowed,
  size_t size);

}  // namespace rffi

}  // namespace webrtc

#endif /* RFFI_API_INJECTABLE_NETWORK_H__ */
