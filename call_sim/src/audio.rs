//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use anyhow::Result;
use itertools::Itertools;
use std::{ffi::OsStr, path::Path};

use crate::docker::analyze_audio;
use crate::report::{AnalysisReport, AnalysisReportMos, Stats, StatsConfig, StatsData};
use crate::test::TestResults;

pub struct ChopAudioResult {
    file_names: Vec<String>,
    reference_time_secs: u32,
    degraded_time_secs: u32,
}

/// Chop a long degraded audio file into parts equal to the length of the reference file.
pub fn chop_audio(
    degraded_path: &str,
    degraded_file: &str,
    ref_path: &str,
    ref_file: &str,
) -> Result<ChopAudioResult> {
    println!("\nChopping audio for `{}`:", degraded_file);

    let reference = hound::WavReader::open(format!("{}/{}", ref_path, ref_file))?;
    let reference_time_secs = reference.duration() / reference.spec().sample_rate;

    let mut degraded = hound::WavReader::open(format!("{}/{}", degraded_path, degraded_file))?;
    let degraded_time_secs = degraded.duration() / degraded.spec().sample_rate;

    let degraded_name = Path::new(degraded_file);
    let degraded_stem = degraded_name
        .file_stem()
        .and_then(OsStr::to_str)
        .expect("valid stem");
    let degraded_extension = degraded_name
        .extension()
        .and_then(OsStr::to_str)
        .expect("valid extension");

    let spec = degraded.spec();

    let mut file_names: Vec<String> = vec![];

    for (i, chunk) in (&degraded.samples::<i16>().chunks(reference.len() as usize))
        .into_iter()
        .enumerate()
    {
        let output_name = format!("{}.{}.{}", degraded_stem, i, degraded_extension);
        let mut writer =
            hound::WavWriter::create(&format!("{}/{}", degraded_path, output_name), spec)?;

        for sample in chunk {
            if let Ok(sample) = sample {
                writer.write_sample(sample)?;
            } else {
                eprintln!("Error: sample was invalid for {}!", output_name);
                break;
            }
        }

        writer.finalize()?;
        file_names.push(output_name);
    }

    Ok(ChopAudioResult {
        file_names,
        reference_time_secs,
        degraded_time_secs,
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn chop_audio_and_analyze(
    degraded_path: &str,
    degraded_file: &str,
    ref_path: &str,
    ref_file: &str,
    extension: &str,
    client_name: &str,
    speech: bool,
    test_results: &mut TestResults,
) -> Result<()> {
    let chopped_audio_result = chop_audio(degraded_path, degraded_file, ref_path, ref_file)?;

    let mut data = StatsData::new_skip_n(0);
    data.set_period(chopped_audio_result.reference_time_secs as f32);

    for degraded_file in chopped_audio_result.file_names.iter() {
        analyze_audio(
            degraded_path,
            degraded_file,
            ref_path,
            ref_file,
            extension,
            speech,
        )
        .await?;

        if let Some(mos) = AnalysisReport::parse_audio_analysis(&format!(
            "{}/{}.{}",
            degraded_path, degraded_file, extension
        ))
        .await?
        {
            data.push(mos);
        } else {
            eprintln!("Error: mos value is missing for {}!", degraded_file);
            data.push(0f32);
        }
    }

    let (title, chart_name) = if speech {
        (
            format!(
                "MOS Speech Over Time ({}sec)",
                chopped_audio_result.reference_time_secs
            ),
            format!("{}.artifacts.mos_s.svg", client_name),
        )
    } else {
        (
            format!(
                "MOS Audio Over Time ({}sec)",
                chopped_audio_result.reference_time_secs
            ),
            format!("{}.artifacts.mos_a.svg", client_name),
        )
    };

    let stats = Stats {
        config: StatsConfig {
            title,
            chart_name,
            x_label: "Test Seconds".to_string(),
            y_label: "MOS".to_string(),
            x_max: Some(chopped_audio_result.degraded_time_secs as f32 + 5.0),
            y_max: Some(5.0),
            ..Default::default()
        },
        data,
    };

    if speech {
        test_results.mos_s = AnalysisReportMos::Series(Box::new(stats));
    } else {
        test_results.mos_a = AnalysisReportMos::Series(Box::new(stats));
    }

    Ok(())
}

pub async fn get_audio_and_analyze(
    degraded_path: &str,
    degraded_file: &str,
    ref_path: &str,
    ref_file: &str,
    extension: &str,
    speech: bool,
    test_results: &mut TestResults,
) -> Result<()> {
    analyze_audio(
        degraded_path,
        degraded_file,
        ref_path,
        ref_file,
        extension,
        speech,
    )
    .await?;

    if let Some(mos) = AnalysisReport::parse_audio_analysis(&format!(
        "{}/{}.{}",
        degraded_path, degraded_file, extension
    ))
    .await?
    {
        if speech {
            test_results.mos_s = AnalysisReportMos::Single(mos);
        } else {
            test_results.mos_a = AnalysisReportMos::Single(mos);
        }
    }

    Ok(())
}
