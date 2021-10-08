//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::ffi::CString;
use std::fmt::Debug;
use std::net::{IpAddr, SocketAddr};
use std::os::raw::c_char;

use crate::core::util::CppObject;
use crate::webrtc::network::{NetworkInterfaceType, RffiIp, RffiIpPort};
use crate::webrtc::peer_connection_factory::PeerConnectionFactory;

#[derive(Debug)]
pub struct Packet {
    pub source: SocketAddr,
    pub dest: SocketAddr,
    pub data: Vec<u8>,
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
    pcf: PeerConnectionFactory,
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
            object: Box::into_raw(Box::new(sender)),
            send_udp: Rust_InjectableNetworkSender_SendUdp,
            release: Rust_InjectableNetworkSender_Release,
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
    object: *const Box<dyn PacketSender>,
    send_udp: extern "C" fn(
        object: *mut Box<dyn PacketSender>,
        source: RffiIpPort,
        dest: RffiIpPort,
        data: *const u8,
        size: usize,
    ),
    release: extern "C" fn(object: *mut Box<dyn PacketSender>),
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
