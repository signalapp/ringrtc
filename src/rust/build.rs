//
// Copyright 2020-2021 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::env;
use std::process::Command;

fn main() {
    let target = env::var("TARGET").unwrap();
    let profile = env::var("PROFILE").unwrap();
    let out_dir = env::var("OUTPUT_DIR");
    // TARGET and PROFILE are set by Cargo, but OUTPUT_DIR is external.
    println!("cargo:rerun-if-env-changed=OUTPUT_DIR");

    let debug = profile.contains("debug");
    let build_type = if debug { "debug" } else { "release" };

    eprintln!("build.rs: target: {}, profile: {}", target, profile);

    // We only depend on environment variables, not any files.
    // Explicitly state that by depending on build.rs itself, as recommended.
    println!("cargo:rerun-if-changed=build.rs");

    if cfg!(feature = "native") {
        if let Ok(out_dir) = out_dir {
            println!(
                "cargo:rustc-link-search=native={}/{}/obj/",
                out_dir, build_type,
            );
        } else {
            println!("cargo:warning=No WebRTC output directory (OUTPUT_DIR) defined!");
        }

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

            // Use statically linked 'libcmt[d]' instead of dynamically linked 'msvcrt[d]'.
            if debug {
                println!("cargo:rustc-link-lib=libcmtd");
            } else {
                println!("cargo:rustc-link-lib=libcmt");
            }
        } else {
            println!("cargo:rustc-link-lib=stdc++");
        }
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
