use crate::{
    CaptureBackend, CaptureConfig, CaptureError, CaptureSession, CaptureTarget,
    DisplayInfo,
};

#[derive(Debug, Default, Clone, Copy)]
pub struct DxgiCaptureBackend;

impl DxgiCaptureBackend {
    pub fn new() -> Result<Self, CaptureError> {
        Ok(Self)
    }
}

impl CaptureBackend for DxgiCaptureBackend {
    fn enumerate_displays(&self) -> Result<Vec<DisplayInfo>, CaptureError> {
        Err(CaptureError::UnsupportedPlatform)
    }

    fn open(
        &self,
        _target: CaptureTarget,
        _config: CaptureConfig,
    ) -> Result<Box<dyn CaptureSession>, CaptureError> {
        Err(CaptureError::UnsupportedPlatform)
    }
}
