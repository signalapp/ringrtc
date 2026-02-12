//
// Copyright 2025 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::fs;

use webrtc_sys_build::webrtc::{self, config::*};

fn main() -> anyhow::Result<()> {
    println!(
        "webrtc-sys build.rs: profile: {}, test: {}, os: {}, arch: {}, sim: {} webrtc_output_dir: {}, workspace_dir: {}",
        *PROFILE, IS_TEST, *TARGET_OS, *TARGET_ARCH, TARGET_SIMULATOR, *OUTPUT_DIR, *WORKSPACE_DIR
    );

    webrtc::verify_webrtc_target()?;
    println!("cargo:rerun-if-env-changed=OUTPUT_DIR");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=libwebrtc/*");
    println!("cargo:rerun-if-changed=src/*");

    fs::create_dir_all(WEBRTC_DEST_DIR.as_str())?;
    if webrtc::should_use_prebuilt() {
        webrtc::download_prebuilt()?;
        return Ok(());
    }

    let sh = webrtc::prepare_workspace_shell()?;
    webrtc::build_webrtc_from_source(sh)
}
