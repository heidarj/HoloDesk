#![cfg(windows)]

use std::time::{Duration, Instant};

use holobridge_capture::{
    CaptureBackend, CaptureConfig, CaptureTarget, DxgiCaptureBackend,
};
use holobridge_encode::{
    recommended_bitrate_bps, MfH264Encoder, VideoEncoder,
    VideoEncoderConfig,
};

#[test]
#[ignore = "requires a real Windows console session with an active display"]
fn hardware_encoder_can_be_created_for_primary_display() {
    let backend = DxgiCaptureBackend::new().unwrap();
    let capture = backend
        .open(CaptureTarget::Primary, CaptureConfig::default())
        .unwrap();

    let display = capture.display_info().clone();
    let bitrate = recommended_bitrate_bps(
        display.desktop_bounds.width(),
        display.desktop_bounds.height(),
        60,
        1,
    );
    let config = VideoEncoderConfig::new(
        display.desktop_bounds.width(),
        display.desktop_bounds.height(),
        bitrate,
        60,
        1,
    );

    let _encoder = MfH264Encoder::new(&capture.d3d11_device(), config).unwrap();
}

#[test]
#[ignore = "requires a real Windows console session with desktop motion"]
fn short_capture_run_produces_at_least_one_access_unit() {
    let backend = DxgiCaptureBackend::new().unwrap();
    let mut capture = backend
        .open(CaptureTarget::Primary, CaptureConfig::default())
        .unwrap();
    let display = capture.display_info().clone();
    let bitrate = recommended_bitrate_bps(
        display.desktop_bounds.width(),
        display.desktop_bounds.height(),
        60,
        1,
    );
    let config = VideoEncoderConfig::new(
        display.desktop_bounds.width(),
        display.desktop_bounds.height(),
        bitrate,
        60,
        1,
    );
    let mut encoder = MfH264Encoder::new(&capture.d3d11_device(), config).unwrap();

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut encoded_frames = 0u32;

    while Instant::now() < deadline && encoded_frames == 0 {
        if let Some(frame) = capture.acquire_frame().unwrap() {
            encoded_frames += encoder.encode(&frame).unwrap().len() as u32;
        }
    }

    assert!(encoded_frames > 0);
}

#[test]
#[ignore = "requires a real Windows console session with sustained desktop motion"]
fn bounded_run_contains_a_keyframe() {
    let backend = DxgiCaptureBackend::new().unwrap();
    let mut capture = backend
        .open(CaptureTarget::Primary, CaptureConfig::default())
        .unwrap();
    let display = capture.display_info().clone();
    let bitrate = recommended_bitrate_bps(
        display.desktop_bounds.width(),
        display.desktop_bounds.height(),
        60,
        1,
    );
    let config = VideoEncoderConfig::new(
        display.desktop_bounds.width(),
        display.desktop_bounds.height(),
        bitrate,
        60,
        1,
    );
    let mut encoder = MfH264Encoder::new(&capture.d3d11_device(), config).unwrap();

    let deadline = Instant::now() + Duration::from_secs(4);
    let mut keyframes = 0u32;

    while Instant::now() < deadline && keyframes == 0 {
        if let Some(frame) = capture.acquire_frame().unwrap() {
            keyframes += encoder
                .encode(&frame)
                .unwrap()
                .into_iter()
                .filter(|access_unit| access_unit.is_keyframe)
                .count() as u32;
        }
    }

    assert!(keyframes > 0);
}
