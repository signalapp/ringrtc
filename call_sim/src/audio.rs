//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use anyhow::Result;
use itertools::Itertools;
use std::{ffi::OsStr, path::Path};

use crate::common::AudioConfig;
use crate::docker::{analyze_pesq_mos, analyze_plc_mos, analyze_visqol_mos};
use crate::report::{AnalysisReport, AnalysisReportMos, Stats, StatsConfig, StatsData};
use crate::test::AudioTestResults;

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

pub struct AudioFiles<'a> {
    pub degraded_path: &'a str,
    pub degraded_file: &'a str,
    pub ref_path: &'a str,
    pub ref_file: &'a str,
}

/// This function chops a long audio file into smaller segments equal in length to the reference
/// file and then analyzes the files to generate MOS values for each segment. Note: If the
/// segments don't correlate to the reference, for example if the degraded files have a large delay,
/// then the results may not be useful.
pub async fn chop_audio_and_analyze(
    audio_files: &AudioFiles<'_>,
    speech_files: &AudioFiles<'_>,
    client_name: &str,
    audio_config: &AudioConfig,
    test_results: &mut AudioTestResults,
) -> Result<()> {
    if audio_config.visqol_audio_analysis {
        let extension = format!("{}.visqol_mos_audio.log", client_name);
        let chopped_result = chop_audio(
            audio_files.degraded_path,
            audio_files.degraded_file,
            audio_files.ref_path,
            audio_files.ref_file,
        )?;

        let mut data = StatsData::new_skip_n(0);
        data.set_period(chopped_result.reference_time_secs as f32);

        for degraded_file in chopped_result.file_names.iter() {
            analyze_visqol_mos(
                audio_files.degraded_path,
                degraded_file,
                audio_files.ref_path,
                audio_files.ref_file,
                &extension,
                false,
            )
            .await?;

            if let Some(mos) = AnalysisReport::parse_visqol_mos_results(&format!(
                "{}/{}.{}",
                audio_files.degraded_path, degraded_file, &extension
            ))
            .await?
            {
                data.push(mos);
            } else {
                eprintln!("Error: mos value is missing for {}!", degraded_file);
                data.push(0f32);
            }
        }

        let stats = Stats {
            config: StatsConfig {
                title: format!(
                    "Visqol MOS Audio Over Time ({}sec)",
                    chopped_result.reference_time_secs
                ),
                chart_name: format!("{}.artifacts.visqol_mos_audio.svg", client_name),
                x_label: "Test Seconds".to_string(),
                y_label: "MOS".to_string(),
                x_max: Some(chopped_result.degraded_time_secs as f32 + 5.0),
                y_max: Some(5.0),
                ..Default::default()
            },
            data,
        };

        test_results.visqol_mos_audio = AnalysisReportMos::Series(Box::new(stats));
    }

    if audio_config.requires_speech() {
        let chopped_result = chop_audio(
            speech_files.degraded_path,
            speech_files.degraded_file,
            speech_files.ref_path,
            speech_files.ref_file,
        )?;

        if audio_config.visqol_speech_analysis {
            let extension = format!("{}.visqol_mos_speech.log", client_name);

            let mut data = StatsData::new_skip_n(0);
            data.set_period(chopped_result.reference_time_secs as f32);

            for degraded_file in chopped_result.file_names.iter() {
                analyze_visqol_mos(
                    speech_files.degraded_path,
                    degraded_file,
                    speech_files.ref_path,
                    speech_files.ref_file,
                    &extension,
                    false,
                )
                .await?;

                if let Some(mos) = AnalysisReport::parse_visqol_mos_results(&format!(
                    "{}/{}.{}",
                    speech_files.degraded_path, degraded_file, &extension
                ))
                .await?
                {
                    data.push(mos);
                } else {
                    eprintln!("Error: mos value is missing for {}!", degraded_file);
                    data.push(0f32);
                }
            }

            let stats = Stats {
                config: StatsConfig {
                    title: format!(
                        "Visqol MOS Speech Over Time ({}sec)",
                        chopped_result.reference_time_secs
                    ),
                    chart_name: format!("{}.artifacts.visqol_mos_speech.svg", client_name),
                    x_label: "Test Seconds".to_string(),
                    y_label: "MOS".to_string(),
                    x_max: Some(chopped_result.degraded_time_secs as f32 + 5.0),
                    y_max: Some(5.0),
                    ..Default::default()
                },
                data,
            };

            test_results.visqol_mos_speech = AnalysisReportMos::Series(Box::new(stats));

            // TODO: Compute the average visqol series values.
        }

        if audio_config.pesq_speech_analysis {
            let extension = format!("{}.pesq_mos.log", client_name);

            let mut data = StatsData::new_skip_n(0);
            data.set_period(chopped_result.reference_time_secs as f32);

            for degraded_file in chopped_result.file_names.iter() {
                analyze_pesq_mos(
                    speech_files.degraded_path,
                    degraded_file,
                    speech_files.ref_path,
                    speech_files.ref_file,
                    &extension,
                )
                .await?;

                if let Some(mos) = AnalysisReport::parse_pesq_mos_results(&format!(
                    "{}/{}.{}",
                    speech_files.degraded_path, degraded_file, &extension
                ))
                .await?
                {
                    data.push(mos);
                } else {
                    eprintln!("Error: mos value is missing for {}!", degraded_file);
                    data.push(0f32);
                }
            }

            let stats = Stats {
                config: StatsConfig {
                    title: format!(
                        "PESQ MOS Over Time ({}sec)",
                        chopped_result.reference_time_secs
                    ),
                    chart_name: format!("{}.artifacts.pesq_mos.svg", client_name),
                    x_label: "Test Seconds".to_string(),
                    y_label: "MOS".to_string(),
                    x_max: Some(chopped_result.degraded_time_secs as f32 + 5.0),
                    y_max: Some(5.0),
                    ..Default::default()
                },
                data,
            };

            test_results.pesq_mos = AnalysisReportMos::Series(Box::new(stats));
        }

        if audio_config.plc_speech_analysis {
            let extension = format!("{}.plc_mos.log", client_name);

            let mut data = StatsData::new_skip_n(0);
            data.set_period(chopped_result.reference_time_secs as f32);

            for degraded_file in chopped_result.file_names.iter() {
                analyze_plc_mos(speech_files.degraded_path, degraded_file, &extension).await?;

                if let Some(mos) = AnalysisReport::parse_plc_mos_results(&format!(
                    "{}/{}.{}",
                    speech_files.degraded_path, degraded_file, &extension
                ))
                .await?
                {
                    data.push(mos);
                } else {
                    eprintln!("Error: mos value is missing for {}!", degraded_file);
                    data.push(0f32);
                }
            }

            let stats = Stats {
                config: StatsConfig {
                    title: format!(
                        "PLC MOS Over Time ({}sec)",
                        chopped_result.reference_time_secs
                    ),
                    chart_name: format!("{}.artifacts.plc_mos.svg", client_name),
                    x_label: "Test Seconds".to_string(),
                    y_label: "MOS".to_string(),
                    x_max: Some(chopped_result.degraded_time_secs as f32 + 5.0),
                    y_max: Some(5.0),
                    ..Default::default()
                },
                data,
            };

            test_results.plc_mos = AnalysisReportMos::Series(Box::new(stats));
        }
    }

    Ok(())
}

pub async fn get_audio_and_analyze(
    audio_files: &AudioFiles<'_>,
    speech_files: &AudioFiles<'_>,
    client_name: &str,
    audio_config: &AudioConfig,
    test_results: &mut AudioTestResults,
) -> Result<()> {
    if audio_config.visqol_audio_analysis {
        let extension = format!("{}.visqol_mos_audio.log", client_name);

        analyze_visqol_mos(
            audio_files.degraded_path,
            audio_files.degraded_file,
            audio_files.ref_path,
            audio_files.ref_file,
            &extension,
            false,
        )
        .await?;

        if let Some(mos) = AnalysisReport::parse_visqol_mos_results(&format!(
            "{}/{}.{}",
            audio_files.degraded_path, audio_files.degraded_file, &extension
        ))
        .await?
        {
            test_results.visqol_mos_audio = AnalysisReportMos::Single(mos);
        }
    }

    if audio_config.visqol_speech_analysis {
        let extension = format!("{}.visqol_mos_speech.log", client_name);

        analyze_visqol_mos(
            speech_files.degraded_path,
            speech_files.degraded_file,
            speech_files.ref_path,
            speech_files.ref_file,
            &extension,
            true,
        )
        .await?;

        if let Some(mos) = AnalysisReport::parse_visqol_mos_results(&format!(
            "{}/{}.{}",
            speech_files.degraded_path, speech_files.degraded_file, extension
        ))
        .await?
        {
            test_results.visqol_mos_speech = AnalysisReportMos::Single(mos);

            // If we also have a visqol mos audio result, compute the average.
            if let AnalysisReportMos::Single(audio_mos) = test_results.visqol_mos_audio {
                test_results.visqol_mos_average =
                    AnalysisReportMos::Single((mos + audio_mos) / 2.0);
            }
        }
    }

    if audio_config.pesq_speech_analysis {
        let extension = format!("{}.pesq_mos.log", client_name);

        analyze_pesq_mos(
            speech_files.degraded_path,
            speech_files.degraded_file,
            speech_files.ref_path,
            speech_files.ref_file,
            &extension,
        )
        .await?;

        if let Some(mos) = AnalysisReport::parse_pesq_mos_results(&format!(
            "{}/{}.{}",
            speech_files.degraded_path, speech_files.degraded_file, extension
        ))
        .await?
        {
            test_results.pesq_mos = AnalysisReportMos::Single(mos);
        }
    }

    if audio_config.plc_speech_analysis {
        let extension = format!("{}.plc_mos.log", client_name);

        analyze_plc_mos(
            speech_files.degraded_path,
            speech_files.degraded_file,
            &extension,
        )
        .await?;

        if let Some(mos) = AnalysisReport::parse_plc_mos_results(&format!(
            "{}/{}.{}",
            speech_files.degraded_path, speech_files.degraded_file, extension
        ))
        .await?
        {
            test_results.plc_mos = AnalysisReportMos::Single(mos);
        }
    }

    Ok(())
}
