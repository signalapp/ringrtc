//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{process::Stdio, time::Duration};

use anyhow::Result;
use bollard::{
    container::{MemoryStatsStats, Stats, StatsOptions},
    Docker,
};
use chrono::DateTime;
use futures_util::stream::TryStreamExt;
use itertools::Itertools;
use tokio::{
    fs::OpenOptions,
    io::{stdout, AsyncWriteExt},
    process::Command,
};

use crate::{
    common::{
        CallConfig, CallProfile, ClientProfile, DelayVariationStrategy, GeLossModel, Loss,
        MarkovLossModel, NetworkConfig,
    },
    test::{CallTypeConfig, MediaFileIo},
};

/// This function builds all docker images that we need.
pub async fn build_images() -> Result<()> {
    println!("\nBuilding images:");

    println!("cli:");
    stdout().flush().await?;
    let _ = Command::new("docker")
        .args([
            "build",
            "-t",
            "ringrtc-cli",
            "-q",
            "-f",
            "call_sim/docker/ringrtc/Dockerfile",
            ".",
        ])
        .spawn()?
        .wait()
        .await?;

    println!("signaling-server:");
    stdout().flush().await?;
    let _ = Command::new("docker")
        .args([
            "build",
            "-t",
            "signaling-server",
            "-q",
            "-f",
            "call_sim/docker/signaling_server/Dockerfile",
            ".",
        ])
        .spawn()?
        .wait()
        .await?;

    println!("visqol_mos:");
    stdout().flush().await?;
    let _ = Command::new("docker")
        .current_dir("call_sim/docker/visqol_mos")
        .args(["build", "-t", "visqol_mos", "-q", "."])
        .spawn()?
        .wait()
        .await?;

    println!("pesq_mos:");
    stdout().flush().await?;
    let _ = Command::new("docker")
        .current_dir("call_sim/docker/pesq_mos")
        .args(["build", "-t", "pesq_mos", "-q", "."])
        .spawn()?
        .wait()
        .await?;

    println!("plc_mos:");
    stdout().flush().await?;
    let _ = Command::new("docker")
        .current_dir("call_sim/docker/plc_mos")
        .args(["build", "-t", "plc_mos", "-q", "."])
        .spawn()?
        .wait()
        .await?;

    Ok(())
}

/// This function cleans all docker containers that were used.
pub async fn clean_up(container_names: Vec<&str>) -> Result<()> {
    println!("\nCleaning up containers:");

    // Ignore errors and try to move on.
    for container in container_names.iter() {
        let result = Command::new("docker")
            .args(["rm", "--force", "--volumes", container])
            .stderr(Stdio::null())
            .spawn()?
            .wait()
            .await;
        if result.is_err() {
            println!("  Couldn't remove {}", container);
        }
    }

    Ok(())
}

pub async fn create_network() -> Result<()> {
    println!("\nCreating networks:");

    let _ = Command::new("docker")
        .args([
            "network",
            "create",
            "--subnet",
            "172.28.0.0/24",
            "ringrtc_default",
        ])
        .spawn()?
        .wait()
        .await?;

    Ok(())
}

pub async fn clean_network() -> Result<()> {
    println!("\nCleaning networks:");

    let _ = Command::new("docker")
        .args(["network", "rm", "ringrtc_default"])
        .spawn()?
        .wait()
        .await;

    Ok(())
}

pub async fn start_signaling_server() -> Result<()> {
    println!("\nStarting Signaling Server:");

    let _ = Command::new("docker")
        .args([
            "run",
            "--name",
            "signaling_server",
            "-d",
            "--privileged",
            "--network",
            "ringrtc_default",
            "--ip",
            "172.28.0.250",
            "-p",
            "9090:8080",
            "--stop-signal",
            "SIGINT",
            "signaling-server",
        ])
        .spawn()?
        .wait()
        .await?;

    Ok(())
}

/// Starts a TURN server at 172.28.0.251, but might be available at `turn`. Exposes STUN and
/// TURN at the standard port (3478) via UDP and TURN at port 80 for TCP. Media ports are
/// restricted to the range 50200 to 50250. TURN access is authenticated but using the static
/// credentials of test/test.
///
/// Note: We'll typically use one server, turn, to serve both clients.
pub async fn start_turn_server() -> Result<()> {
    println!("\nStarting TURN/relay server");

    let _ = Command::new("docker")
        .args([
            "run",
            "--name",
            "turn",
            "-d",
            "--privileged",
            "--network",
            "ringrtc_default",
            "--ip",
            "172.28.0.251",
            "-p",
            "3478:3478/udp",
            "-p",
            "80:80",
            "-p",
            "50200-50250:50200-50250/udp",
            "--entrypoint",
            "turnserver",
            "coturn/coturn",
            "--external-ip=172.28.0.251/172.28.0.251",
            "--relay-ip=172.28.0.251",
            "--prod",
            "-n",
            "-v",
            "--fingerprint",
            "--listening-port=3478",
            "--alt-listening-port=80",
            "--realm=signal.org",
            "--no-multicast-peers",
            "--min-port=50200",
            "--max-port=50250",
            "--no-cli",
            "--log-file=stdout",
            "--no-dtls",
            "--no-sslv2",
            "--no-sslv3",
            "--no-tlsv1",
            "--no-tlsv1_1",
            "--lt-cred-mech",
            "--user=test:test",
        ])
        .spawn()?
        .wait()
        .await?;

    Ok(())
}

pub async fn start_tcp_dump(report_path: &str) -> Result<()> {
    println!("\nStarting tcpdump");

    let _ = Command::new("docker")
        .args([
            "run",
            "--name",
            "tcpdump",
            "-d",
            "--net=host",
            "-v",
            &format!("{}:/tcpdump", report_path),
            "kaazing/tcpdump",
        ])
        .spawn()?
        .wait()
        .await?;

    Ok(())
}

/// Starts a client in a bash shell, waiting for future exec commands to actually do
/// something useful.
pub async fn start_client(name: &str, report_path: &str, media_path: &str) -> Result<()> {
    println!("\nStarting Client `{}`:", name);

    let _ = Command::new("docker")
        .args([
            "run",
            "--name",
            name,
            "-dit",
            "--privileged",
            "--network",
            "ringrtc_default",
            "-v",
            &format!("{}:/report", report_path),
            "-v",
            &format!("{}:/media", media_path),
            "--cap-add",
            "NET_ADMIN",
            "ringrtc-cli",
            "/bin/bash",
        ])
        .spawn()?
        .wait()
        .await?;

    Ok(())
}

/// Appends netem options to args vector based on the given NetworkConfig.
fn append_netem_options(network_config: &NetworkConfig, args: &mut Vec<String>) {
    // limit: maximum number of packets the qdisc may hold queued at a time.
    if network_config.limit > 0 {
        args.push("limit".to_string());
        args.push(format!("{}", network_config.limit));
    }

    if network_config.delay > 0 {
        args.push("delay".to_string());
        args.push(format!("{}ms", network_config.delay));

        if network_config.delay_variability > 0 {
            // In later documentation, this is now called jitter.
            args.push(format!("{}ms", network_config.delay_variability));

            match network_config.delay_variation_strategy {
                Some(DelayVariationStrategy::Correlation(correlation)) => {
                    args.push(format!("{}%", correlation));
                }
                Some(DelayVariationStrategy::Distribution(distribution)) => {
                    args.push("distribution".to_string());
                    args.push(format!("{}", distribution));
                }
                _ => {}
            }
        }
    }

    match &network_config.loss {
        Some(Loss::Percentage(percentage)) => {
            args.push("loss".to_string());
            args.push(format!("{}%", percentage));
        }
        Some(Loss::GeModel(ge_loss_model)) => {
            args.extend_from_slice(&["loss".to_string(), "gemodel".to_string()]);
            match ge_loss_model {
                GeLossModel::Bernoulli { p } => {
                    args.extend_from_slice(&[format!("{}%", p)]);
                }
                GeLossModel::SimpleGilbert { p, r } => {
                    args.extend_from_slice(&[format!("{}%", p), format!("{}%", r)]);
                }
                GeLossModel::Gilbert { p, r, one_minus_h } => {
                    args.extend_from_slice(&[
                        format!("{}%", p),
                        format!("{}%", r),
                        format!("{}%", one_minus_h),
                    ]);
                }
                GeLossModel::GilbertElliot {
                    p,
                    r,
                    one_minus_h,
                    one_minus_k,
                } => {
                    args.extend_from_slice(&[
                        format!("{}%", p),
                        format!("{}%", r),
                        format!("{}%", one_minus_h),
                        format!("{}%", one_minus_k),
                    ]);
                }
            }
        }
        Some(Loss::State(markov_model)) => {
            args.extend_from_slice(&["loss".to_string(), "state".to_string()]);
            match markov_model {
                MarkovLossModel::Bernoulli { p13 } => {
                    args.extend_from_slice(&[format!("{}%", p13)]);
                }
                MarkovLossModel::TwoState { p13, p31 } => {
                    args.extend_from_slice(&[format!("{}%", p13), format!("{}%", p31)]);
                }
                MarkovLossModel::ThreeState { p13, p31, p32, p23 } => {
                    args.extend_from_slice(&[
                        format!("{}%", p13),
                        format!("{}%", p31),
                        format!("{}%", p32),
                        format!("{}%", p23),
                    ]);
                }
                MarkovLossModel::FourState {
                    p13,
                    p31,
                    p32,
                    p23,
                    p14,
                } => {
                    args.extend_from_slice(&[
                        format!("{}%", p13),
                        format!("{}%", p31),
                        format!("{}%", p32),
                        format!("{}%", p23),
                        format!("{}%", p14),
                    ]);
                }
            }
        }
        _ => {}
    }

    if network_config.duplication > 0 {
        args.push("duplicate".to_string());
        args.push(format!("{}%", network_config.duplication));
    }

    if network_config.corruption > 0 {
        args.push("corrupt".to_string());
        args.push(format!("{}%", network_config.corruption));
    }

    if network_config.reorder > 0 {
        args.push("reorder".to_string());
        args.push(format!("{}%", network_config.reorder));

        if network_config.reorder_correlation > 0 {
            args.push(format!("{}%", network_config.reorder_correlation));
        }

        if network_config.reorder_gap > 0 {
            args.push("gap".to_string());
            args.push(format!("{}", network_config.reorder_gap));
        }
    }

    if network_config.slot > 0 {
        args.push("slot".to_string());
        // Simplistic approach, assign desired time to both min and max delay.
        args.push(format!("{}ms", network_config.slot));
        args.push(format!("{}ms", network_config.slot));
    }
}

/// Setup and start network emulation for a given client.
pub async fn emulate_network_start(name: &str, network_config: &NetworkConfig) -> Result<()> {
    // Add a qdisc to the root of interface eth0. The source of the commands used in this
    // function are from `tcconfig ... --tc-command`, which dumps the `tc` commands used
    // to achieve the result. They use 'htb' not 'tbf', so we'll go with it.
    //
    // We will use the common handle `1a1a`.
    let _ = Command::new("docker")
        .args([
            "exec", name, "tc", "qdisc", "add", "dev", "eth0", "root", "handle", "1a1a:", "htb",
            "default", "1",
        ])
        .spawn()?
        .wait()
        .await?;

    // Add a class to the qdisc. This is used for traffic that isn't emulated. We will leave
    // the bitrate wide-open for that (10Gbps).
    //
    // Use class id `1a1a:1` for traffic that isn't emulated.
    let _ = Command::new("docker")
        .args([
            "exec",
            name,
            "tc",
            "class",
            "add",
            "dev",
            "eth0",
            "parent",
            "1a1a:",
            "classid",
            "1a1a:1",
            "htb",
            "rate",
            "10000000.0kbit",
        ])
        .spawn()?
        .wait()
        .await?;

    // We will use the htb to specify the rate that we want to emulate. If we haven't set
    // any value (i.e. it is zero), then we will still set it, but to be wide-open.
    // Note: We also could specify `burst` and `cburst` bytes, but let's see if the default
    // values are good enough for now.
    let mut rate = 10_000_000u64;
    if network_config.rate > 0 {
        rate = network_config.rate as u64;
    }

    // Apply the rate here (not in the netem command which is next).
    //
    // Use class id `1a1a:88` for emulated traffic.
    let _ = Command::new("docker")
        .args([
            "exec",
            name,
            "tc",
            "class",
            "add",
            "dev",
            "eth0",
            "parent",
            "1a1a:",
            "classid",
            "1a1a:88",
            "htb",
            "rate",
            &format!("{}.0kbit", rate),
            "ceil",
            &format!("{}.0kbit", rate),
        ])
        .spawn()?
        .wait()
        .await?;

    // Now we will add the actual network emulation to the qdisc. The parent shall be the
    // class identified by `1a1a:88`. The actual parameters to apply come next.
    let mut args = [
        "exec", name, "tc", "qdisc", "add", "dev", "eth0", "parent", "1a1a:88", "handle", "2518:",
        "netem",
    ]
    .map(String::from)
    .to_vec();

    append_netem_options(network_config, &mut args);

    // Note: As mentioned above, we won't use the rate option for netem.

    // Now issue the netem command.
    let _ = Command::new("docker").args(&args).spawn()?.wait().await?;

    // Now create filters for specific ports and services that we don't want to emulate, in
    // order to maintain the sanctity of our tests (for example, the connection from the cli
    // to the signaling server).
    //
    // All of these operate on the flowid matching the class id `1a1a:1`.

    // DNS
    let _ = Command::new("docker")
        .args([
            "exec", name, "tc", "filter", "add", "dev", "eth0", "protocol", "ip", "parent",
            "1a1a:", "prio", "1", "u32", "match", "ip", "dport", "53", "0xffff", "flowid",
            "1a1a:1",
        ])
        .spawn()?
        .wait()
        .await?;

    // 8080
    let _ = Command::new("docker")
        .args([
            "exec", name, "tc", "filter", "add", "dev", "eth0", "protocol", "ip", "parent",
            "1a1a:", "prio", "1", "u32", "match", "ip", "dport", "8080", "0xffff", "flowid",
            "1a1a:1",
        ])
        .spawn()?
        .wait()
        .await?;

    // ARP (This does not work "Error: Filter with specified priority/protocol not found.")
    // let _ = Command::new("docker")
    //     .args(&[
    //         "exec", name, "tc", "filter", "add", "dev", "eth0", "protocol", "arp", "parent",
    //         "1a1a:", "prio", "1", "u32", "match", "u32", "0", "0", "flowid", "1a1a:1",
    //     ])
    //     .spawn()?
    //     .wait()
    //     .await?;

    // ICMP
    let _ = Command::new("docker")
        .args([
            "exec", name, "tc", "filter", "add", "dev", "eth0", "parent", "1a1a:", "prio", "2",
            "protocol", "ip", "u32", "match", "ip", "protocol", "1", "0xff", "flowid", "1a1a:1",
        ])
        .spawn()?
        .wait()
        .await?;

    // IGMP
    let _ = Command::new("docker")
        .args([
            "exec", name, "tc", "filter", "add", "dev", "eth0", "parent", "1a1a:", "prio", "3",
            "protocol", "ip", "u32", "match", "ip", "protocol", "2", "0xff", "flowid", "1a1a:1",
        ])
        .spawn()?
        .wait()
        .await?;

    // DHCP
    let _ = Command::new("docker")
        .args([
            "exec", name, "tc", "filter", "add", "dev", "eth0", "parent", "1a1a:", "prio", "4",
            "protocol", "ip", "u32", "match", "ip", "protocol", "17", "0xff", "match", "ip",
            "dport", "67", "0xffff", "flowid", "1a1a:1",
        ])
        .spawn()?
        .wait()
        .await?;
    let _ = Command::new("docker")
        .args([
            "exec", name, "tc", "filter", "add", "dev", "eth0", "parent", "1a1a:", "prio", "4",
            "protocol", "ip", "u32", "match", "ip", "protocol", "17", "0xff", "match", "ip",
            "dport", "68", "0xffff", "flowid", "1a1a:1",
        ])
        .spawn()?
        .wait()
        .await?;

    // Finally, open up the filter for anything else to be emulated and associate the flowid
    // to the class id `1a1a:88`.
    let _ = Command::new("docker")
        .args([
            "exec",
            name,
            "tc",
            "filter",
            "add",
            "dev",
            "eth0",
            "protocol",
            "ip",
            "parent",
            "1a1a:",
            "prio",
            "5",
            "u32",
            "match",
            "ip",
            "dst",
            "0.0.0.0/0",
            "match",
            "ip",
            "src",
            "0.0.0.0/0",
            "flowid",
            "1a1a:88",
        ])
        .spawn()?
        .wait()
        .await?;

    Ok(())
}

/// Change the existing emulation to avoid reloading the qdisc.
pub async fn emulate_network_change(name: &str, network_config: &NetworkConfig) -> Result<()> {
    let mut rate = 10_000_000u64;
    if network_config.rate > 0 {
        rate = network_config.rate as u64;
    }

    let _ = Command::new("docker")
        .args([
            "exec",
            name,
            "tc",
            "class",
            "change",
            "dev",
            "eth0",
            "parent",
            "1a1a:",
            "classid",
            "1a1a:88",
            "htb",
            "rate",
            &format!("{}.0kbit", rate),
            "ceil",
            &format!("{}.0kbit", rate),
        ])
        .spawn()?
        .wait()
        .await?;

    let mut args = [
        "exec", name, "tc", "qdisc", "change", "dev", "eth0", "parent", "1a1a:88", "handle",
        "2518:", "netem",
    ]
    .map(String::from)
    .to_vec();

    append_netem_options(network_config, &mut args);

    let _ = Command::new("docker").args(&args).spawn()?.wait().await?;

    Ok(())
}

#[allow(dead_code)]
pub async fn emulate_network_clear(name: &str) -> Result<()> {
    let _ = Command::new("docker")
        .args(["exec", name, "tc", "qdisc", "del", "dev", "eth0", "root"])
        .spawn()?
        .wait()
        .await?;

    Ok(())
}

pub async fn start_cli(
    name: &str,
    media_io: MediaFileIo,
    call_config: &CallConfig,
    remote_call_config: &CallConfig,
    client_profile: &ClientProfile,
    call_type: &CallTypeConfig,
    profile: bool,
) -> Result<()> {
    println!("Starting cli for `{}`", name);
    let log_file_arg = format!("/report/{}.log", name);
    let input_file_arg = format!("/media/{}", media_io.audio_input_file);

    let mut args = ["exec", "-d", name].map(String::from).to_vec();

    if profile {
        let perf_arg = format!("--output=/report/{}.perf", name);

        args.extend_from_slice(
            &[
                "perf",
                "record",
                "-e",
                "cycles",
                "--call-graph=dwarf",
                "-F",
                "1499",
                "--user-callchains",
                "--sample-cpu",
                &perf_arg,
            ]
            .map(String::from),
        );
    }

    args.extend_from_slice(
        &[
            "call_sim-cli",
            "--name",
            name,
            "--log-file",
            &log_file_arg,
            "--input-file",
            &input_file_arg,
        ]
        .map(String::from),
    );
    if let Some(audio_output_file) = media_io.audio_output_file {
        args.push(format!("--output-file=/report/{}", audio_output_file));
    }

    args.push("--stats-interval-secs".to_string());
    args.push(format!("{}", call_config.stats_interval_secs));

    args.push("--stats-initial-offset-secs".to_string());
    args.push(format!("{}", call_config.stats_initial_offset_secs));

    args.push("--allowed-bitrate-kbps".to_string());
    args.push(format!("{}", call_config.allowed_bitrate_kbps));

    args.push(format!(
        "--initial-packet-size-ms={}",
        call_config.audio.initial_packet_size_ms
    ));
    args.push(format!(
        "--min-packet-size-ms={}",
        call_config.audio.min_packet_size_ms
    ));
    args.push(format!(
        "--max-packet-size-ms={}",
        call_config.audio.max_packet_size_ms
    ));

    args.push(format!(
        "--initial-bitrate-bps={}",
        call_config.audio.initial_bitrate_bps
    ));
    args.push(format!(
        "--min-bitrate-bps={}",
        call_config.audio.min_bitrate_bps
    ));
    args.push(format!(
        "--max-bitrate-bps={}",
        call_config.audio.max_bitrate_bps
    ));

    args.push(format!("--bandwidth={}", call_config.audio.bandwidth));
    args.push(format!("--complexity={}", call_config.audio.complexity));
    args.push(format!("--adaptation={}", call_config.audio.adaptation));

    args.push(format!("--cbr={}", call_config.audio.enable_cbr));
    args.push(format!("--dtx={}", call_config.audio.enable_dtx));
    args.push(format!("--fec={}", call_config.audio.enable_fec));

    args.push(format!("--tcc={}", call_config.audio.enable_tcc));

    args.push(format!("--vp9={}", call_config.video.enable_vp9));

    args.push(format!(
        "--high-pass-filter={}",
        call_config.audio.enable_high_pass_filter,
    ));
    args.push(format!("--aec={}", call_config.audio.enable_aec));
    args.push(format!("--ns={}", call_config.audio.enable_ns));
    args.push(format!("--agc={}", call_config.audio.enable_agc));

    let mut field_trials = call_config.field_trials.join("/");
    if !field_trials.is_empty() {
        field_trials.push('/');
    }

    args.push(format!("--field-trials={}", field_trials));

    for relay_server in &call_config.relay_servers {
        args.push(format!("--relay-servers={}", relay_server));
    }

    args.push(format!("--relay-username={}", call_config.relay_username));
    args.push(format!("--relay-password={}", call_config.relay_password));
    args.push(format!("--force-relay={}", call_config.force_relay));

    args.push(format!(
        "--audio-jitter-buffer-max-packets={}",
        call_config.audio.jitter_buffer_max_packets
    ));

    args.push(format!(
        "--audio-jitter-buffer-min-delay-ms={}",
        call_config.audio.jitter_buffer_min_delay_ms
    ));

    args.push(format!(
        "--audio-jitter-buffer-max-target-delay-ms={}",
        call_config.audio.jitter_buffer_max_target_delay_ms
    ));

    args.push(format!(
        "--audio-jitter-buffer-fast-accelerate={}",
        call_config.audio.jitter_buffer_fast_accelerate
    ));

    args.push(format!(
        "--audio-rtcp-report-interval-ms={}",
        call_config.audio.rtcp_report_interval_ms
    ));

    if let Some(input_video_file) = media_io.video_input_file {
        args.push(format!("--input-video-file=/media/{}", input_video_file));
    }
    if let Some(output_video_file) = media_io.video_output_file {
        args.push(format!("--output-video-file=/report/{}", output_video_file));
    }

    if let Some((width, height)) = remote_call_config.video.dimensions() {
        args.push(format!("--output-video-width={}", width));
        args.push(format!("--output-video-height={}", height));
    }

    if name == "client_a" {
        args.push("--ip=172.28.0.2".to_string());
    } else {
        args.push("--ip=172.28.0.3".to_string());
    }

    if let CallProfile::DeterministicLoss(loss_rate) = call_config.profile {
        args.push(format!("--deterministic-loss={}", loss_rate));
    }

    args.extend(call_config.extra_cli_args.iter().cloned());

    args.push(format!("--user-id={}", client_profile.user_id));
    args.push(format!("--device-id={}", client_profile.device_id));
    if let CallTypeConfig::Group {
        sfu_url,
        group_name,
    } = call_type
    {
        args.push(format!("--sfu-url={}", sfu_url));
        args.push("--is-group-call".to_string());

        let group = if let Some(group_name) = group_name {
            client_profile
                .groups
                .iter()
                .filter(|&g| group_name == &g.name)
                .exactly_one()
                .map_err(|_| {
                    anyhow::anyhow!("Did't find exactly one group named: {:?}", group_name)
                })?
        } else {
            client_profile
                .groups
                .first()
                .expect("at least one group info detailed")
        };
        args.push(format!("--group-id={}", group.id));
        args.push(format!("--membership-proof={}", group.membership_proof));

        let member_info = group
            .members
            .iter()
            .map(|member| format!("{}:{}", member.user_id, member.member_id))
            .join(",");
        args.push(format!("--group-member-info={}", member_info));
    }

    println!("Final Client args: {}", args.join(" "));
    let _ = Command::new("docker").args(&args).spawn()?.wait().await?;

    Ok(())
}

pub async fn finish_perf(client: &str) -> Result<()> {
    let mut exited = false;
    for _ in 0..60 {
        let status = Command::new("docker")
            .args(["exec", client, "pgrep", "perf"])
            .stdout(Stdio::null())
            .spawn()?
            .wait()
            .await?;
        if !status.success() {
            // if we couldn't find it, it exited; otherwise keep waiting.
            exited = true;
            let perf_command = format!(
                "perf report -s symbol --percent-limit=5 --call-graph=2 -i /report/{}.perf \
                --addr2line=/root/.cargo/bin/addr2line > /report/{}.perf.txt 2>&1",
                client, client
            );
            let _ = Command::new("docker")
                .args(["exec", client, "sh", "-c", &perf_command])
                .spawn()?
                .wait()
                .await?;

            let _ = Command::new("docker")
                .args([
                    "exec",
                    client,
                    "chmod",
                    "o+r",
                    &format!("/report/{}.perf", client),
                ])
                .spawn()?
                .wait()
                .await?;

            let _ = Command::new("docker")
                .args([
                    "exec",
                    client,
                    "perf",
                    "archive",
                    &format!("/report/{}.perf", client),
                ])
                .spawn()?
                .wait()
                .await?;

            break;
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    println!("{} perf exited? {}", client, exited);
    Ok(())
}

pub async fn convert_raw_to_wav(
    location: &str,
    raw_file: &str,
    wav_file: &str,
    length: Option<u16>,
) -> Result<()> {
    println!("\nConverting raw file `{}` to wav:", raw_file);

    let mut args = [
        "run",
        "--rm",
        "-v",
        &format!("{}:/work", location),
        "bigpapoo/sox",
        "mysox",
        "-V1",
        "-t",
        "raw",
        "-r",
        "48000",
        "-b",
        "16",
        "-c",
        "2",
        "-L",
        "-e",
        "signed-integer",
        raw_file,
        wav_file,
    ]
    .map(String::from)
    .to_vec();

    if let Some(length) = length {
        // Make sure the wav audio is of the expected length to smooth out MOS measurements.
        args.push("pad".to_string());
        args.push("0".to_string());
        args.push(format!("{}", length));
        args.push("trim".to_string());
        args.push("0".to_string());
        args.push(format!("{}", length));
    }

    let _ = Command::new("docker").args(&args).spawn()?.wait().await?;

    Ok(())
}

pub async fn convert_wav_to_16khz_mono(
    location: &str,
    input_file: &str,
    output_file: &str,
) -> Result<()> {
    println!("\nConverting file `{}` to 16kHz/mono wav:", input_file);

    let args = [
        "run",
        "--rm",
        "-v",
        &format!("{}:/work", location),
        "bigpapoo/sox",
        "mysox",
        "-V1",
        input_file,
        "-r",
        "16000",
        "-c",
        "1",
        output_file,
    ]
    .map(String::from)
    .to_vec();

    let _ = Command::new("docker").args(&args).spawn()?.wait().await?;

    Ok(())
}

pub async fn convert_mp4_to_yuv(location: &str, mp4_file: &str, yuv_file: &str) -> Result<()> {
    println!("\nConverting `{mp4_file}` to YUV:");

    let args = [
        "run",
        "--rm",
        "-v",
        &format!("{location}:/work"),
        "linuxserver/ffmpeg",
        "-hide_banner",
        "-loglevel",
        "warning",
        "-i",
        &format!("/work/{mp4_file}"),
        &format!("/work/{yuv_file}"),
    ];

    let _ = Command::new("docker").args(args).spawn()?.wait().await?;

    Ok(())
}

pub async fn convert_yuv_to_mp4(
    location: &str,
    yuv_file: &str,
    mp4_file: &str,
    dimensions: (u16, u16),
) -> Result<()> {
    println!("\nConverting `{yuv_file}` to MP4:");

    let args = [
        "run",
        "--rm",
        "-v",
        &format!("{location}:/work"),
        "linuxserver/ffmpeg",
        "-hide_banner",
        "-loglevel",
        "warning",
        "-s",
        &format!("{}x{}", dimensions.0, dimensions.1),
        "-r",
        "30",
        "-i",
        &format!("/work/{yuv_file}"),
        &format!("/work/{mp4_file}"),
    ];

    let _ = Command::new("docker").args(args).spawn()?.wait().await?;

    Ok(())
}

pub async fn generate_spectrogram(location: &str, wav_file: &str, extension: &str) -> Result<()> {
    println!("\nGenerating spectrogram for `{}`:", wav_file);

    let _ = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-v",
            &format!("{}:/work", location),
            "bigpapoo/sox",
            "mysox",
            wav_file,
            "-n",
            // Only show the first channel.
            "remix",
            "1",
            "spectrogram",
            // Limit height since we are only showing one channel.
            "-y",
            "257",
            "-o",
            &format!("{}.{}", wav_file, extension),
        ])
        .spawn()?
        .wait()
        .await?;

    Ok(())
}

pub async fn analyze_visqol_mos(
    degraded_path: &str,
    degraded_file: &str,
    ref_path: &str,
    ref_file: &str,
    extension: &str,
    speech: bool,
) -> Result<()> {
    println!("\nAnalyzing visqol mos for `{}`:", degraded_file);

    let mut args = [
        "run",
        "--name",
        "visqol_mos",
        "-v",
        &format!("{}:/degraded", degraded_path),
        "-v",
        &format!("{}:/ref", ref_path),
        "visqol_mos",
        "--degraded_file",
        &format!("/degraded/{}", degraded_file),
        "--reference_file",
        &format!("/ref/{}", ref_file),
    ]
    .map(String::from)
    .to_vec();

    if speech {
        args.push("--use_speech_mode".to_string());
    }

    let _ = Command::new("docker").args(&args).spawn()?.wait().await?;

    // Get the logs.
    let output = Command::new("docker")
        .args(["logs", "visqol_mos"])
        .output()
        .await?;

    // Remove the container.
    let _ = Command::new("docker")
        .args(["rm", "visqol_mos"])
        .spawn()?
        .wait()
        .await?;

    // Save the logs.
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(format!("{}/{}.{}", degraded_path, degraded_file, extension))
        .await?;
    file.write_all(&output.stdout).await?;
    file.write_all(&output.stderr).await?;

    Ok(())
}

pub async fn analyze_pesq_mos(
    degraded_path: &str,
    degraded_file: &str,
    ref_path: &str,
    ref_file: &str,
    extension: &str,
) -> Result<()> {
    println!("\nAnalyzing pesq mos for `{}`:", degraded_file);

    let args = [
        "run",
        "--name",
        "pesq_mos",
        "-v",
        &format!("{}:/degraded", degraded_path),
        "-v",
        &format!("{}:/ref", ref_path),
        "pesq_mos",
        &format!("/ref/{}", ref_file),
        &format!("/degraded/{}", degraded_file),
    ]
    .map(String::from)
    .to_vec();

    let _ = Command::new("docker").args(&args).spawn()?.wait().await?;

    // Get the logs.
    let output = Command::new("docker")
        .args(["logs", "pesq_mos"])
        .output()
        .await?;

    // Remove the container.
    let _ = Command::new("docker")
        .args(["rm", "pesq_mos"])
        .spawn()?
        .wait()
        .await?;

    // Save the logs.
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(format!("{}/{}.{}", degraded_path, degraded_file, extension))
        .await?;
    file.write_all(&output.stdout).await?;
    file.write_all(&output.stderr).await?;

    Ok(())
}

pub async fn analyze_plc_mos(
    degraded_path: &str,
    degraded_file: &str,
    extension: &str,
) -> Result<()> {
    println!("\nAnalyzing plc mos for `{}`:", degraded_file);

    let args = [
        "run",
        "--name",
        "plc_mos",
        "-v",
        &format!("{}:/degraded", degraded_path),
        "plc_mos",
        "--degraded",
        &format!("/degraded/{}", degraded_file),
    ]
    .map(String::from)
    .to_vec();

    let _ = Command::new("docker").args(&args).spawn()?.wait().await?;

    // Get the logs.
    let output = Command::new("docker")
        .args(["logs", "plc_mos"])
        .output()
        .await?;

    // Remove the container.
    let _ = Command::new("docker")
        .args(["rm", "plc_mos"])
        .spawn()?
        .wait()
        .await?;

    // Save the logs.
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(format!("{}/{}.{}", degraded_path, degraded_file, extension))
        .await?;
    file.write_all(&output.stdout).await?;
    file.write_all(&output.stderr).await?;

    Ok(())
}

pub async fn analyze_video(
    degraded_path: &str,
    degraded_file: &str,
    ref_path: &str,
    ref_file: &str,
    dimensions: (u16, u16),
) -> Result<()> {
    println!("\nAnalyzing video for `{}`:", degraded_file);

    let output = Command::new("docker")
        .args([
            "run",
            "--name",
            "vmaf",
            "-v",
            &format!("{}:/degraded", degraded_path),
            "-v",
            &format!("{}:/ref", ref_path),
            "vmaf",
            "yuv420p",
            &dimensions.0.to_string(),
            &dimensions.1.to_string(),
            &format!("/ref/{}", ref_file),
            &format!("/degraded/{}", degraded_file),
            "--phone-model",
            "--out-fmt",
            "json",
        ])
        .output()
        .await?;

    // Remove the container.
    let _ = Command::new("docker")
        .args(["rm", "vmaf"])
        .spawn()?
        .wait()
        .await?;

    // Save the output.
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(format!("{}/{}.json", degraded_path, degraded_file))
        .await?;
    file.write_all(&output.stdout).await?;

    Ok(())
}

pub async fn get_signaling_server_logs(path: &str) -> Result<()> {
    // Get the logs.
    let output = Command::new("docker")
        .args(["logs", "signaling_server"])
        .output()
        .await?;

    // Save the logs.
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(format!("{}/signaling_server.log", path))
        .await?;
    file.write_all(&output.stdout).await?;
    file.write_all(&output.stderr).await?;

    Ok(())
}

pub async fn get_turn_server_logs(path: &str) -> Result<()> {
    // Get the logs.
    let output = Command::new("docker")
        .args(["logs", "turn"])
        .output()
        .await?;

    // Save the logs.
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(format!("{}/turn.log", path))
        .await?;
    file.write_all(&output.stdout).await?;
    file.write_all(&output.stderr).await?;

    Ok(())
}

pub struct DockerStats {
    docker: Docker,
}

impl DockerStats {
    pub async fn new() -> Result<Self> {
        let docker = Docker::connect_with_unix_defaults()?;
        Ok(DockerStats { docker })
    }

    pub fn start(&self, name: &str, path: &str) -> Result<()> {
        let docker = self.docker.clone();
        let name = name.to_string();
        let path = path.to_string();

        tokio::spawn(async move {
            let stream = &mut docker.stats(
                &name,
                Some(StatsOptions {
                    stream: true,
                    ..Default::default()
                }),
            );

            // Collect the stats. This will await until the container is stopped then dump
            // all the stats to a log.
            match stream.try_collect::<Vec<Stats>>().await {
                Ok(stats) => {
                    match OpenOptions::new()
                        .write(true)
                        .create(true)
                        .truncate(true)
                        .open(format!("{}/{}_stats.log", path, name))
                        .await
                    {
                        Ok(mut file) => {
                            let _ = file
                                .write_all(b"Timestamp\tCPU\tMEM\tTX_Bitrate\tRX_Bitrate\n")
                                .await;

                            let mut prev_timestamp = 0i64;

                            let mut prev_tx_bytes = 0u64;
                            let mut prev_rx_bytes = 0u64;

                            let mut prev_total_cpu_usage = 0u64;
                            let mut prev_system_cpu_usage = 0u64;

                            for stat in stats {
                                match (
                                    stat.cpu_stats.system_cpu_usage,
                                    stat.cpu_stats.online_cpus,
                                    stat.memory_stats.usage,
                                    stat.memory_stats.stats,
                                    stat.networks,
                                ) {
                                    (
                                        Some(system_cpu_usage),
                                        Some(online_cpus),
                                        Some(memory_usage),
                                        Some(memory_stats),
                                        Some(networks),
                                    ) => {
                                        let timestamp = DateTime::parse_from_rfc3339(&stat.read)
                                            .expect("stats timestamp is valid")
                                            .timestamp_millis();

                                        let (tx_bitrate, rx_bitrate) = match networks.get("eth0") {
                                            Some(network) => {
                                                let time_delta =
                                                    (timestamp - prev_timestamp) as f32 / 1000.0;
                                                let tx_bitrate =
                                                    (network.tx_bytes - prev_tx_bytes) as f32 * 8.0
                                                        / time_delta;
                                                let rx_bitrate =
                                                    (network.rx_bytes - prev_rx_bytes) as f32 * 8.0
                                                        / time_delta;

                                                prev_timestamp = timestamp;
                                                prev_tx_bytes = network.tx_bytes;
                                                prev_rx_bytes = network.rx_bytes;

                                                if prev_timestamp == 0 {
                                                    // Ignore the first data point since there was no reference.
                                                    (0.0, 0.0)
                                                } else {
                                                    (tx_bitrate, rx_bitrate)
                                                }
                                            }
                                            None => {
                                                println!("Error: stat missing eth0!");
                                                break;
                                            }
                                        };

                                        // cpuPercent = (cpuDelta / systemDelta) * onlineCPUs * 100.0
                                        let cpu_percent = ((stat.cpu_stats.cpu_usage.total_usage
                                            - prev_total_cpu_usage)
                                            as f32
                                            / (system_cpu_usage - prev_system_cpu_usage) as f32)
                                            * online_cpus as f32
                                            * 100.0;

                                        let memory = memory_usage
                                            - match memory_stats {
                                                // Exclude file cache usage since it causes the stats to
                                                // grow over time and doesn't directly reflect RingRTC's
                                                // memory usage.
                                                // https://docs.docker.com/engine/reference/commandline/stats/#description
                                                MemoryStatsStats::V1(stats) => {
                                                    stats.total_inactive_file
                                                }
                                                MemoryStatsStats::V2(stats) => stats.inactive_file,
                                            };
                                        let _ = file
                                            .write_all(
                                                format!(
                                                    "{}\t{:.2}\t{}\t{:.0}\t{:.0}\n",
                                                    timestamp,
                                                    cpu_percent,
                                                    memory,
                                                    tx_bitrate,
                                                    rx_bitrate
                                                )
                                                .as_bytes(),
                                            )
                                            .await;

                                        prev_total_cpu_usage = stat.cpu_stats.cpu_usage.total_usage;
                                        prev_system_cpu_usage = system_cpu_usage;
                                    }
                                    _ => {
                                        println!("Error: stat missing required data!");
                                        break;
                                    }
                                }
                            }
                        }
                        Err(err) => {
                            println!("Error creating stats file: {:?}", err);
                        }
                    }
                }
                Err(err) => {
                    println!("Error collecting stats for {}: {:?}", name, err);
                }
            }
        });

        Ok(())
    }
}
