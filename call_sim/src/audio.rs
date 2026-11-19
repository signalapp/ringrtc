//
// Copyright 2023 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::{ffi::OsStr, path::Path};

use anyhow::Result;
use futures_util::{FutureExt, StreamExt, TryStreamExt, stream};
use itertools::Itertools;
use log::{error, info};

use crate::{
    common::AudioConfig,
    docker::{analyze_pesq_mos, analyze_plc_mos, analyze_visqol_mos},
    report::{AnalysisReport, AnalysisReportMos, Stats, StatsConfig, StatsData},
    test::AudioTestResults,
};

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
    info!("Chopping audio for `{}`:", degraded_file);

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
            hound::WavWriter::create(format!("{}/{}", degraded_path, output_name), spec)?;

        for sample in chunk {
            if let Ok(sample) = sample {
                writer.write_sample(sample)?;
            } else {
                error!("Error: sample was invalid for {}!", output_name);
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

/// Analyzes the chopped segments and returns one MOS per segment, in segment order.
async fn analyze_segments<F>(
    file_names: &[String],
    concurrency: u16,
    analyze: F,
) -> Result<Vec<f32>>
where
    F: AsyncFn(&str) -> Result<Option<f32>>,
{
    stream::iter(file_names.iter().map(String::as_str))
        .map(|degraded_file| {
            let analysis = analyze(degraded_file);
            async move {
                Ok(analysis.await?.unwrap_or_else(|| {
                    error!("Error: mos value is missing for {}!", degraded_file);
                    0f32
                }))
            }
        })
        .buffered(concurrency as usize)
        .try_collect()
        .await
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
    analysis_concurrency: u16,
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

        for mos in analyze_segments(
            &chopped_result.file_names,
            analysis_concurrency,
            async |degraded_file| {
                analyze_visqol_mos(
                    audio_files.degraded_path,
                    degraded_file,
                    audio_files.ref_path,
                    audio_files.ref_file,
                    &extension,
                    false,
                )
                .await?;

                AnalysisReport::parse_visqol_mos_results(&format!(
                    "{}/{}.{}",
                    audio_files.degraded_path, degraded_file, extension
                ))
                .await
            },
        )
        .await?
        {
            data.push(mos);
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

            for mos in analyze_segments(
                &chopped_result.file_names,
                analysis_concurrency,
                async |degraded_file| {
                    analyze_visqol_mos(
                        speech_files.degraded_path,
                        degraded_file,
                        speech_files.ref_path,
                        speech_files.ref_file,
                        &extension,
                        true,
                    )
                    .await?;

                    AnalysisReport::parse_visqol_mos_results(&format!(
                        "{}/{}.{}",
                        speech_files.degraded_path, degraded_file, extension
                    ))
                    .await
                },
            )
            .await?
            {
                data.push(mos);
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
        }

        if audio_config.pesq_speech_analysis {
            let extension = format!("{}.pesq_mos.log", client_name);

            let mut data = StatsData::new_skip_n(0);
            data.set_period(chopped_result.reference_time_secs as f32);

            for mos in analyze_segments(
                &chopped_result.file_names,
                analysis_concurrency,
                async |degraded_file| {
                    analyze_pesq_mos(
                        speech_files.degraded_path,
                        degraded_file,
                        speech_files.ref_path,
                        speech_files.ref_file,
                        &extension,
                    )
                    .await?;

                    AnalysisReport::parse_pesq_mos_results(&format!(
                        "{}/{}.{}",
                        speech_files.degraded_path, degraded_file, extension
                    ))
                    .await
                },
            )
            .await?
            {
                data.push(mos);
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

            for mos in analyze_segments(
                &chopped_result.file_names,
                analysis_concurrency,
                async |degraded_file| {
                    analyze_plc_mos(speech_files.degraded_path, degraded_file, &extension).await?;

                    AnalysisReport::parse_plc_mos_results(&format!(
                        "{}/{}.{}",
                        speech_files.degraded_path, degraded_file, extension
                    ))
                    .await
                },
            )
            .await?
            {
                data.push(mos);
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

        calculate_average_mos_series(test_results, &chopped_result, client_name);
    }

    Ok(())
}

fn calculate_average_mos_series(
    test_results: &mut AudioTestResults,
    chopped_result: &ChopAudioResult,
    client_name: &str,
) {
    let mut series_stats: Vec<&Stats> = Vec::new();

    if let AnalysisReportMos::Series(stats) = &test_results.visqol_mos_audio {
        series_stats.push(stats);
    }
    if let AnalysisReportMos::Series(stats) = &test_results.visqol_mos_speech {
        series_stats.push(stats);
    }
    if let AnalysisReportMos::Series(stats) = &test_results.pesq_mos {
        series_stats.push(stats);
    }
    if let AnalysisReportMos::Series(stats) = &test_results.plc_mos {
        series_stats.push(stats);
    }

    if series_stats.is_empty() {
        return;
    }

    let mut data = StatsData::new_skip_n(0);
    data.set_period(chopped_result.reference_time_secs as f32);

    for i in 0..chopped_result.file_names.len() {
        let mut sum = 0.0f32;

        // For each chopped file, get the average of the MOS values across all series.
        for stats in &series_stats {
            sum += stats.data.points[i].1;
        }

        data.push(sum / series_stats.len() as f32);
    }

    let stats = Stats {
        config: StatsConfig {
            title: format!(
                "Average MOS Over Time ({}sec)",
                chopped_result.reference_time_secs
            ),
            chart_name: format!("{}.artifacts.mos_average.svg", client_name),
            x_label: "Test Seconds".to_string(),
            y_label: "MOS".to_string(),
            x_max: Some(chopped_result.degraded_time_secs as f32 + 5.0),
            y_max: Some(5.0),
            ..Default::default()
        },
        data,
    };

    test_results.mos_average = AnalysisReportMos::Series(Box::new(stats));
}

pub async fn get_audio_and_analyze(
    audio_files: &AudioFiles<'_>,
    speech_files: &AudioFiles<'_>,
    client_name: &str,
    audio_config: &AudioConfig,
    analysis_concurrency: u16,
    test_results: &mut AudioTestResults,
) -> Result<()> {
    let analyses = vec![
        async {
            if !audio_config.visqol_audio_analysis {
                return Ok(None);
            }

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

            AnalysisReport::parse_visqol_mos_results(&format!(
                "{}/{}.{}",
                audio_files.degraded_path, audio_files.degraded_file, extension
            ))
            .await
        }
        .boxed(),
        async {
            if !audio_config.visqol_speech_analysis {
                return Ok(None);
            }

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

            AnalysisReport::parse_visqol_mos_results(&format!(
                "{}/{}.{}",
                speech_files.degraded_path, speech_files.degraded_file, extension
            ))
            .await
        }
        .boxed(),
        async {
            if !audio_config.pesq_speech_analysis {
                return Ok(None);
            }

            let extension = format!("{}.pesq_mos.log", client_name);

            analyze_pesq_mos(
                speech_files.degraded_path,
                speech_files.degraded_file,
                speech_files.ref_path,
                speech_files.ref_file,
                &extension,
            )
            .await?;

            AnalysisReport::parse_pesq_mos_results(&format!(
                "{}/{}.{}",
                speech_files.degraded_path, speech_files.degraded_file, extension
            ))
            .await
        }
        .boxed(),
        async {
            if !audio_config.plc_speech_analysis {
                return Ok(None);
            }

            let extension = format!("{}.plc_mos.log", client_name);

            analyze_plc_mos(
                speech_files.degraded_path,
                speech_files.degraded_file,
                &extension,
            )
            .await?;

            AnalysisReport::parse_plc_mos_results(&format!(
                "{}/{}.{}",
                speech_files.degraded_path, speech_files.degraded_file, extension
            ))
            .await
        }
        .boxed(),
    ];

    let results: Vec<_> = stream::iter(analyses)
        .buffered(analysis_concurrency as usize)
        .try_collect()
        .await?;

    for (mos, result) in results.into_iter().zip([
        &mut test_results.visqol_mos_audio,
        &mut test_results.visqol_mos_speech,
        &mut test_results.pesq_mos,
        &mut test_results.plc_mos,
    ]) {
        if let Some(mos) = mos {
            *result = AnalysisReportMos::Single(mos);
        }
    }

    calculate_average_mos(test_results);

    Ok(())
}

fn calculate_average_mos(test_results: &mut AudioTestResults) {
    let mut mos_values = Vec::new();

    if let AnalysisReportMos::Single(mos) = test_results.visqol_mos_audio {
        mos_values.push(mos);
    }
    if let AnalysisReportMos::Single(mos) = test_results.visqol_mos_speech {
        mos_values.push(mos);
    }
    if let AnalysisReportMos::Single(mos) = test_results.pesq_mos {
        mos_values.push(mos);
    }
    if let AnalysisReportMos::Single(mos) = test_results.plc_mos {
        mos_values.push(mos);
    }

    if !mos_values.is_empty() {
        let average = mos_values.iter().sum::<f32>() / mos_values.len() as f32;
        test_results.mos_average = AnalysisReportMos::Single(average);
    }
}
