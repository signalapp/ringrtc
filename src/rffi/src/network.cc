/*
 * Copyright 2019-2021 Signal Messenger, LLC
 * SPDX-License-Identifier: AGPL-3.0-only
 */

#include "rffi/api/network.h"

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

}  // namespace rffi

}  // namespace webrtc
