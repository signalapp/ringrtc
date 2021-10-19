//
// Copyright 2019-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::ffi::CString;
use std::fmt::Debug;
use std::net::{IpAddr, SocketAddr};
use std::os::raw::c_char;

use crate::webrtc;
use crate::webrtc::network::{NetworkInterfaceType, RffiIp, RffiIpPort};
use crate::webrtc::peer_connection_factory::RffiPeerConnectionFactoryOwner;

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
    rffi: webrtc::ptr::Borrowed<RffiInjectableNetwork>,
    // We keep this around as an easy way to make sure the PeerConnectionFactory
    // outlives the InjectableNetwork.
    _owner: webrtc::Arc<RffiPeerConnectionFactoryOwner>,
}

impl InjectableNetwork {
    pub fn new(
        rffi: webrtc::ptr::Borrowed<RffiInjectableNetwork>,
        owner: webrtc::Arc<RffiPeerConnectionFactoryOwner>,
    ) -> Self {
        Self {
            rffi,
            _owner: owner,
        }
    }

    pub fn set_sender(&self, sender: Box<dyn PacketSender>) {
        let sender_ptr = &RffiInjectableNetworkSender {
            // Yes, it's a box in a box.  That's because we need
            // a non-dyn pointer to then point to the dyn pointer
            // because C++ wants normal pointers and dyn pointers
            // are fat (double size) pointers.
            object: unsafe { webrtc::ptr::Owned::from_ptr(Box::into_raw(Box::new(sender))) },
            send_udp: Rust_InjectableNetworkSender_SendUdp,
            delete: Rust_InjectableNetworkSender_Delete,
        };
        unsafe {
            Rust_InjectableNetwork_SetSender(
                self.rffi,
                webrtc::ptr::Borrowed::from_ptr(sender_ptr).to_void(),
            );
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
                webrtc::ptr::Borrowed::from_ptr(cname.as_ptr()),
                typ,
                ip.into(),
                preference,
            );
        }
    }

    pub fn remove_interface(&self, name: &str) {
        let cname = CString::new(name).expect("Bad network interface name.");
        unsafe {
            Rust_InjectableNetwork_RemoveInterface(
                self.rffi,
                webrtc::ptr::Borrowed::from_ptr(cname.as_ptr()),
            );
        }
    }

    pub fn receive_udp(&self, packet: Packet) {
        unsafe {
            // Rust_receiveUdp is expected to copy it because it's going to get dropped.
            Rust_InjectableNetwork_ReceiveUdp(
                self.rffi,
                packet.source.into(),
                packet.dest.into(),
                webrtc::ptr::Borrowed::from_ptr(packet.data.as_ptr()),
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
        network: webrtc::ptr::Borrowed<RffiInjectableNetwork>,
        sender: webrtc::ptr::Borrowed<std::ffi::c_void>,
    );
    pub fn Rust_InjectableNetwork_AddInterface(
        network: webrtc::ptr::Borrowed<RffiInjectableNetwork>,
        name: webrtc::ptr::Borrowed<c_char>,
        typ: NetworkInterfaceType,
        ip: RffiIp,
        preference: u16,
    );
    pub fn Rust_InjectableNetwork_RemoveInterface(
        network: webrtc::ptr::Borrowed<RffiInjectableNetwork>,
        name: webrtc::ptr::Borrowed<c_char>,
    );
    pub fn Rust_InjectableNetwork_ReceiveUdp(
        network: webrtc::ptr::Borrowed<RffiInjectableNetwork>,
        source: RffiIpPort,
        dest: RffiIpPort,
        data: webrtc::ptr::Borrowed<u8>,
        size: usize,
    );
}

#[repr(C)]
struct RffiInjectableNetworkSender {
    object: webrtc::ptr::Owned<Box<dyn PacketSender>>,
    send_udp: extern "C" fn(
        object: webrtc::ptr::Borrowed<Box<dyn PacketSender>>,
        source: RffiIpPort,
        dest: RffiIpPort,
        data: webrtc::ptr::Borrowed<u8>,
        size: usize,
    ),
    delete: extern "C" fn(object: webrtc::ptr::Owned<Box<dyn PacketSender>>),
}

#[allow(non_snake_case)]
extern "C" fn Rust_InjectableNetworkSender_SendUdp(
    sender: webrtc::ptr::Borrowed<Box<dyn PacketSender>>,
    source: RffiIpPort,
    dest: RffiIpPort,
    data: webrtc::ptr::Borrowed<u8>,
    size: usize,
) {
    debug!("Send UDP {:?} => {:?} of size {}", source, dest, size);

    // Safe because the sender should still be alive (it was just passed to us)
    if let Some(sender) = unsafe { sender.as_ref() } {
        // Copy the data because it won't be valid any more.
        let data = unsafe { std::slice::from_raw_parts(data.as_ptr(), size) }.to_vec();
        let packet = Packet {
            source: source.socket_addr(),
            dest: dest.socket_addr(),
            data,
        };
        sender.send_udp(packet);
    } else {
        error!("Rust_InjectableNetworkSender_SendUdp called with null sender");
    }
}

#[allow(non_snake_case)]
extern "C" fn Rust_InjectableNetworkSender_Delete(
    sender: webrtc::ptr::Owned<Box<dyn PacketSender>>,
) {
    debug!("Rust_InjectableNetworkSender_Release({:?})", sender);

    if let Some(sender) = sender.as_mut() {
        let sender = unsafe { Box::from_raw(sender) };
        drop(sender);
    } else {
        error!("Rust_InjectableNetworkSender_Delete called with null sender");
    }
}

// They are safe to Send/Sync because everything in the C++ hops to
// the network thread.
unsafe impl Sync for InjectableNetwork {}
unsafe impl Send for InjectableNetwork {}
