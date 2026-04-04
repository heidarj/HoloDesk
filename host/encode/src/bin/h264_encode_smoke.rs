#[cfg(windows)]
use std::path::PathBuf;

#[cfg(windows)]
use std::{
    env,
    fs::File,
    io::Write,
    time::{Duration, Instant},
};

#[cfg(windows)]
use holobridge_capture::{
    CaptureBackend, CaptureConfig, CaptureSession, CaptureTarget,
    CapturedFrame, DisplayId, DxgiCaptureBackend,
};
#[cfg(windows)]
use holobridge_encode::{
    recommended_bitrate_bps, MfH264Encoder, VideoEncoder,
    VideoEncoderConfig,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    #[cfg(not(windows))]
    {
        return Err(
            "h264 encode smoke is only supported on Windows".to_owned(),
        );
    }

    #[cfg(windows)]
    {
        let options = SmokeOptions::parse(env::args().skip(1))?;
        let backend = DxgiCaptureBackend::new().map_err(|error| error.to_string())?;
        let mut capture = backend
            .open(
                options.capture_target()?,
                CaptureConfig {
                    timeout_ms: options.timeout_ms,
                    target_fps_hint: Some(options.frame_rate_num),
                },
            )
            .map_err(|error| error.to_string())?;

        let display_info = capture.display_info().clone();
        let first_frame = wait_for_first_frame(
            capture.as_mut(),
            Duration::from_secs(options.first_frame_timeout_seconds),
        )?;
        let first_frame_metadata = first_frame.metadata();
        let bitrate_bps = options.bitrate_bps.unwrap_or_else(|| {
            recommended_bitrate_bps(
                first_frame_metadata.width,
                first_frame_metadata.height,
                options.frame_rate_num,
                options.frame_rate_den,
            )
        });
        let config = VideoEncoderConfig::new(
            first_frame_metadata.width,
            first_frame_metadata.height,
            bitrate_bps,
            options.frame_rate_num,
            options.frame_rate_den,
        );

        let mut encoder = {
            #[cfg(windows)]
            {
                MfH264Encoder::new(&capture.d3d11_device(), config)
                    .map_err(|error| error.to_string())?
            }
        };

        let mut file = File::create(&options.output).map_err(|error| {
            format!(
                "failed to create output file {}: {error}",
                options.output.display()
            )
        })?;

        let deadline =
            Instant::now() + Duration::from_secs(options.duration_seconds);
        let mut encoded_frames = 0u64;
        let mut keyframes = 0u64;
        let mut total_bytes = 0u64;
        let mut encode_latency_total = Duration::ZERO;
        let mut capture_timeouts = 0u64;
        let mut frame_width = first_frame_metadata.width;
        let mut frame_height = first_frame_metadata.height;

        let started = Instant::now();
        let access_units =
            encoder.encode(&first_frame).map_err(|error| error.to_string())?;
        encode_latency_total += started.elapsed();
        for access_unit in access_units {
            file.write_all(&access_unit.data).map_err(|error| {
                format!("failed to write output stream: {error}")
            })?;
            encoded_frames += 1;
            keyframes += u64::from(access_unit.is_keyframe);
            total_bytes += access_unit.data.len() as u64;
        }

        while Instant::now() < deadline {
            let Some(frame) = capture
                .acquire_frame()
                .map_err(|error| error.to_string())?
            else {
                capture_timeouts += 1;
                continue;
            };
            let metadata = frame.metadata();
            frame_width = metadata.width;
            frame_height = metadata.height;

            let started = Instant::now();
            let access_units =
                encoder.encode(&frame).map_err(|error| error.to_string())?;
            encode_latency_total += started.elapsed();

            for access_unit in access_units {
                file.write_all(&access_unit.data).map_err(|error| {
                    format!("failed to write output stream: {error}")
                })?;
                encoded_frames += 1;
                keyframes += u64::from(access_unit.is_keyframe);
                total_bytes += access_unit.data.len() as u64;
            }
        }

        for access_unit in
            encoder.flush().map_err(|error| error.to_string())?
        {
            file.write_all(&access_unit.data)
                .map_err(|error| format!("failed to write flush data: {error}"))?;
            encoded_frames += 1;
            keyframes += u64::from(access_unit.is_keyframe);
            total_bytes += access_unit.data.len() as u64;
        }

        let effective_duration_secs = options.duration_seconds.max(1) as f64;
        let average_latency_ms = if encoded_frames == 0 {
            0.0
        } else {
            encode_latency_total.as_secs_f64() * 1000.0
                / encoded_frames as f64
        };
        let effective_bitrate_bps =
            (total_bytes as f64 * 8.0 / effective_duration_secs) as u64;

        println!(
            "selected_display: {} ({})",
            display_info.output_name, display_info.id
        );
        println!(
            "display_bounds: {}x{}",
            display_info.desktop_bounds.width(),
            display_info.desktop_bounds.height()
        );
        println!("capture_frame_size: {frame_width}x{frame_height}");
        println!("output_file: {}", options.output.display());
        println!("encoded_frames: {encoded_frames}");
        println!("keyframes: {keyframes}");
        println!("capture_timeouts: {capture_timeouts}");
        println!("total_bytes: {total_bytes}");
        println!("average_encode_latency_ms: {average_latency_ms:.2}");
        println!("effective_bitrate_bps: {effective_bitrate_bps}");

        Ok(())
    }
}

#[cfg(windows)]
struct SmokeOptions {
    display_id: Option<String>,
    duration_seconds: u64,
    timeout_ms: u32,
    output: PathBuf,
    bitrate_bps: Option<u32>,
    frame_rate_num: u32,
    frame_rate_den: u32,
    first_frame_timeout_seconds: u64,
}

#[cfg(windows)]
impl SmokeOptions {
    fn parse(
        mut arguments: impl Iterator<Item = String>,
    ) -> Result<Self, String> {
        let mut options = Self {
            display_id: None,
            duration_seconds: 5,
            timeout_ms: 16,
            output: PathBuf::from("holobridge-smoke.h264"),
            bitrate_bps: None,
            frame_rate_num: 60,
            frame_rate_den: 1,
            first_frame_timeout_seconds: 2,
        };

        while let Some(argument) = arguments.next() {
            match argument.as_str() {
                "--display-id" => {
                    options.display_id = Some(
                        arguments
                            .next()
                            .ok_or_else(|| {
                                "--display-id requires a value".to_owned()
                            })?,
                    );
                }
                "--duration-seconds" => {
                    options.duration_seconds = arguments
                        .next()
                        .ok_or_else(|| {
                            "--duration-seconds requires a value".to_owned()
                        })?
                        .parse::<u64>()
                        .map_err(|error| {
                            format!(
                                "invalid --duration-seconds value: {error}"
                            )
                        })?;
                }
                "--timeout-ms" => {
                    options.timeout_ms = arguments
                        .next()
                        .ok_or_else(|| {
                            "--timeout-ms requires a value".to_owned()
                        })?
                        .parse::<u32>()
                        .map_err(|error| {
                            format!("invalid --timeout-ms value: {error}")
                        })?;
                }
                "--output" => {
                    options.output = PathBuf::from(
                        arguments.next().ok_or_else(|| {
                            "--output requires a value".to_owned()
                        })?,
                    );
                }
                "--bitrate-bps" => {
                    options.bitrate_bps = Some(
                        arguments
                            .next()
                            .ok_or_else(|| {
                                "--bitrate-bps requires a value".to_owned()
                            })?
                            .parse::<u32>()
                            .map_err(|error| {
                                format!("invalid --bitrate-bps value: {error}")
                            })?,
                    );
                }
                "--frame-rate" => {
                    let value = arguments.next().ok_or_else(|| {
                        "--frame-rate requires a value like 60/1".to_owned()
                    })?;
                    let (num, den) = value
                        .split_once('/')
                        .ok_or_else(|| {
                            "--frame-rate must be formatted as <num>/<den>"
                                .to_owned()
                        })?;
                    options.frame_rate_num =
                        num.parse::<u32>().map_err(|error| {
                            format!("invalid frame-rate numerator: {error}")
                        })?;
                    options.frame_rate_den =
                        den.parse::<u32>().map_err(|error| {
                            format!("invalid frame-rate denominator: {error}")
                        })?;
                }
                "--first-frame-timeout-seconds" => {
                    options.first_frame_timeout_seconds = arguments
                        .next()
                        .ok_or_else(|| {
                            "--first-frame-timeout-seconds requires a value"
                                .to_owned()
                        })?
                        .parse::<u64>()
                        .map_err(|error| {
                            format!(
                                "invalid --first-frame-timeout-seconds value: {error}"
                            )
                        })?;
                }
                "--help" | "-h" => {
                    return Err(Self::usage().to_owned());
                }
                other => {
                    return Err(format!(
                        "unknown argument: {other}\n{}",
                        Self::usage()
                    ));
                }
            }
        }

        Ok(options)
    }

    fn capture_target(&self) -> Result<CaptureTarget, String> {
        match &self.display_id {
            Some(display_id) => {
                let display_id = display_id.parse::<DisplayId>().map_err(|e| {
                    format!("invalid --display-id value: {e}")
                })?;
                Ok(CaptureTarget::Display(display_id))
            }
            None => Ok(CaptureTarget::Primary),
        }
    }

    fn usage() -> &'static str {
        "usage: h264_encode_smoke [--display-id <adapter_luid:output_index>] [--duration-seconds <n>] [--timeout-ms <n>] [--output <path>] [--bitrate-bps <n>] [--frame-rate <num/den>] [--first-frame-timeout-seconds <n>]"
    }
}

#[cfg(windows)]
fn wait_for_first_frame(
    capture: &mut dyn CaptureSession,
    timeout: Duration,
) -> Result<CapturedFrame, String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(frame) = capture
            .acquire_frame()
            .map_err(|error| error.to_string())?
        {
            return Ok(frame);
        }
    }

    Err(format!(
        "timed out waiting {:?} for the first captured frame; keep the Windows desktop active",
        timeout
    ))
}
