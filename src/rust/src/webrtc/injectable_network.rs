//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use std::ffi::CString;
use std::fmt;
use std::fmt::Debug;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::os::raw::c_char;

use crate::core::util::CppObject;
use crate::webrtc::peer_connection_factory::PeerConnectionFactory;

#[derive(Debug)]
pub struct Packet {
    pub source: SocketAddr,
    pub dest:   SocketAddr,
    pub data:   Vec<u8>,
}

pub trait PacketSender {
    fn send_udp(&self, packet: Packet);
}

impl<F: Fn(Packet)> PacketSender for F {
    fn send_udp(&self, packet: Packet) {
        self(packet)
    }
}

#[derive(Clone)]
pub struct InjectableNetwork {
    rffi: *const RffiInjectableNetwork,
    // We keep this around not to make sure the PCF doesn't get
    // destroyed before we do (the PCF is atomically ref-counted in C++).
    pcf:  PeerConnectionFactory,
}

impl InjectableNetwork {
    pub fn new(rffi: *const RffiInjectableNetwork, pcf: &PeerConnectionFactory) -> Self {
        Self {
            rffi,
            pcf: pcf.clone(),
        }
    }

    pub fn set_sender(&self, sender: Box<dyn PacketSender>) {
        let sender_ptr = &RffiInjectableNetworkSender {
            // C++ is expected to call release() on this.
            // Yes, it's a box in a box.  That's because we need
            // a non-dyn pointer to then point to the dyn pointer
            // because C++ wants normal pointers and dyn pointers
            // are fat (double size) pointers.
            object:   Box::into_raw(Box::new(sender)),
            send_udp: Rust_InjectableNetworkSender_SendUdp,
            release:  Rust_InjectableNetworkSender_Release,
        } as *const RffiInjectableNetworkSender as CppObject;
        unsafe {
            Rust_InjectableNetwork_SetSender(self.rffi, sender_ptr);
        }
    }

    pub fn add_interface(
        &self,
        name: &str,
        typ: NetworkInterfaceType,
        ip: IpAddr,
        preference: u16,
    ) {
        let cname = CString::new(name).expect("Bad network interface name.");
        unsafe {
            Rust_InjectableNetwork_AddInterface(
                self.rffi,
                cname.as_ptr(),
                typ,
                ip.into(),
                preference,
            );
        }
    }

    pub fn remove_interface(&self, name: &str) {
        let cname = CString::new(name).expect("Bad network interface name.");
        unsafe {
            Rust_InjectableNetwork_RemoveInterface(self.rffi, cname.as_ptr());
        }
    }

    pub fn receive_udp(&self, packet: Packet) {
        unsafe {
            // Rust_receiveUdp is expected to copy it because it's going to get dropped.
            Rust_InjectableNetwork_ReceiveUdp(
                self.rffi,
                packet.source.into(),
                packet.dest.into(),
                packet.data.as_ptr(),
                packet.data.len(),
            );
        }
    }
}

/// Rust version of WebRTC RFFI InjectableNetwork
#[repr(C)]
pub struct RffiInjectableNetwork {
    _private: [u8; 0],
}

extern "C" {
    pub fn Rust_InjectableNetwork_SetSender(
        network: *const RffiInjectableNetwork,
        sender: CppObject,
    );
    pub fn Rust_InjectableNetwork_AddInterface(
        network: *const RffiInjectableNetwork,
        name: *const c_char,
        typ: NetworkInterfaceType,
        ip: RffiIp,
        preference: u16,
    );
    pub fn Rust_InjectableNetwork_RemoveInterface(
        network: *const RffiInjectableNetwork,
        name: *const c_char,
    );
    pub fn Rust_InjectableNetwork_ReceiveUdp(
        network: *const RffiInjectableNetwork,
        source: RffiIpPort,
        dest: RffiIpPort,
        data: *const u8,
        size: usize,
    );
}

#[repr(C)]
struct RffiInjectableNetworkSender {
    object:   *const Box<dyn PacketSender>,
    send_udp: extern "C" fn(
        object: *mut Box<dyn PacketSender>,
        source: RffiIpPort,
        dest: RffiIpPort,
        data: *const u8,
        size: usize,
    ),
    release:  extern "C" fn(object: *mut Box<dyn PacketSender>),
}

#[allow(non_snake_case)]
extern "C" fn Rust_InjectableNetworkSender_SendUdp(
    sender: *mut Box<dyn PacketSender>,
    source: RffiIpPort,
    dest: RffiIpPort,
    data: *const u8,
    size: usize,
) {
    debug!("Send UDP {:?} => {:?} of size {}", source, dest, size);

    let sender = unsafe { &*sender };
    // Copy the data because it won't be valid any more.
    let data = unsafe { std::slice::from_raw_parts(data, size) }.to_vec();
    let packet = Packet {
        source: source.socket_addr(),
        dest: dest.socket_addr(),
        data,
    };
    sender.send_udp(packet);
}

#[allow(non_snake_case)]
extern "C" fn Rust_InjectableNetworkSender_Release(sender: *mut Box<dyn PacketSender>) {
    debug!("Rust_InjectableNetworkSender_Release({:?})", sender);

    let sender: Box<Box<dyn PacketSender>> = unsafe { Box::from_raw(sender) };
    drop(sender);
}

// They are safe to Send/Sync because everything in the C++ hops to
// the network thread.
unsafe impl Sync for InjectableNetwork {}
unsafe impl Send for InjectableNetwork {}

/// Rust version of WebRTC AdapterType
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub enum NetworkInterfaceType {
    Unknown  = 0,
    Ethernet = 1,
    Wifi     = 1 << 1,
    Cellular = 1 << 2, // 2G? 3G? 4G? Unknown.
    Vpn      = 1 << 3,
    Loopback = 1 << 4,
    Any      = 1 << 5, // When using the "any address".  Not the same as "unknown"
}

/// Rust version of WebRTC RFFI Ip,
/// which is like WebRTC IPAddress.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RffiIp {
    // If v6 == false, only use the first 4 bytes.
    v6:      bool,
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
                    v6:      false,
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
                    v6:      true,
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
    ip:   RffiIp,
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
            ip:   addr.ip().into(),
            port: addr.port(),
        }
    }
}

impl Debug for RffiIpPort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.socket_addr().fmt(f)
    }
}
