use std::{
    env,
    process::ExitCode,
    time::{Duration, Instant},
};

use holobridge_capture::{
    CaptureBackend, CaptureConfig, CaptureTarget, DisplayId, DxgiCaptureBackend,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args = SmokeArgs::parse(env::args().skip(1))?;
    let backend = DxgiCaptureBackend::new().map_err(|error| error.to_string())?;
    let displays = backend
        .enumerate_displays()
        .map_err(|error| error.to_string())?;

    println!("Enumerated displays:");
    for display in &displays {
        let primary_suffix = if display.is_primary { " [primary]" } else { "" };
        println!(
            "- {}{} adapter=\"{}\" output=\"{}\" bounds={} rotation={}",
            display.id,
            primary_suffix,
            display.adapter_name,
            display.output_name,
            display.desktop_bounds,
            display.rotation,
        );
    }

    if args.list_only {
        return Ok(());
    }

    let target = args
        .display_id
        .map(CaptureTarget::Display)
        .unwrap_or(CaptureTarget::Primary);
    let mut session = backend
        .open(
            target.clone(),
            CaptureConfig {
                timeout_ms: args.timeout_ms,
                target_fps_hint: Some(60),
            },
        )
        .map_err(|error| error.to_string())?;

    println!(
        "Selected display: {} ({})",
        session.display_info().id,
        session.display_info().output_name
    );

    let deadline = Instant::now() + Duration::from_secs(args.duration_seconds);
    let mut captured_frames = 0u64;
    let mut timeouts = 0u64;
    let mut last_dimensions = None;
    let mut prior_frame_at = None;
    let mut cadence_total = Duration::ZERO;
    let mut cadence_samples = 0u64;

    while Instant::now() < deadline {
        match session.acquire_frame().map_err(|error| error.to_string())? {
            Some(frame) => {
                captured_frames += 1;
                last_dimensions = Some((frame.metadata().width, frame.metadata().height));

                let frame_at = Instant::now();
                if let Some(previous) = prior_frame_at.replace(frame_at) {
                    cadence_total += frame_at.saturating_duration_since(previous);
                    cadence_samples += 1;
                }
            }
            None => {
                timeouts += 1;
            }
        }
    }

    println!("Capture summary:");
    println!("- target: {target}");
    println!("- duration_seconds: {}", args.duration_seconds);
    println!("- timeout_ms: {}", args.timeout_ms);
    println!("- captured_frames: {captured_frames}");
    println!("- timeouts: {timeouts}");
    match last_dimensions {
        Some((width, height)) => println!("- last_frame_size: {}x{}", width, height),
        None => println!("- last_frame_size: none"),
    }
    if cadence_samples > 0 {
        let average_ms = cadence_total.as_secs_f64() * 1000.0 / cadence_samples as f64;
        println!("- average_cadence_ms: {:.2}", average_ms);
    } else {
        println!("- average_cadence_ms: n/a");
    }

    Ok(())
}

struct SmokeArgs {
    list_only: bool,
    display_id: Option<DisplayId>,
    duration_seconds: u64,
    timeout_ms: u32,
}

impl SmokeArgs {
    fn parse<I>(args: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = String>,
    {
        let mut parsed = Self {
            list_only: false,
            display_id: None,
            duration_seconds: 3,
            timeout_ms: 16,
        };

        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--list" => parsed.list_only = true,
                "--display-id" => {
                    let value = args.next().ok_or_else(Self::usage)?;
                    parsed.display_id = Some(value.parse().map_err(
                        |error: holobridge_capture::DisplayIdParseError| error.to_string(),
                    )?);
                }
                "--duration-seconds" => {
                    let value = args.next().ok_or_else(Self::usage)?;
                    parsed.duration_seconds = value.parse::<u64>().map_err(|_| Self::usage())?;
                }
                "--timeout-ms" => {
                    let value = args.next().ok_or_else(Self::usage)?;
                    parsed.timeout_ms = value.parse::<u32>().map_err(|_| Self::usage())?;
                }
                _ => return Err(Self::usage()),
            }
        }

        Ok(parsed)
    }

    fn usage() -> String {
        "usage: dxgi_capture_smoke [--list] [--display-id <adapter_luid:output_index>] [--duration-seconds <n>] [--timeout-ms <n>]".to_owned()
    }
}
