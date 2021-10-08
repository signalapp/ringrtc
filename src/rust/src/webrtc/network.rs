//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::fmt;
use std::fmt::Debug;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

/// Rust version of WebRTC AdapterType
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub enum NetworkInterfaceType {
    Unknown = 0,
    Ethernet = 1,
    Wifi = 1 << 1,
    Cellular = 1 << 2, // 2G? 3G? 4G? Unknown.
    Vpn = 1 << 3,
    Loopback = 1 << 4,
    Any = 1 << 5, // When using the "any address".  Not the same as "unknown"
}

/// Rust version of Web RFFI Ip,
/// which is like WebRTC IPAddress.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RffiIp {
    // If v6 == false, only use the first 4 bytes.
    v6: bool,
    address: [u8; 16],
}

impl RffiIp {
    pub fn ip(&self) -> IpAddr {
        if self.v6 {
            IpAddr::V6(Ipv6Addr::new(
                u16::from_be_bytes([self.address[0], self.address[1]]),
                u16::from_be_bytes([self.address[2], self.address[3]]),
                u16::from_be_bytes([self.address[4], self.address[5]]),
                u16::from_be_bytes([self.address[6], self.address[7]]),
                u16::from_be_bytes([self.address[8], self.address[9]]),
                u16::from_be_bytes([self.address[10], self.address[11]]),
                u16::from_be_bytes([self.address[12], self.address[13]]),
                u16::from_be_bytes([self.address[14], self.address[15]]),
            ))
        } else {
            IpAddr::V4(Ipv4Addr::new(
                self.address[0],
                self.address[1],
                self.address[2],
                self.address[3],
            ))
        }
    }
}

impl From<IpAddr> for RffiIp {
    fn from(ip: IpAddr) -> RffiIp {
        match ip {
            IpAddr::V4(ipv4) => {
                let octets = ipv4.octets();
                RffiIp {
                    v6: false,
                    address: [
                        octets[0], octets[1], octets[2], octets[3], 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                        0, 0,
                    ],
                }
            }
            IpAddr::V6(ipv6) => {
                let segments = ipv6.segments();
                let [o0, o1] = segments[0].to_be_bytes();
                let [o2, o3] = segments[1].to_be_bytes();
                let [o4, o5] = segments[2].to_be_bytes();
                let [o6, o7] = segments[3].to_be_bytes();
                let [o8, o9] = segments[4].to_be_bytes();
                let [o10, o11] = segments[5].to_be_bytes();
                let [o12, o13] = segments[6].to_be_bytes();
                let [o14, o15] = segments[7].to_be_bytes();
                RffiIp {
                    v6: true,
                    address: [
                        o0, o1, o2, o3, o4, o5, o6, o7, o8, o9, o10, o11, o12, o13, o14, o15,
                    ],
                }
            }
        }
    }
}

impl Debug for RffiIp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.ip().fmt(f)
    }
}

/// Rust version of WebRTC RFFI IpPort,
/// which is like WebRTC SocketAddress
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RffiIpPort {
    ip: RffiIp,
    port: u16,
}

impl RffiIpPort {
    pub fn socket_addr(&self) -> SocketAddr {
        SocketAddr::new(self.ip.ip(), self.port)
    }
}

impl From<SocketAddr> for RffiIpPort {
    fn from(addr: SocketAddr) -> RffiIpPort {
        RffiIpPort {
            ip: addr.ip().into(),
            port: addr.port(),
        }
    }
}

impl From<&RffiIpPort> for SocketAddr {
    fn from(rffi: &RffiIpPort) -> SocketAddr {
        SocketAddr::new(rffi.ip.ip(), rffi.port)
    }
}

impl Debug for RffiIpPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.socket_addr().fmt(f)
    }
}
