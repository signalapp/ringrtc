//
// Copyright 2026 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{
    thread,
    time::{Duration, Instant},
};

use anyhow::anyhow;
use log::*;

use crate::{
    common::Result,
    webrtc::peer_connection_factory::{AudioDeviceList, PeerConnectionFactory},
};

// We may need to try a few times to get audio devices; they're not necessarily
// populated instantly. Ideally we could use the callbacks to notify us
// when these are populated, but that's slightly tricky, because we might
// fetch the devices before callback registration, and they're device
// **change** callbacks.
// Only try for up to AUDIO_DEVICE_TIMEOUT before giving up.
const AUDIO_DEVICE_TIMEOUT: Duration = Duration::from_secs(5);

fn wait_for_playout_devices(
    peer_connection_factory: &mut PeerConnectionFactory,
) -> Result<AudioDeviceList> {
    let start = Instant::now();
    while start.elapsed() < AUDIO_DEVICE_TIMEOUT {
        if let Ok(device_list) = peer_connection_factory.get_audio_playout_devices()
            && !device_list.list.is_empty()
        {
            return Ok(device_list);
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(anyhow!("timed out waiting for playout devices"))
}

fn wait_for_recording_devices(
    peer_connection_factory: &mut PeerConnectionFactory,
) -> Result<AudioDeviceList> {
    let start = Instant::now();
    while start.elapsed() < AUDIO_DEVICE_TIMEOUT {
        if let Ok(device_list) = peer_connection_factory.get_audio_recording_devices()
            && !device_list.list.is_empty()
        {
            return Ok(device_list);
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(anyhow!("timed out waiting for playout devices"))
}

pub fn set_default_audio_devices(
    peer_connection_factory: &mut PeerConnectionFactory,
) -> Result<()> {
    let playout_devices = wait_for_playout_devices(peer_connection_factory)?;
    info!("Audio playout devices: {:?}", playout_devices);
    let recording_devices = wait_for_recording_devices(peer_connection_factory)?;
    info!("Audio recording devices: {:?}", recording_devices);

    let default_playout = playout_devices
        .default
        .default_for_voip()
        .ok_or(anyhow!("No default playout device"))?;
    let default_recording = recording_devices
        .default
        .default_for_voip()
        .ok_or(anyhow!("No default recording device"))?;

    peer_connection_factory.set_audio_playout_device_by_id(&default_playout.unique_id, 0)?;
    peer_connection_factory.set_audio_recording_device_by_id(&default_recording.unique_id, 0)?;
    Ok(())
}
