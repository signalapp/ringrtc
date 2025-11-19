//
// Copyright 2025 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

//! Virtual audio script wrappers, exposed for testing.

use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use anyhow::anyhow;

const VIRTUAL_AUDIO_SCRIPT: &str = include_str!("../../../bin/virtual_audio.sh");
const AUDIO_HAL_PATH: &str = "/Library/Audio/Plug-Ins/HAL/";

// Internal utility function to invoke the virtual audio script with given arguments.
fn run(args: &[&str]) -> anyhow::Result<()> {
    let mut script = Command::new("bash")
        .arg("-s")
        .arg("--")
        .args(args)
        .stdin(Stdio::piped())
        .spawn()?;

    let mut stdin = script.stdin.take().expect("failed to open bash stdin");
    std::thread::spawn(move || {
        stdin
            .write_all(VIRTUAL_AUDIO_SCRIPT.as_bytes())
            .expect("failed to write to bash stdin");
    });

    let status = script.wait()?;
    if !status.success() {
        Err(anyhow!("script exited with status {}", status))
    } else {
        Ok(())
    }
}

// Create devices |input_source| and |output_sink|.
// * |input_source| will be usable as a microphone / recording device
// * |output_sink| will be usable as a speaker / output device
fn setup(input_source: &str, output_sink: &str) -> anyhow::Result<()> {
    let args = [
        "--setup",
        "--input-source",
        input_source,
        "--output-sink",
        output_sink,
    ];
    if cfg!(target_os = "macos") {
        let input_path = Path::new(AUDIO_HAL_PATH).join(format!("{}.driver", input_source));
        let output_path = Path::new(AUDIO_HAL_PATH).join(format!("{}.driver", output_sink));
        if let Ok(true) = input_path.try_exists()
            && let Ok(true) = output_path.try_exists()
        {
            info!(
                "Assuming existing drivers in {} are correct",
                AUDIO_HAL_PATH
            );
            Ok(())
        } else {
            error!(
                "Need root to set up virtual audio on mac; try running the below at an interactive shell:"
            );
            error!("$PATH_TO_RINGRTC/bin/virtual_audio.sh {}", args.join(" "));
            Err(anyhow!(
                "Cannot automatically set up virtual audio on macos"
            ))
        }
    } else {
        run(&args)
    }
}

// Tear down the specified devices (which should have been created by |setup|)
fn teardown(input_source: &str, output_sink: &str) -> anyhow::Result<()> {
    let args = [
        "--teardown",
        "--input-source",
        input_source,
        "--output-sink",
        output_sink,
    ];
    if cfg!(target_os = "macos") {
        error!(
            "Need root to tear down virtual audio on mac; try running the below at an interactive shell:"
        );
        error!("$PATH_TO_RINGRTC/bin/virtual_audio.sh {}", args.join(" "));
        Err(anyhow!(
            "Cannot automatically tear down virtual audio on macos"
        ))
    } else {
        run(&args)
    }
}

// Start playing the sound contained in |input_file| so that it will be the
// content seen when recording from |input_source|.
// If |input_loops| is not None, loop |input_file| that number of times.
// If |output_file| is not None, save any output played to |output_sink| to that
// file.
fn play(
    input_source: &str,
    output_sink: &str,
    input_file: &Path,
    output_file: Option<&Path>,
    input_loops: Option<u32>,
) -> anyhow::Result<()> {
    let mut args = vec![
        "--play",
        "--input-source",
        input_source,
        "--output-sink",
        output_sink,
        "--input-file",
        input_file
            .to_str()
            .ok_or(anyhow!("Invalid input file name"))?,
    ];
    if let Some(file) = output_file {
        args.push("--output-file");
        args.push(file.to_str().ok_or(anyhow!("Invalid output file name"))?);
    }

    // Handle to keep this alive if needed
    let loops_str;
    if let Some(loops) = input_loops {
        args.push("--input-loops");
        loops_str = loops.to_string();
        args.push(&loops_str);
    }
    run(&args)
}

// Stop recording and playing.
fn stop(input_source: &str, output_sink: &str) -> anyhow::Result<()> {
    run(&[
        "--stop",
        "--input-source",
        input_source,
        "--output-sink",
        output_sink,
    ])
}

#[derive(Debug)]
pub struct VirtualAudioDevicePair {
    input_source: String,
    output_sink: String,
}

impl VirtualAudioDevicePair {
    pub fn new(input_source: &str, output_sink: &str) -> anyhow::Result<Self> {
        setup(input_source, output_sink)?;
        Ok(VirtualAudioDevicePair {
            input_source: input_source.to_string(),
            output_sink: output_sink.to_string(),
        })
    }

    pub fn play(
        &self,
        input_file: &Path,
        output_file: Option<&Path>,
        input_loops: Option<u32>,
    ) -> anyhow::Result<()> {
        play(
            &self.input_source,
            &self.output_sink,
            input_file,
            output_file,
            input_loops,
        )
    }

    pub fn stop(&self) -> anyhow::Result<()> {
        stop(&self.input_source, &self.output_sink)
    }

    pub fn input_source(&self) -> &str {
        self.input_source.as_str()
    }

    pub fn output_sink(&self) -> &str {
        self.output_sink.as_str()
    }
}

impl Drop for VirtualAudioDevicePair {
    fn drop(&mut self) {
        if let Err(e) = teardown(&self.input_source, &self.output_sink) {
            error!("Failed to tear down {:?} {}", self, e);
        }
    }
}
