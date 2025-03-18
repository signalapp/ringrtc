//
// Copyright 2020-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use core::panic;
use std::{
    env::{self, VarError},
    fs,
    process::Command,
};

// corresponds to PROJECT_DIR in bin/env.sh
fn project_dir() -> String {
    format!("{}/../..", env::current_dir().unwrap().display())
}

// corresponds to CONFIG_DIR in bin/env.sh
fn config_dir() -> String {
    format!("{}/config", project_dir())
}

// corresponds to default OUTPUT_DIR in bin/env.sh
fn default_output_dir() -> String {
    format!("{}/out", project_dir())
}

fn main() {
    let target = env::var("TARGET").unwrap();
    let profile = env::var("PROFILE").unwrap();
    let out_dir = env::var("OUTPUT_DIR")
        .or_else(|err| match err {
            VarError::NotPresent => {
                let out_dir = default_output_dir();
                eprintln!("Defaulting WebRTC output directory, OUTPUT_DIR={}", out_dir);
                Ok(out_dir)
            }
            err => Err(err),
        })
        .expect("Invalid OUTPUT_DIR environment variable");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let target_arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    // TARGET and PROFILE are set by Cargo, but OUTPUT_DIR is external.
    println!("cargo:rerun-if-env-changed=OUTPUT_DIR");

    let debug = profile.contains("debug");
    let build_type = if debug { "debug" } else { "release" };

    eprintln!(
        "build.rs: target: {}, profile: {}, os: {}, arch: {}, outdir: {}",
        target, profile, target_os, target_arch, out_dir
    );

    // We only depend on environment variables, not any files.
    // Explicitly state that by depending on build.rs itself, as recommended.
    println!("cargo:rerun-if-changed=build.rs");

    if cfg!(feature = "prebuilt_webrtc") && cfg!(feature = "prebuilt_webrtc_sim") {
        panic!("Cannot enable both prebuilt_webrtc and prebuilt_webrtc_sim features");
    }

    if cfg!(feature = "native") {
        let webrtc_dir =
            if cfg!(feature = "prebuilt_webrtc") || cfg!(feature = "prebuilt_webrtc_sim") {
                if let Err(e) = fs::create_dir_all(&out_dir) {
                    panic!("Failed to create webrtc out directory: {:?}", e);
                }
                fetch_webrtc_artifact(
                    &target_os,
                    &target_arch,
                    &out_dir,
                    cfg!(feature = "prebuilt_webrtc_sim"),
                )
                .unwrap();
                // Ignore build type since we only have release prebuilts
                format!("{}/release/obj/", out_dir)
            } else {
                format!("{}/{}/obj", out_dir, build_type)
            };
        println!("cargo:rerun-if-changed={}", webrtc_dir);
        println!("cargo:rerun-if-changed={}", config_dir());
        println!("cargo:rustc-link-search=native={}", webrtc_dir);
        println!("cargo:rustc-link-lib=webrtc");

        if cfg!(target_os = "macos") {
            println!("cargo:rustc-link-lib=dylib=c++");
            println!("cargo:rustc-link-lib=framework=Foundation");
            println!("cargo:rustc-link-lib=framework=CoreAudio");
            println!("cargo:rustc-link-lib=framework=AudioToolbox");
            println!("cargo:rustc-link-lib=framework=CoreGraphics");

            if let Some(path) = macos_link_search_path() {
                println!("cargo:rustc-link-lib=clang_rt.osx");
                println!("cargo:rustc-link-search={}", path);
            } else {
                panic!("No valid macos search path found!")
            }
        } else if cfg!(target_os = "windows") {
            println!("cargo:rustc-link-lib=winmm");
            println!("cargo:rustc-link-lib=dmoguids");
            println!("cargo:rustc-link-lib=msdmo");
            println!("cargo:rustc-link-lib=wmcodecdspuuid");
            println!("cargo:rustc-link-lib=secur32");
            println!("cargo:rustc-link-lib=iphlpapi");
            // Include the appropriate static C/C++ Standard Libraries.
            if debug {
                println!("cargo:rustc-link-lib=libcmtd");
                println!("cargo:rustc-link-lib=libcpmtd");
            } else {
                println!("cargo:rustc-link-lib=libcmt");
                println!("cargo:rustc-link-lib=libcpmt");
            }
        } else {
            println!("cargo:rustc-link-lib=stdc++");
        }
    } else if target_os == "android" {
        // Rely on the compile invocation to provide the right search path.
        println!("cargo:rustc-link-lib=ringrtc_rffi");
    }
}

// Based on https://github.com/alexcrichton/curl-rust/blob/master/curl-sys/build.rs
fn macos_link_search_path() -> Option<String> {
    let output = Command::new("clang")
        .arg("--print-search-dirs")
        .output()
        .ok()?;
    if !output.status.success() {
        // Failed to run 'clang --print-search-dirs'.
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("libraries: =") {
            let path = line.split('=').nth(1)?;
            return Some(format!("{}/lib/darwin", path));
        }
    }

    // Failed to determine link search path.
    None
}

fn fetch_webrtc_artifact(
    target_os: &str,
    target_arch: &str,
    artifact_out_dir: &str,
    target_sim: bool,
) -> Result<(), String> {
    let fetch_script = format!("{}/bin/fetch-artifact", project_dir());
    let platform = format!("{}-{}", target_os, target_arch);
    eprintln!(
        "Fetching prebuilt webrtc for {} and outputting to {}...",
        platform, artifact_out_dir
    );

    let mut command = Command::new("bash");
    command
        .current_dir(project_dir())
        .env("OUTPUT_DIR", artifact_out_dir)
        .arg(fetch_script)
        .arg("--platform")
        .arg(platform);
    if target_sim {
        command.arg("--for-simulator");
    }
    let output = command
        .output()
        .expect("bin/fetch-artifact failed to complete");

    if !output.status.success() {
        // debug format shows captured stdout/stderr
        return Err(format!("Failed to fetch artifact: {:?}", output));
    }
    Ok(())
}
