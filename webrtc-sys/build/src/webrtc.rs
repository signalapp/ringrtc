//
// Copyright 2025 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use core::panic;
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{self, BufReader, Read, Seek},
    path::Path,
};

use anyhow::bail;
use config::*;
use sha2::{Digest, Sha256};
use xshell::{Shell, cmd};

pub fn prepare_workspace_shell() -> anyhow::Result<Shell> {
    let mut sh = Shell::new()?;
    if *TARGET_PLATFORM == "windows" {
        sh.set_var("DEPOT_TOOLS_WIN_TOOLCHAIN", "0");
    }

    ensure_chromium_depot_tools(&mut sh)?;
    sync_webrtc_source(&mut sh)?;
    Ok(sh)
}

pub fn ensure_chromium_depot_tools(sh: &mut Shell) -> anyhow::Result<()> {
    if which::which("gclient").is_ok() {
        return Ok(());
    }

    let depot_tools_path = format!("{}/depot_tools", *OUTPUT_DIR);
    if !Path::new(&depot_tools_path).exists() {
        cmd!(sh, "git clone --depth 1 https://chromium.googlesource.com/chromium/tools/depot_tools.git {depot_tools_path}").run()?;
    }
    sh.set_var(
        "PATH",
        format!("{depot_tools_path}:{}", sh.var("PATH").unwrap()),
    );

    Ok(())
}

pub fn sync_webrtc_source(sh: &mut Shell) -> anyhow::Result<()> {
    println!(
        "Syncing WebRTC source version '{}' to directory {}",
        *WEBRTC_VERSION, *LIBWEBRTC_DIR
    );
    let gclient_file = *GCLIENT_FILE;
    let revision = WEBRTC_VERSION.as_str();
    sh.change_dir(LIBWEBRTC_DIR.as_str());
    let env_vars = HashMap::from([
        ("WEBRTC_DIR", LIBWEBRTC_DIR.clone()),
        ("WEBRTC_VERSION", WEBRTC_VERSION.clone()),
        (
            "WEBRTC_REVISION",
            format!("branch-heads/{}", *WEBRTC_VERSION),
        ),
    ]);
    cmd!(
        sh,
        "gclient sync --no-history --jobs 32 --with_tags --revision=src@{revision} --gclientfile={gclient_file}"
    )
    .envs(env_vars)
    .run()?;
    Ok(())
}

pub fn build_webrtc_from_source(sh: Shell) -> anyhow::Result<()> {
    match *TARGET_PLATFORM {
        "ios" => build_webrtc_for_ios(sh),
        "android" => build_webrtc_for_android(sh),
        "desktop" => build_webrtc_for_desktop(sh),
        _ => bail!("Unsupported target platform {}", *TARGET_PLATFORM),
    }
}

pub fn common_webrtc_flags() -> Vec<String> {
    let target_os = match TARGET_OS.as_str() {
        "macos" => "target_os=\"mac\"".to_string(),
        os => format!("target_os=\"{os}\""),
    };
    let symbol_level = if *IS_RELEASE { 1 } else { 2 };
    let include_tests = *IS_DESKTOP && IS_TEST;
    vec![
        target_os,
        format!("is_debug={}", !*IS_RELEASE),
        format!("rtc_include_tests={include_tests}"),
        format!("rtc_enable_protobuf={include_tests}"),
        format!("symbol_level={symbol_level}"),
        "rtc_build_examples=false".to_string(),
        "rtc_build_tools=false".to_string(),
        "rtc_enable_sctp=false".to_string(),
        "rtc_disable_metrics=true".to_string(),
        "rtc_disable_trace_events=true".to_string(),
    ]
}

pub fn build_webrtc_for_desktop(sh: Shell) -> anyhow::Result<()> {
    let working_dir = format!("{}/src", *LIBWEBRTC_DIR);
    let dest = format!("out/{}/{}", *TARGET_KEY, *PROFILE);
    sh.change_dir(&working_dir);

    let gnu_arch = DESKTOP_TARGET_ARCH_TO_GNU_ARCH[TARGET_ARCH.as_str()];
    let mut args = common_webrtc_flags();
    args.extend(vec![
        format!("target_cpu=\"{gnu_arch}\""),
        "rtc_use_x11=false".to_string(),
        "rtc_libvpx_build_vp9=true".to_string(),
        "use_siso=true".to_string(),
    ]);
    if TARGET_OS.as_str() == "linux" && gnu_arch == "arm64" {
        args.push("libyuv_use_sme=false".to_string());
        // Ensure that experimental compact relocation is disabled until upstream projects properly set it.
        // https://issues.webrtc.org/issues/407797634
        // https://chromium-review.googlesource.com/c/chromium/src/+/5938657
        cmd!(
            sh,
            "sed -i '/^[^#].*--allow-experimental-crel/ s/^/#/' src/webrtc/src/build/config/compiler/BUILD.gn"
        ).run()?;
    }
    let args_flag = args.join(" ");
    cmd!(sh, "gn gen -C {dest} --args={args_flag}").run()?;
    cmd!(sh, "third_party/siso/cipd/siso ninja -C {dest} webrtc").run()?;
    if IS_TEST {
        cmd!(sh, "third_party/siso/cipd/siso ninja -C {dest} default").run()?;
        cmd!(sh, "download_from_google_storage --directory --recursive --num_threads=10 --no_auth --quiet --bucket chromium-webrtc-resources resources")
            .run()?;
    }
    cmd!(
        sh,
        "tools_webrtc/libs/generate_licenses.py --target :webrtc {dest} {dest}"
    )
    .run()?;

    let filename = if *TARGET_PLATFORM == "windows" {
        "webrtc.lib"
    } else {
        "libwebrtc.a"
    };
    let from = format!("{working_dir}/{dest}/obj/{filename}");
    let to = format!("{}/{}", *WEBRTC_DEST_DIR, filename);
    let err_msg = format!("could not copy webrtc lib from {from} to {to}");
    fs::copy(from, to).expect(&err_msg);
    let from = format!("{working_dir}/{dest}/LICENSE.md");
    let to = format!("{}/{}/{}/LICENSE.md", *OUTPUT_DIR, *TARGET_KEY, *PROFILE);
    let err_msg = format!("could not copy webrtc license from {from} to {to}");
    fs::copy(from, to).expect(&err_msg);
    Ok(())
}

pub fn build_webrtc_for_ios(sh: Shell) -> anyhow::Result<()> {
    const IPHONEOS_DEPLOYMENT_TARGET: &str = "14.0";
    let profile = PROFILE.as_str();
    let mut args = common_webrtc_flags();
    args.extend(vec![
        format!("enable_dsyms={}", !*IS_RELEASE),
        "rtc_libvpx_build_vp9=false".to_string(),
    ]);
    let working_dir = format!("{}/src", *LIBWEBRTC_DIR);
    let dest = format!("{}/out/ios/{}", working_dir, *PROFILE);
    let library_dest = WEBRTC_DEST_DIR.as_str();
    let xcframework_dest = format!("{library_dest}/WebRTC.xcframework");
    let bin_dir = format!("{}/bin", *WORKSPACE_DIR);
    let webrtc_version = WEBRTC_VERSION.as_str();
    let ringrtc_version = RINGRTC_VERSION.as_str();

    sh.change_dir(working_dir);
    cmd!(
        sh,
        "tools_webrtc/ios/build_ios_libs.py -o {dest} --build_config {profile} --arch simulator:x64 simulator:arm64 device:arm64 --deployment-target {IPHONEOS_DEPLOYMENT_TARGET} --extra-gn-args {args...}"
    ).run()?;
    cmd!(sh, "cp -Rf {dest}/WebRTC.xcframework {library_dest}").run()?;
    let build_env = cmd!(sh, "{bin_dir}/print_build_env.py --webrtc-version={webrtc_version} --ringrtc-version={ringrtc_version}").read().unwrap();
    fs::write(format!("{xcframework_dest}/build_env.txt"), build_env)
        .expect("failed to write build_env.txt");
    // Delete dSYMs out of the built XCFramework.
    //  FIXME: In the future, we probably want to keep these,
    // which is why we aren't changing WebRTC's build script to skip them altogether.
    // We enumerate directories since we can't use parameter expansion with xshell
    cmd!(sh, "rm -r {xcframework_dest}/ios-arm64/dSYMs").run()?;
    cmd!(
        sh,
        "rm -r {xcframework_dest}/ios-arm64_x86_64-simulator/dSYMs"
    )
    .run()?;
    cmd!(
        sh,
        "plutil -remove AvailableLibraries.DebugSymbolsPath {xcframework_dest}/Info.plist"
    )
    .run()?;

    let podspec = format!(
        include_str!("../WebRTCForTesting.podspec.tpl"),
        IPHONEOS_DEPLOYMENT_TARGET, profile
    );
    fs::write(format!("{library_dest}/WebRTCForTesting.podspec"), podspec)
        .expect("failed to write podspec");
    let acknowledgements = cmd!(
        sh,
        "{bin_dir}/convert_webrtc_acknowledgments.py -f plist {xcframework_dest}/LICENSE.md"
    )
    .read()?;
    fs::write(
        format!("{library_dest}/acknowledgments-webrtc-ios.plist"),
        acknowledgements,
    )
    .expect("failed to write acknowledgements");

    Ok(())
}

pub fn build_webrtc_for_android(sh: Shell) -> anyhow::Result<()> {
    let gnu_arch = ANDROID_TARGET_ARCH_TO_GNU_ARCH[TARGET_ARCH.as_str()];
    let profile = PROFILE.as_str();
    let mut args = common_webrtc_flags();
    args.extend(vec![
        format!("target_cpu={gnu_arch}"),
        "rtc_libvpx_build_vp9=false".to_string(),
        "android_static_analysis=\"off\"".to_string(),
        "use_siso=true".to_string(),
    ]);
    let args_flag = args.join(" ");
    let working_dir = format!("{}/src", *LIBWEBRTC_DIR);
    let dest = format!("{working_dir}/out/android-{gnu_arch}/{profile}");
    let library_dest = WEBRTC_DEST_DIR.as_str();
    let path = "lib.java/sdk/android";
    let filename = "libwebrtc.jar";

    sh.change_dir(working_dir);
    cmd!(sh, "gn gen -C {dest} --args={args_flag}").run()?;
    cmd!(sh, "third_party/siso/cipd/siso ninja -C {dest} ringrtc").run()?;
    cmd!(sh, "cp -Rf {dest} {library_dest}").run()?;
    cmd!(sh, "mkdir -p {path}").run()?;
    cmd!(
        sh,
        "cp -Rf {dest}/{path}/{filename} {library_dest}/{path}/{filename}"
    )
    .run()?;

    Ok(())
}

pub fn should_use_prebuilt() -> bool {
    let has_prebuilt_env: Option<&'static str> = option_env!("PREBUILT_WEBRTC");
    let has_prebuilt_feature = cfg!(feature = "prebuilt-webrtc");

    has_prebuilt_env
        .and_then(|s: &str| s.parse::<u8>().ok())
        .and_then(|b| match b {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        })
        .unwrap_or(has_prebuilt_feature)
}

pub fn download_prebuilt() -> anyhow::Result<()> {
    let expected_checksum = ARTIFACT_CHECKSUMS
        .get(TARGET_KEY.as_str())
        .unwrap_or_else(|| panic!("could not find checksum for artifact key: {}", *TARGET_KEY))
        .as_str()
        .expect("artifact checksum must be string");
    let archive_filename = format!(
        "webrtc-{}-{}-{}.tar.bz2",
        *WEBRTC_VERSION, *TARGET_PAIR, *PROFILE
    );
    let archive_local_dir = format!("{}/{}", *OUTPUT_DIR, *TARGET_KEY);
    let archive_local_path = format!("{}/{}", archive_local_dir, archive_filename);
    let download_url = format!("https://build-artifacts.signal.org/libraries/{archive_filename}");
    let temporary_path = format!("{}/unverified.tar.bz2", archive_local_dir);

    println!("Looking for artifact in {archive_local_path} with checksum {expected_checksum}");
    let mut archive_file = OpenOptions::new()
        .read(true)
        .open(&archive_local_path)
        .or_else(|_| download_file(&download_url, &temporary_path))
        .expect("failed to download prebuilt-webrtc artifact");

    let checksum = calculate_sha256(&mut archive_file).expect("Failed to checksum");
    if checksum != expected_checksum {
        bail!(
            "download checksum did not match expected checksum: {checksum} vs {expected_checksum}"
        );
    }
    let _ = fs::rename(temporary_path, archive_local_path);
    archive_file
        .rewind()
        .expect("failed to seek to start of archive file");
    let decoder = bzip2::read::BzDecoder::new(BufReader::new(archive_file));
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(archive_local_dir)
        .expect("failed to decompress webrtc artifact");
    Ok(())
}

pub fn calculate_sha256(file: &mut File) -> io::Result<String> {
    file.rewind()?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 1024]; // Buffer to read file in chunks
    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let hash_bytes = hasher.finalize();
    Ok(hex::encode(hash_bytes))
}

pub fn download_file(url: &str, filename: &str) -> anyhow::Result<File> {
    println!("Downloading from {url} to local file {filename}");
    fs::create_dir_all(Path::new(filename).parent().unwrap())
        .expect("could not make target download folder");
    let mut response = reqwest::blocking::get(url)?.error_for_status()?;
    let mut file = OpenOptions::new()
        .read(true)
        .truncate(true)
        .create(true)
        .write(true)
        .open(filename)?;
    std::io::copy(&mut response, &mut file)?;
    Ok(file)
}

// only certain HOSTS can build certain TARGET_OS such as
// windows targets must be built on windows
// ios and macos must be built on macos
// android or linux can be built on macos or linux
pub fn verify_webrtc_target() -> anyhow::Result<()> {
    if !SUPPORTED_OS_TO_TARGET_ARCH.contains_key(TARGET_OS.as_str()) {
        bail!("Unsupported target OS: {}", *TARGET_OS);
    }
    if !SUPPORTED_OS_TO_TARGET_ARCH[TARGET_OS.as_str()].contains(TARGET_ARCH.as_str()) {
        bail!("Unsupported target ARCH: {}", *TARGET_ARCH);
    }
    if TARGET_SIMULATOR && !*IS_DESKTOP {
        bail!("Unsupported target OS for simulator: {}", *TARGET_OS);
    }

    let target_os = TARGET_OS.as_str();
    let host_os = std::env::consts::OS;
    let host_os_can_build_target = match target_os {
        "windows" => host_os == "windows",
        "ios" | "macos" => ["macos"].contains(&host_os),
        "android" | "linux" => ["macos", "linux"].contains(&host_os),
        _ => bail!("Should have been prevented in earlier supported OS check"),
    };

    if !host_os_can_build_target {
        bail!("Invalid host_os {host_os} for building target_os {target_os}")
    }
    Ok(())
}

pub fn shell<T: AsRef<str>>(cwd: T) -> Shell {
    let sh = Shell::new().unwrap();
    sh.change_dir(cwd.as_ref());
    sh
}

pub mod config {
    use std::{
        collections::{HashMap, HashSet},
        env,
        io::BufReader,
        sync::LazyLock,
    };

    use serde_json::Value;

    // Build Target info
    pub static VERBOSE: LazyLock<bool> = LazyLock::new(|| env::var("CARGO_TERM_VERBOSE").is_ok());
    pub static PROFILE: LazyLock<String> = LazyLock::new(|| env::var("PROFILE").unwrap());
    pub static TARGET_OS: LazyLock<String> =
        LazyLock::new(|| env::var("CARGO_CFG_TARGET_OS").unwrap());
    pub static TARGET_ARCH: LazyLock<String> =
        LazyLock::new(|| env::var("CARGO_CFG_TARGET_ARCH").unwrap());
    pub static TARGET_SIMULATOR: bool = cfg!(feature = "simulator");
    pub static TARGET_PLATFORM: LazyLock<&str> = LazyLock::new(|| match TARGET_OS.as_str() {
        "ios" => "ios",
        "android" => "ios",
        "windows" | "macos" | "linux" => "desktop",
        _ => panic!("Unsupported TARGET OS"),
    });
    // we can simplify if we use cargo target key for webrtc artifacts
    pub static TARGET_PAIR: LazyLock<String> = LazyLock::new(|| match TARGET_OS.as_ref() {
        os @ "ios" | os @ "android" => os.to_string(),
        os => {
            // remove once we migrate releasing prebuilds using cargo build
            let os = if os == "macos" { "mac" } else { os };
            format!(
                "{}-{}",
                os,
                DESKTOP_TARGET_ARCH_TO_GNU_ARCH[TARGET_ARCH.as_str()],
            )
        }
    });
    pub static TARGET_KEY: LazyLock<String> = LazyLock::new(|| {
        if TARGET_SIMULATOR {
            format!("{}-sim", *TARGET_PAIR)
        } else {
            TARGET_PAIR.clone()
        }
    });
    pub static RINGRTC_VERSION: LazyLock<String> = LazyLock::new(|| {
        format!(
            "{}.{}.{}",
            env::var("CARGO_PKG_VERSION_MAJOR").unwrap(),
            env::var("CARGO_PKG_VERSION_MINOR").unwrap(),
            env::var("CARGO_PKG_VERSION_PATCH").unwrap()
        )
    });
    pub static IS_DESKTOP: LazyLock<bool> =
        LazyLock::new(|| ["linux", "macos", "windows"].contains(&TARGET_OS.as_str()));
    pub static IS_RELEASE: LazyLock<bool> = LazyLock::new(|| &*PROFILE == "release");
    pub static IS_TEST: bool = cfg!(test);

    // Directory info
    pub static WORKSPACE_DIR: LazyLock<String> = LazyLock::new(|| {
        env::var("CARGO_WORKSPACE_DIR")
            .map(|s| s.strip_suffix(|_| true).unwrap_or(&s).to_string())
            .unwrap()
    });
    pub static CRATE_DIR: LazyLock<String> =
        LazyLock::new(|| format!("{}/webrtc-sys", *WORKSPACE_DIR));
    pub static LIBWEBRTC_DIR: LazyLock<String> =
        LazyLock::new(|| format!("{}/libwebrtc", *CRATE_DIR));
    pub static OUTPUT_DIR: LazyLock<String> =
        LazyLock::new(|| env::var("OUTPUT_DIR").unwrap_or(format!("{}/out", *CRATE_DIR)));
    pub static WEBRTC_DEST_DIR: LazyLock<String> = LazyLock::new(|| {
        if *IS_DESKTOP {
            format!("{}/{}/{}/obj", *OUTPUT_DIR, *TARGET_KEY, *PROFILE)
        } else {
            format!("{}/{}/{}/obj", *OUTPUT_DIR, *TARGET_OS, *PROFILE)
        }
    });

    pub const SUPPORTED_DESKTOP_TARGET_ARCH: [&str; 4] = ["x86_64", "i686", "aarch64", "arm64"];
    pub const SUPPORTED_ANDROID_TARGET_ARCH: [&str; 5] = ["aarch64", "arm", "arm64", "x86", "x64"];
    pub const SUPPORTED_IOS_TARGET_ARCH: [&str; 3] = ["aarch64", "x86", "arm64"];
    pub static SUPPORTED_OS_TO_TARGET_ARCH: LazyLock<HashMap<&str, HashSet<&str>>> =
        LazyLock::new(|| {
            HashMap::from([
                ("android", HashSet::from(SUPPORTED_ANDROID_TARGET_ARCH)),
                ("ios", HashSet::from(SUPPORTED_IOS_TARGET_ARCH)),
                ("macos", HashSet::from(SUPPORTED_DESKTOP_TARGET_ARCH)),
                ("windows", HashSet::from(SUPPORTED_DESKTOP_TARGET_ARCH)),
                ("linux", HashSet::from(SUPPORTED_DESKTOP_TARGET_ARCH)),
            ])
        });
    pub static DESKTOP_TARGET_ARCH_TO_GNU_ARCH: LazyLock<HashMap<&str, &str>> =
        LazyLock::new(|| {
            HashMap::from([
                ("x86_64", "x64"),
                ("i686", "x86"),
                ("aarch64", "arm64"),
                ("arm64", "arm64"),
            ])
        });
    pub static ANDROID_TARGET_ARCH_TO_GNU_ARCH: LazyLock<HashMap<&str, &str>> =
        LazyLock::new(|| {
            HashMap::from([("x86_64", "x64"), ("aarch64", "arm64"), ("armv7", "arm")])
        });

    // Values pulled from config files
    pub static ARTIFACT_CHECKSUMS: LazyLock<Value> = LazyLock::new(|| {
        serde_json::from_str(include_str!(
            "../../../config/webrtc_artifact_checksums.json"
        ))
        .expect("artifact checksums should be valid json file")
    });
    pub static VERSION_PROPERTIES: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
        java_properties::read(BufReader::new(
            include_str!("../../../config/version.properties").as_bytes(),
        ))
        .expect("invalid webrtc version properties file")
    });
    pub static WEBRTC_VERSION: LazyLock<&String> = LazyLock::new(|| {
        VERSION_PROPERTIES
            .get("webrtc.version")
            .expect("no webrtc.version property in versions property file")
    });
    pub static GCLIENT_FILE: LazyLock<&str> = LazyLock::new(|| match TARGET_OS.as_str() {
        "windows" => ".gclient.windows",
        "ios" | "macos" => ".gclient.apple",
        "android" | "linux" => ".gclient.unix",
        _ => panic!("Invalid target os gclient file specified"),
    });
}
