use holobridge_capture::CapturedFrame;

use crate::{
    EncodeError, EncodedAccessUnit, VideoEncoder, VideoEncoderConfig,
};

#[derive(Debug, Default)]
pub struct MfH264Encoder;

impl MfH264Encoder {
    pub fn new(_config: VideoEncoderConfig) -> Result<Self, EncodeError> {
        Err(EncodeError::UnsupportedPlatform)
    }
}

impl VideoEncoder for MfH264Encoder {
    fn encode(
        &mut self,
        _frame: &CapturedFrame,
    ) -> Result<Vec<EncodedAccessUnit>, EncodeError> {
        Err(EncodeError::UnsupportedPlatform)
    }

    fn flush(&mut self) -> Result<Vec<EncodedAccessUnit>, EncodeError> {
        Err(EncodeError::UnsupportedPlatform)
    }
}
