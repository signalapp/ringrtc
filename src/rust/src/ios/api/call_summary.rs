//
// Copyright 2025 Signal Messenger, LLC
// SPDX-License-Identifier: AGPL-3.0-only
//

use std::ffi::{CString, c_char};

use crate::{core::call_summary::CallSummary, lite::ffi::ios::rtc_OptionalF32};

#[repr(C)]
#[derive(Debug, Default)]
#[allow(non_camel_case_types)]
pub struct rtc_callsummary_CallSummary {
    pub start_time: u64,
    pub end_time: u64,
    pub rtt_median_connection: rtc_OptionalF32,
    pub audio_rtt_median_media: rtc_OptionalF32,
    pub audio_jitter_median_send: rtc_OptionalF32,
    pub audio_jitter_median_recv: rtc_OptionalF32,
    pub audio_packet_loss_fraction_send: rtc_OptionalF32,
    pub audio_packet_loss_fraction_recv: rtc_OptionalF32,
    pub video_rtt_median_media: rtc_OptionalF32,
    pub video_jitter_median_send: rtc_OptionalF32,
    pub video_jitter_median_recv: rtc_OptionalF32,
    pub video_packet_loss_fraction_send: rtc_OptionalF32,
    pub video_packet_loss_fraction_recv: rtc_OptionalF32,
    pub raw_stats: *const u8,
    pub raw_stats_len: usize,
    pub raw_stats_text: *const c_char,
    pub raw_call_end_reason_text: *const c_char,
    pub is_survey_candidate: bool,
}

impl rtc_callsummary_CallSummary {
    pub fn release(self) {
        if !self.raw_stats_text.is_null() {
            let _ = unsafe { CString::from_raw(self.raw_stats_text as *mut _) };
        }
        if !self.raw_call_end_reason_text.is_null() {
            let _ = unsafe { CString::from_raw(self.raw_call_end_reason_text as *mut _) };
        }
    }

    pub fn wrap(summary: &CallSummary) -> Self {
        let (raw_stats, raw_stats_len) = match summary.raw_stats.as_ref() {
            Some(raw_stats) => (raw_stats.as_ptr(), raw_stats.len()),
            _ => (std::ptr::null(), 0),
        };

        let raw_stats_text = summary
            .raw_stats_text
            .as_ref()
            .map(|text| CString::new(text.clone()).unwrap_or_default().into_raw() as *const _)
            .unwrap_or(std::ptr::null());

        let raw_call_end_reason_text = CString::new(summary.call_end_reason_text.clone())
            .unwrap_or_default()
            .into_raw() as *const _;

        let mut wrapped_summary = Self {
            start_time: summary.start_time.into(),
            end_time: summary.end_time.into(),
            raw_call_end_reason_text,
            is_survey_candidate: summary.is_survey_candidate,
            raw_stats,
            raw_stats_len,
            raw_stats_text,
            ..Default::default()
        };

        wrapped_summary.rtt_median_connection = summary.quality_stats.rtt_median_connection.into();
        wrapped_summary.audio_rtt_median_media =
            summary.quality_stats.audio_stats.rtt_median.into();
        wrapped_summary.audio_jitter_median_send =
            summary.quality_stats.audio_stats.jitter_median_send.into();
        wrapped_summary.audio_jitter_median_recv =
            summary.quality_stats.audio_stats.jitter_median_recv.into();
        wrapped_summary.audio_packet_loss_fraction_send = summary
            .quality_stats
            .audio_stats
            .packet_loss_fraction_send
            .into();
        wrapped_summary.audio_packet_loss_fraction_recv = summary
            .quality_stats
            .audio_stats
            .packet_loss_fraction_recv
            .into();
        wrapped_summary.video_rtt_median_media =
            summary.quality_stats.video_stats.rtt_median.into();
        wrapped_summary.video_jitter_median_send =
            summary.quality_stats.video_stats.jitter_median_send.into();
        wrapped_summary.video_jitter_median_recv =
            summary.quality_stats.video_stats.jitter_median_recv.into();
        wrapped_summary.video_packet_loss_fraction_send = summary
            .quality_stats
            .video_stats
            .packet_loss_fraction_send
            .into();
        wrapped_summary.video_packet_loss_fraction_recv = summary
            .quality_stats
            .video_stats
            .packet_loss_fraction_recv
            .into();

        wrapped_summary
    }
}
