/*
 *
 *  Copyright (C) 2020 Signal Messenger, LLC.
 *  All rights reserved.
 *
 *  SPDX-License-Identifier: GPL-3.0-only
 *
 */


#include "rffi/api/injectable_network.h"

#include "api/packet_socket_factory.h"
#include "p2p/client/basic_port_allocator.h"
#include "rtc_base/ip_address.h"
#include "rtc_base/network.h"

namespace webrtc {

namespace rffi {

rtc::IPAddress IpToRtcIp(Ip ip) {
  if (ip.v6) {
    in6_addr ipv6;
    ::memcpy(&ipv6.s6_addr, &ip.address, 16);
    return rtc::IPAddress(ipv6);
  } else {
    in_addr ipv4;
    ::memcpy(&ipv4.s_addr, &ip.address, 4);
    return rtc::IPAddress(ipv4);
  }
}

rtc::SocketAddress IpPortToRtcSocketAddress(IpPort ip_port) {
  return rtc::SocketAddress(IpToRtcIp(ip_port.ip), ip_port.port);
}

Ip RtcIpToIp(rtc::IPAddress address) {
  Ip ip;
  memset(&ip.address, 0, sizeof(ip.address));
  if (address.family() == AF_INET6) {
    in6_addr ipv6 = address.ipv6_address();
    ip.v6 = true;
    ::memcpy(&ip.address, &ipv6.s6_addr, 16);
  } else {
    in_addr ipv4 = address.ipv4_address();
    ip.v6 = false;
    ::memcpy(&ip.address, &ipv4.s_addr, 4);
  }
  return ip;
}

IpPort RtcSocketAddressToIpPort(const rtc::SocketAddress& address) {
  IpPort ip_port;
  ip_port.ip = RtcIpToIp(address.ipaddr());
  ip_port.port = address.port();
  return ip_port;
}

class InjectableUdpSocket : public rtc::AsyncPacketSocket {
 public:
  InjectableUdpSocket(InjectableNetwork* network, const rtc::SocketAddress& local_address)
      : network_(network), local_address_(local_address) {
  }
  ~InjectableUdpSocket() override {
    network_->ForgetUdp(local_address_);
  }

  // As rtc::AsyncPacketSocket
  rtc::SocketAddress GetLocalAddress() const override {
    return local_address_;
  }

  // As rtc::AsyncPacketSocket
  rtc::SocketAddress GetRemoteAddress() const override {
    // Only used for TCP.
    return rtc::SocketAddress();
  }

  // As rtc::AsyncPacketSocket
  int Send(const void* data,
           size_t data_size,
           const rtc::PacketOptions& options) override {
    // Only used for TCP
    return -1;
  }

  // As rtc::AsyncPacketSocket
  int SendTo(const void* data,
             size_t data_size,
             const rtc::SocketAddress& remote_address,
             const rtc::PacketOptions& options) override {
    // RTC_LOG(LS_VERBOSE) << "InjectableUdpSocket::SendTo()"
    //                     << " from " << local_address_.ToString()
    //                     << " to " << remote_address.ToString();
    int result = network_->SendUdp(local_address_, remote_address, static_cast<const uint8_t*>(data), data_size);
    if (result < 0) {
      last_error_ = result;
      return result;
    }

    // Ends up going to Call::OnSentPacket for congestion control purposes.
    SignalSentPacket(this, rtc::SentPacket(options.packet_id, rtc::TimeMillis()));
    return result;
  }

  void ReceiveFrom(const uint8_t* data,
                   size_t data_size,
                   const rtc::SocketAddress& remote_address) {
    RTC_LOG(LS_VERBOSE) << "InjectableUdpSocket::ReceiveFrom()"
                        << " from " << remote_address.ToString()
                        << " to " << local_address_.ToString();
    auto now = rtc::TimeMicros();
    SignalReadPacket(this, reinterpret_cast<const char*>(data), data_size, remote_address, now);
  }

  // As rtc::AsyncPacketSocket
  int Close() override {
    // This appears to never be called.
    // And the real "close" is the destructor.
    return -1;
  }

  // As rtc::AsyncPacketSocket
  State GetState() const override {
    // UDPPort waits until it's bound to generate a candidate and send binding requests.
    // If it's not currently bound, it will listen for SignalAddressReady.
    // TODO: Simulate slow binds?
    return  rtc::AsyncPacketSocket::STATE_BOUND;
  }

  // As rtc::AsyncPacketSocket
  int GetOption(rtc::Socket::Option option, int* value) override {
    // This appears to never be called.
    return -1;
  }

  // As rtc::AsyncPacketSocket
  int SetOption(rtc::Socket::Option option, int value) override {
    // This is used to:
    //  Set OPT_NODELAY on TCP connections (we can ignore that)
    //  Set OPT_DSCP when DSCP is enabled (we can ignore that)
    //  Set OPT_SNDBUF to 65536 (when video is used)
    //  Set OPT_RCVBUF to 262144 (when video is used)
    // TODO: Simulate changes to OPT_SNDBUF and OPT_RCVBUF

    // Pretend it worked.
    return 1;
  }

  // As rtc::AsyncPacketSocket
  int GetError() const override {
    // UDPPort and TurnPort will call this if SendTo fails (returns < 0).
    // And that gets bubbled all the way up to RtpTransport::SendPacket
    // which will check to see if it's ENOTCONN, at which point it will
    // stop sending RTP/RTCP until SignalReadyToSend fires (weird, right?).
    // TODO: Simulate "ready" or "not ready to send" by returning ENOTCONN
    // and firing SignalReadyToSend at the appropriate times.
    return last_error_;
  }

  // As rtc::AsyncPacketSocket
  void SetError(int error) override {
    // This appears to never be called.
  }

 private:
  InjectableNetwork* network_;
  rtc::SocketAddress local_address_;
  int last_error_ = 0;
};

class InjectableNetworkImpl : public InjectableNetwork, public rtc::NetworkManager, public rtc::PacketSocketFactory {
 public:
  InjectableNetworkImpl(rtc::Thread* network_thread) : network_thread_(network_thread) {
  }

  ~InjectableNetworkImpl() override {
    if (sender_.object) {
      sender_.Release(sender_.object);
    }
  }

  // As InjectableNetwork
  std::unique_ptr<cricket::PortAllocator> CreatePortAllocator() override {
    RTC_LOG(LS_INFO) << "InjectableNetworkImpl::CreatePortAllocator()";
    return network_thread_->Invoke<std::unique_ptr<cricket::BasicPortAllocator>>(
        RTC_FROM_HERE, [this] {
          return std::make_unique<cricket::BasicPortAllocator>(this, this);
      });
  }

  void SetSender(const InjectableNetworkSender* sender) override {
    RTC_LOG(LS_INFO) << "InjectableNetworkImpl::SetSender()";
    sender_ = *sender;
  }

  // name used for debugging a lot, but also as an ID for the network for TURN pruning.
  // type Affects Candidate network cost and other ICE behavior
  // preference affects ICE cndidate priorities higher is more preferred 
  void AddInterface(
    const char* name, rtc::AdapterType type, Ip ip, int preference) override {
    RTC_LOG(LS_INFO) << "InjectableNetworkImpl::AddInterface() name: " << name;
    // We need to access interface_by_name_ and SignalNetworksChanged on the network_thread_.
    // Make sure to copy the name first!
    network_thread_->PostTask(RTC_FROM_HERE,
        [this, name{std::string(name)}, type, ip, preference] { 
      // TODO: Support different IP prefixes.
      auto interface = std::make_unique<rtc::Network>(
          name, name /* description */,  IpToRtcIp(ip) /* prefix */, 0 /* prefix_length */, type);
      // TODO: Add more than one IP per network interface
      interface->AddIP(IpToRtcIp(ip));
      interface->set_preference(preference);
      interface_by_name_.insert({std::move(name), std::move(interface)});
      SignalNetworksChanged();
    });
  }

  void RemoveInterface(const char* name) override {
    RTC_LOG(LS_INFO) << "InjectableNetworkImpl::RemoveInterface() name: " << name;
    // We need to access interface_by_name_ on the network_thread_.
    // Make sure to copy the name first!
    network_thread_->PostTask(RTC_FROM_HERE, [this, name{std::string(name)}] { 
      interface_by_name_.erase(name);
    });
  }

  void ReceiveUdp(IpPort source,
                  IpPort dest,
                  const uint8_t* data,
                  size_t size) override {
    // The network stack expects everything to happen on the network thread.
    // Make sure to copy the data!
    network_thread_->PostTask(RTC_FROM_HERE, 
        [this, source, dest, data{std::vector<uint8_t>(data, data+size)}, size] { 
      auto local_address = IpPortToRtcSocketAddress(dest);
      auto remote_address = IpPortToRtcSocketAddress(source);
     RTC_LOG(LS_VERBOSE) << "InjectableNetworkImpl::ReceiveUdp()"
                         << " from " << remote_address.ToString()
                         << " to " << local_address.ToString()
                         << " size: " << size;
      auto udp_socket = udp_socket_by_local_address_.find(local_address);
      if (udp_socket == udp_socket_by_local_address_.end()) {
        RTC_LOG(LS_WARNING) << "Received packet for unknown local address.";
        return;
      }
      udp_socket->second->ReceiveFrom(data.data(), data.size(), remote_address);      
    });
  }

  int SendUdp(const rtc::SocketAddress& local_address,
              const rtc::SocketAddress& remote_address,
              const uint8_t* data,
              size_t size) override {
    if (!sender_.object) {
      RTC_LOG(LS_WARNING) << "Dropping packet because no sender set.";
      return -1;
    }
    IpPort local = RtcSocketAddressToIpPort(local_address);
    IpPort remote = RtcSocketAddressToIpPort(remote_address);
    // RTC_LOG(LS_VERBOSE) << "InjectableNetworkImpl::SendUdp()"
    //                     << " from " << local_address.ToString()
    //                     << " to " << remote_address.ToString()
    //                     << " size: " << size;
    sender_.SendUdp(sender_.object, local, remote, data, size);
    return size;
  }

  void ForgetUdp(const rtc::SocketAddress& local_address) override {
    // We need to access udp_socket_by_local_address_ on the network_thread_.
    network_thread_->PostTask(RTC_FROM_HERE, [this, local_address] { 
      udp_socket_by_local_address_.erase(local_address);
    });
  }

  // As NetworkManager
  void StartUpdating() override {
    RTC_DCHECK(network_thread_->IsCurrent());
    RTC_LOG(LS_INFO) << "InjectableNetworkImpl::StartUpdating()";
    // TODO: Add support for changing networks dynamically.
    //       BasicPortAllocatorSession listens to it do detect when networks have failed (gone away)
    // Documentation says this must be called by StartUpdating() once the network list is available.
    SignalNetworksChanged();
  }

  // As NetworkManager
  void StopUpdating() override {
  }

  // As NetworkManager
  void GetNetworks(std::vector<rtc::Network*>* networks) const override {
    RTC_LOG(LS_INFO) << "InjectableNetworkImpl::GetNetworks()";
    RTC_DCHECK(network_thread_->IsCurrent());
    for (const auto& kv : interface_by_name_) {
      networks->push_back(kv.second.get());
    }
  }

  // As NetworkManager
  webrtc::MdnsResponderInterface* GetMdnsResponder() const override {
    // We'll probably never use mDNS
    return nullptr;
  }

  // As NetworkManager
  void GetAnyAddressNetworks(std::vector<rtc::Network*>* networks) override {
    // TODO: Add support for using a default route instead of choosing a particular network.
    // (such as when we can't enumerate networks or IPs)
  }

  // As NetworkManager
  EnumerationPermission enumeration_permission() const override {
    // This is only really needed for web security things we don't need to worry about.
    // So, always allow.
    return ENUMERATION_ALLOWED;
  }

  // As NetworkManager
  bool GetDefaultLocalAddress(int family, rtc::IPAddress* ipaddr) const override {
    // TODO: Add support for using a default route instead of choosing a particular network.
    // (such as when we can't enumerate networks or IPs)
    return false;
  }

  // As PacketSocketFactory
  rtc::AsyncPacketSocket* CreateUdpSocket(const rtc::SocketAddress& local_address_without_port,
                                          uint16_t min_port,
                                          uint16_t max_port) override {
    RTC_DCHECK(network_thread_->IsCurrent());
    RTC_LOG(LS_INFO) << "InjectableNetworkImpl::CreateUdpSocket() ip: " << local_address_without_port.ip();
    const rtc::IPAddress& local_ip = local_address_without_port.ipaddr();
    // The min_port and max_port are ultimiately controlled by the PortAllocator,
    // which we create, so we can ignore those.
    // And the local_address is supposed to have a port of 0.
    uint16_t local_port = next_udp_port_++;
    rtc::SocketAddress local_address(local_ip, local_port);
    auto udp_socket = new InjectableUdpSocket(this, local_address);
    udp_socket_by_local_address_.insert({local_address, udp_socket});
    // This really should return a std::unique_ptr because callers all take ownership.
    return udp_socket;
  }

  // As PacketSocketFactory
  rtc::AsyncPacketSocket* CreateServerTcpSocket(const rtc::SocketAddress& local_address,
                                           uint16_t min_port,
                                           uint16_t max_port,
                                           int opts) override {
    // We never plan to support TCP ICE (other than through TURN),
    // So we'll never implement this.
    return nullptr;
  }

  // As PacketSocketFactory
  rtc::AsyncPacketSocket* CreateClientTcpSocket(
      const rtc::SocketAddress& local_address,
      const rtc::SocketAddress& remote_address,
      const rtc::ProxyInfo& proxy_info,
      const std::string& user_agent,
      const rtc::PacketSocketTcpOptions& tcp_options) override {
    // TODO: Support TCP for TURN
    return nullptr;
  }

  // As PacketSocketFactory
  rtc::AsyncResolverInterface* CreateAsyncResolver() override {
    // TODO: Add support for DNS-based STUN/TURN servers.
    // For now, just use IP addresses
    return nullptr;
  }

 private:
  rtc::Thread* network_thread_;
  std::map<std::string, std::unique_ptr<rtc::Network>> interface_by_name_;
  std::map<rtc::SocketAddress, InjectableUdpSocket*> udp_socket_by_local_address_;
  // The ICE stack does not like ports below 1024.
  // Give it a nice even number to count up from.
  uint16_t next_udp_port_ = 2001;
  InjectableNetworkSender sender_ = {};
};

std::unique_ptr<InjectableNetwork> CreateInjectableNetwork(rtc::Thread* network_thread) {
  return std::make_unique<InjectableNetworkImpl>(network_thread);
}

RUSTEXPORT void Rust_InjectableNetwork_SetSender(
    InjectableNetwork* network,
    const InjectableNetworkSender* sender) {
  network->SetSender(sender);
}

RUSTEXPORT void Rust_InjectableNetwork_AddInterface(
    InjectableNetwork* network,
    const char* name,
    rtc::AdapterType type,
    Ip ip, 
    int preference) {
  network->AddInterface(name, type, ip, preference);
}

RUSTEXPORT void Rust_InjectableNetwork_RemoveInterface(
    InjectableNetwork* network,
    const char* name) {
  network->RemoveInterface(name);
}

RUSTEXPORT void Rust_InjectableNetwork_ReceiveUdp(
    InjectableNetwork* network,
    IpPort local,
    IpPort remote,
    const uint8_t* data,
    size_t size) {
  network->ReceiveUdp(local, remote, data, size);
}

}  // namespace rffi

}  // namespace webrtc



