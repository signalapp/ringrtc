//
// Copyright (C) 2020 Signal Messenger, LLC.
// All rights reserved.
//
// SPDX-License-Identifier: GPL-3.0-only
//

use std::env;
use std::process::Command;

fn main() {
    let target = env::var("TARGET").unwrap();
    let profile = env::var("PROFILE").unwrap();
    let debug = profile.contains("debug");

    eprintln!("build.rs: target: {}, profile: {}", target, profile);

    if cfg!(feature = "native") {
        println!("cargo:rustc-link-lib=webrtc");
        if debug {
            println!("cargo:rustc-link-search=native=../../src/webrtc/src/out/Debug/obj/",);
        } else {
            println!("cargo:rustc-link-search=native=../../src/webrtc/src/out/Release/obj/",);
        }

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

    if cfg!(feature = "electron") {
        neon_build::setup();
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
