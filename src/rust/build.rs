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
    let debug = profile.contains("debug");
    let build_type = if debug { "debug" } else { "release" };

    eprintln!("build.rs: target: {}, profile: {}", target, profile);

    if cfg!(feature = "native") {
        if out_dir.is_err() {
            panic!("No output directory (OUTPUT_DIR) defined!");
        }

        println!("cargo:rustc-link-lib=webrtc");
        println!(
            "{}",
            format!(
                "cargo:rustc-link-search=native={}/{}/obj/",
                out_dir.unwrap(),
                build_type,
            )
        );

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
    } else if target.ends_with("-ios") {
        if out_dir.is_err() {
            panic!("No output directory (OUTPUT_DIR) defined!");
        }

        println!("cargo:rustc-link-lib=framework=WebRTC");
        println!(
            "{}",
            format!("cargo:rustc-link-search=framework={}", out_dir.unwrap(),)
        );
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
