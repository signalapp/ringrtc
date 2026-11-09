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

    fs::create_dir_all(WEBRTC_DEST_DIR.as_str())?;
    // changes to versions/checksums files also trigger this
    if webrtc::should_use_prebuilt() {
        webrtc::download_prebuilt()?;
        return Ok(());
    }

    println!("cargo:rerun-if-changed=libwebrtc/{}", *GCLIENT_FILE);

    let sh = webrtc::prepare_workspace_shell()?;
    webrtc::build_webrtc_from_source(sh)?;

    // skip file watching to speed up build times. We skip every folder thatbit, we skip a lot
    // has a .git entry: third_party, build, buildtools, ios, testing, tools
    const WATCHED_THIRD_PARTY: &[&str] = &["opus"];
    let src_dir = format!("{}/src", *LIBWEBRTC_DIR);
    for entry in fs::read_dir(&src_dir)? {
        let entry = entry?;
        let path = entry.path();
        let skipped = matches!(
            entry.file_name().to_str(),
            Some("out" | ".git" | "third_party")
        );
        if !skipped && !path.join(".git").exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    for dep in WATCHED_THIRD_PARTY {
        println!("cargo:rerun-if-changed={src_dir}/third_party/{dep}");
    }
    Ok(())
}
