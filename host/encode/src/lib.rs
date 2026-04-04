#![cfg_attr(not(windows), allow(dead_code))]

use std::{
    error::Error,
    fmt,
    time::Duration,
};

use holobridge_capture::CapturedFrame;

#[cfg(not(windows))]
mod stub_backend;
#[cfg(windows)]
mod windows_backend;

#[cfg(not(windows))]
pub use stub_backend::MfH264Encoder;
#[cfg(windows)]
pub use windows_backend::MfH264Encoder;

const HUNDRED_NANOS_PER_SECOND: i64 = 10_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum H264Profile {
    Main,
}

impl fmt::Display for H264Profile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Main => formatter.write_str("main"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoEncoderConfig {
    pub width: u32,
    pub height: u32,
    pub bitrate_bps: u32,
    pub frame_rate_num: u32,
    pub frame_rate_den: u32,
    pub profile: H264Profile,
    pub keyframe_interval: Duration,
    pub low_latency: bool,
}

impl VideoEncoderConfig {
    pub fn new(
        width: u32,
        height: u32,
        bitrate_bps: u32,
        frame_rate_num: u32,
        frame_rate_den: u32,
    ) -> Self {
        Self {
            width,
            height,
            bitrate_bps,
            frame_rate_num,
            frame_rate_den,
            profile: H264Profile::Main,
            keyframe_interval: Duration::from_secs(2),
            low_latency: true,
        }
    }

    pub fn validate(&self) -> Result<(), EncodeError> {
        if self.width == 0 {
            return Err(EncodeError::InvalidConfig(
                "width must be greater than zero",
            ));
        }
        if self.height == 0 {
            return Err(EncodeError::InvalidConfig(
                "height must be greater than zero",
            ));
        }
        if self.bitrate_bps == 0 {
            return Err(EncodeError::InvalidConfig(
                "bitrate_bps must be greater than zero",
            ));
        }
        if self.frame_rate_num == 0 || self.frame_rate_den == 0 {
            return Err(EncodeError::InvalidConfig(
                "frame rate numerator and denominator must be greater than zero",
            ));
        }
        if self.keyframe_interval.is_zero() {
            return Err(EncodeError::InvalidConfig(
                "keyframe_interval must be greater than zero",
            ));
        }
        let _ = self.gop_size()?;
        let _ = self.frame_duration_100ns()?;
        Ok(())
    }

    pub fn gop_size(&self) -> Result<u32, EncodeError> {
        let interval_100ns = duration_to_100ns(self.keyframe_interval)?;
        let numerator = i128::from(interval_100ns)
            * i128::from(self.frame_rate_num);
        let denominator = i128::from(HUNDRED_NANOS_PER_SECOND)
            * i128::from(self.frame_rate_den);
        let rounded = ((numerator + (denominator / 2)) / denominator)
            .max(1);
        u32::try_from(rounded).map_err(|_| {
            EncodeError::InvalidConfig(
                "calculated GOP size exceeds supported range",
            )
        })
    }

    pub fn frame_duration_100ns(&self) -> Result<i64, EncodeError> {
        let numerator = i128::from(HUNDRED_NANOS_PER_SECOND)
            * i128::from(self.frame_rate_den);
        let denominator = i128::from(self.frame_rate_num);
        let rounded = ((numerator + (denominator / 2)) / denominator)
            .max(1);
        i64::try_from(rounded).map_err(|_| {
            EncodeError::InvalidConfig(
                "frame duration exceeds supported range",
            )
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedAccessUnit {
    pub data: Vec<u8>,
    pub is_keyframe: bool,
    pub pts_100ns: i64,
    pub duration_100ns: i64,
}

pub trait VideoEncoder {
    fn encode(
        &mut self,
        frame: &CapturedFrame,
    ) -> Result<Vec<EncodedAccessUnit>, EncodeError>;

    fn flush(&mut self) -> Result<Vec<EncodedAccessUnit>, EncodeError>;
}

#[derive(Debug)]
pub enum EncodeError {
    UnsupportedPlatform,
    InvalidConfig(&'static str),
    HardwareEncoderUnavailable,
    MissingSequenceHeader,
    Bitstream(String),
    WindowsApi {
        operation: &'static str,
        code: i32,
        message: String,
    },
}

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("encoding is only supported on Windows")
            }
            Self::InvalidConfig(message) => formatter.write_str(message),
            Self::HardwareEncoderUnavailable => formatter.write_str(
                "no compatible hardware H.264 Media Foundation encoder was found",
            ),
            Self::MissingSequenceHeader => formatter.write_str(
                "the encoder did not provide an H.264 sequence header",
            ),
            Self::Bitstream(message) => formatter.write_str(message),
            Self::WindowsApi {
                operation,
                code,
                message,
            } => write!(
                formatter,
                "{operation} failed with 0x{code:08x}: {message}"
            ),
        }
    }
}

impl Error for EncodeError {}

pub fn recommended_bitrate_bps(
    width: u32,
    height: u32,
    frame_rate_num: u32,
    frame_rate_den: u32,
) -> u32 {
    if width == 0 || height == 0 || frame_rate_num == 0 || frame_rate_den == 0
    {
        return 12_000_000;
    }

    let pixels_per_second = (u128::from(width) * u128::from(height))
        .saturating_mul(u128::from(frame_rate_num))
        / u128::from(frame_rate_den);
    let estimate = (pixels_per_second / 12)
        .clamp(8_000_000u128, 35_000_000u128);
    estimate as u32
}

pub(crate) fn duration_to_100ns(
    duration: Duration,
) -> Result<i64, EncodeError> {
    let ticks = i128::from(duration.as_secs())
        .saturating_mul(i128::from(HUNDRED_NANOS_PER_SECOND))
        .saturating_add(i128::from(duration.subsec_nanos() / 100));
    i64::try_from(ticks).map_err(|_| {
        EncodeError::InvalidConfig(
            "duration exceeds supported 100ns timestamp range",
        )
    })
}

pub(crate) fn pack_ratio_u64(
    numerator: u32,
    denominator: u32,
) -> u64 {
    ((u64::from(numerator)) << 32) | u64::from(denominator)
}

pub(crate) fn avcc_sequence_header_to_annex_b(
    config: &[u8],
) -> Result<(usize, Vec<u8>), EncodeError> {
    if config.len() < 7 {
        return Err(EncodeError::Bitstream(
            "AVCDecoderConfigurationRecord is truncated".to_owned(),
        ));
    }
    if config[0] != 1 {
        return Err(EncodeError::Bitstream(
            "unexpected AVCDecoderConfigurationRecord version".to_owned(),
        ));
    }

    let nal_length_size = usize::from((config[4] & 0x03) + 1);
    let sps_count = usize::from(config[5] & 0x1f);
    let mut offset = 6usize;
    let mut annex_b = Vec::new();

    for _ in 0..sps_count {
        let length = read_u16(config, &mut offset)?;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| EncodeError::Bitstream(
                "AVC sequence header length overflow".to_owned(),
            ))?;
        let payload = config.get(offset..end).ok_or_else(|| {
            EncodeError::Bitstream(
                "AVC sequence header SPS is truncated".to_owned(),
            )
        })?;
        annex_b.extend_from_slice(&[0, 0, 0, 1]);
        annex_b.extend_from_slice(payload);
        offset = end;
    }

    let pps_count = usize::from(*config.get(offset).ok_or_else(|| {
        EncodeError::Bitstream(
            "AVC sequence header PPS count is truncated".to_owned(),
        )
    })?);
    offset += 1;

    for _ in 0..pps_count {
        let length = read_u16(config, &mut offset)?;
        let end = offset
            .checked_add(length)
            .ok_or_else(|| EncodeError::Bitstream(
                "AVC sequence header length overflow".to_owned(),
            ))?;
        let payload = config.get(offset..end).ok_or_else(|| {
            EncodeError::Bitstream(
                "AVC sequence header PPS is truncated".to_owned(),
            )
        })?;
        annex_b.extend_from_slice(&[0, 0, 0, 1]);
        annex_b.extend_from_slice(payload);
        offset = end;
    }

    Ok((nal_length_size, annex_b))
}

pub(crate) fn assemble_annex_b_access_unit(
    payload: &[u8],
    sequence_header_annex_b: Option<&[u8]>,
    nal_length_size: Option<usize>,
    is_keyframe: bool,
) -> Result<Vec<u8>, EncodeError> {
    let mut annex_b = if looks_like_annex_b(payload) {
        payload.to_vec()
    } else {
        length_prefixed_h264_to_annex_b(
            payload,
            nal_length_size.unwrap_or(4),
        )?
    };

    if is_keyframe {
        if let Some(sequence_header_annex_b) = sequence_header_annex_b {
            if !annex_b_starts_with_parameter_set(&annex_b) {
                let mut prefixed = sequence_header_annex_b.to_vec();
                prefixed.extend_from_slice(&annex_b);
                annex_b = prefixed;
            }
        }
    }

    Ok(annex_b)
}

pub(crate) fn looks_like_annex_b(payload: &[u8]) -> bool {
    payload.starts_with(&[0, 0, 1]) || payload.starts_with(&[0, 0, 0, 1])
}

pub(crate) fn length_prefixed_h264_to_annex_b(
    payload: &[u8],
    nal_length_size: usize,
) -> Result<Vec<u8>, EncodeError> {
    if !(1..=4).contains(&nal_length_size) {
        return Err(EncodeError::Bitstream(format!(
            "invalid H.264 NAL length field size: {nal_length_size}"
        )));
    }

    let mut offset = 0usize;
    let mut annex_b = Vec::new();
    while offset < payload.len() {
        if payload.len() - offset < nal_length_size {
            return Err(EncodeError::Bitstream(
                "NAL length prefix is truncated".to_owned(),
            ));
        }

        let mut nal_length = 0usize;
        for _ in 0..nal_length_size {
            nal_length = (nal_length << 8) | usize::from(payload[offset]);
            offset += 1;
        }

        if nal_length == 0 {
            continue;
        }

        let end = offset
            .checked_add(nal_length)
            .ok_or_else(|| {
                EncodeError::Bitstream(
                    "NAL length overflow while converting to Annex-B"
                        .to_owned(),
                )
            })?;
        let nal = payload.get(offset..end).ok_or_else(|| {
            EncodeError::Bitstream(
                "NAL payload is truncated".to_owned(),
            )
        })?;

        annex_b.extend_from_slice(&[0, 0, 0, 1]);
        annex_b.extend_from_slice(nal);
        offset = end;
    }

    if annex_b.is_empty() {
        return Err(EncodeError::Bitstream(
            "encoded sample did not contain any NAL units".to_owned(),
        ));
    }

    Ok(annex_b)
}

fn annex_b_starts_with_parameter_set(payload: &[u8]) -> bool {
    let Some(nal_type) = first_annex_b_nal_type(payload) else {
        return false;
    };
    matches!(nal_type, 7 | 8)
}

fn first_annex_b_nal_type(payload: &[u8]) -> Option<u8> {
    let mut offset = 0usize;
    while offset + 3 < payload.len() {
        if payload[offset..].starts_with(&[0, 0, 1]) {
            return payload.get(offset + 3).map(|value| value & 0x1f);
        }
        if payload[offset..].starts_with(&[0, 0, 0, 1]) {
            return payload.get(offset + 4).map(|value| value & 0x1f);
        }
        offset += 1;
    }
    None
}

fn read_u16(
    value: &[u8],
    offset: &mut usize,
) -> Result<usize, EncodeError> {
    let bytes = value
        .get(*offset..(*offset + 2))
        .ok_or_else(|| {
            EncodeError::Bitstream(
                "truncated 16-bit value in H.264 configuration".to_owned(),
            )
        })?;
    *offset += 2;
    Ok(u16::from_be_bytes([bytes[0], bytes[1]]) as usize)
}

#[cfg(test)]
mod tests {
    use super::{
        assemble_annex_b_access_unit, avcc_sequence_header_to_annex_b,
        length_prefixed_h264_to_annex_b, recommended_bitrate_bps,
        H264Profile, VideoEncoderConfig,
    };
    use std::time::Duration;

    #[test]
    fn config_validation_rejects_zero_dimensions() {
        let config = VideoEncoderConfig::new(0, 1080, 8_000_000, 60, 1);
        assert!(config.validate().is_err());
    }

    #[test]
    fn gop_size_matches_two_second_interval_at_sixty_fps() {
        let mut config =
            VideoEncoderConfig::new(1920, 1080, 12_000_000, 60, 1);
        config.profile = H264Profile::Main;
        config.keyframe_interval = Duration::from_secs(2);

        assert_eq!(config.gop_size().unwrap(), 120);
        assert_eq!(config.frame_duration_100ns().unwrap(), 166_667);
    }

    #[test]
    fn avcc_helpers_produce_annex_b_keyframes() {
        let sequence_header = [
            0x01, 0x4d, 0x40, 0x1f, 0xff, 0xe1, 0x00, 0x04, 0x67, 0x4d,
            0x40, 0x1f, 0x01, 0x00, 0x03, 0x68, 0xee, 0x06,
        ];
        let (nal_length_size, annex_b_header) =
            avcc_sequence_header_to_annex_b(&sequence_header).unwrap();
        assert_eq!(nal_length_size, 4);
        assert_eq!(
            annex_b_header,
            vec![
                0, 0, 0, 1, 0x67, 0x4d, 0x40, 0x1f, 0, 0, 0, 1, 0x68,
                0xee, 0x06,
            ]
        );

        let payload = [0, 0, 0, 2, 0x65, 0x88];
        assert_eq!(
            length_prefixed_h264_to_annex_b(&payload, nal_length_size)
                .unwrap(),
            vec![0, 0, 0, 1, 0x65, 0x88]
        );

        let access_unit = assemble_annex_b_access_unit(
            &payload,
            Some(&annex_b_header),
            Some(nal_length_size),
            true,
        )
        .unwrap();
        assert_eq!(
            access_unit,
            vec![
                0, 0, 0, 1, 0x67, 0x4d, 0x40, 0x1f, 0, 0, 0, 1, 0x68,
                0xee, 0x06, 0, 0, 0, 1, 0x65, 0x88,
            ]
        );
    }

    #[test]
    fn bitrate_recommendation_scales_with_resolution() {
        let bitrate_1080p = recommended_bitrate_bps(1920, 1080, 60, 1);
        let bitrate_4k = recommended_bitrate_bps(3840, 2160, 60, 1);
        assert!(bitrate_1080p >= 8_000_000);
        assert!(bitrate_4k > bitrate_1080p);
    }
}
