use std::{
    error::Error,
    fmt,
    str::FromStr,
    time::SystemTime,
};

#[cfg(windows)]
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11Texture2D,
};

#[cfg(not(windows))]
mod stub_backend;
#[cfg(windows)]
mod windows_backend;

#[cfg(not(windows))]
pub use stub_backend::DxgiCaptureBackend;
#[cfg(windows)]
pub use windows_backend::DxgiCaptureBackend;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayId {
    pub adapter_luid: i64,
    pub output_index: u32,
}

impl fmt::Display for DisplayId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}:{}", self.adapter_luid, self.output_index)
    }
}

impl FromStr for DisplayId {
    type Err = DisplayIdParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (adapter_luid, output_index) = value
            .split_once(':')
            .ok_or_else(|| DisplayIdParseError(value.to_owned()))?;

        let adapter_luid = adapter_luid
            .parse::<i64>()
            .map_err(|_| DisplayIdParseError(value.to_owned()))?;
        let output_index = output_index
            .parse::<u32>()
            .map_err(|_| DisplayIdParseError(value.to_owned()))?;

        Ok(Self {
            adapter_luid,
            output_index,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayIdParseError(String);

impl fmt::Display for DisplayIdParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "display id must be formatted as <adapter_luid>:<output_index>, got '{}'",
            self.0
        )
    }
}

impl Error for DisplayIdParseError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopBounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl DesktopBounds {
    pub fn width(&self) -> u32 {
        (self.right - self.left).max(0) as u32
    }

    pub fn height(&self) -> u32 {
        (self.bottom - self.top).max(0) as u32
    }
}

impl fmt::Display for DesktopBounds {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "({}, {}) -> ({}, {})",
            self.left, self.top, self.right, self.bottom
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayRotation {
    Identity,
    Rotate90,
    Rotate180,
    Rotate270,
    Unspecified,
}

impl fmt::Display for DisplayRotation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::Identity => "identity",
            Self::Rotate90 => "rotate90",
            Self::Rotate180 => "rotate180",
            Self::Rotate270 => "rotate270",
            Self::Unspecified => "unspecified",
        };
        formatter.write_str(label)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayInfo {
    pub id: DisplayId,
    pub adapter_name: String,
    pub output_name: String,
    pub is_primary: bool,
    pub desktop_bounds: DesktopBounds,
    pub rotation: DisplayRotation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureConfig {
    pub timeout_ms: u32,
    pub target_fps_hint: Option<u32>,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            timeout_ms: 16,
            target_fps_hint: Some(60),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameMetadata {
    pub acquired_at: SystemTime,
    pub width: u32,
    pub height: u32,
    pub accumulated_frames: u32,
    pub last_present_qpc: i64,
    pub update_kind: FrameUpdateKind,
    pub pointer: Option<PointerUpdate>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameUpdateKind {
    None,
    ImageOnly,
    PointerOnly,
    ImageAndPointer,
}

impl FrameUpdateKind {
    pub fn from_flags(
        has_image_update: bool,
        has_pointer_update: bool,
    ) -> Self {
        match (has_image_update, has_pointer_update) {
            (false, false) => Self::None,
            (true, false) => Self::ImageOnly,
            (false, true) => Self::PointerOnly,
            (true, true) => Self::ImageAndPointer,
        }
    }

    pub fn has_image_update(&self) -> bool {
        matches!(self, Self::ImageOnly | Self::ImageAndPointer)
    }

    pub fn has_pointer_update(&self) -> bool {
        matches!(self, Self::PointerOnly | Self::ImageAndPointer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerPosition {
    pub x: i32,
    pub y: i32,
    pub visible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerUpdate {
    pub last_update_qpc: i64,
    pub position: PointerPosition,
    pub shape: Option<PointerShape>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PointerShape {
    pub kind: PointerShapeKind,
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub hotspot_x: i32,
    pub hotspot_y: i32,
    pub pixels_rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerShapeKind {
    Monochrome,
    Color,
    MaskedColor,
    Unknown(u32),
}

impl PointerShapeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Monochrome => "monochrome",
            Self::Color => "color",
            Self::MaskedColor => "masked_color",
            Self::Unknown(_) => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureTarget {
    Primary,
    Display(DisplayId),
}

impl fmt::Display for CaptureTarget {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Primary => formatter.write_str("primary"),
            Self::Display(display_id) => write!(formatter, "{display_id}"),
        }
    }
}

#[derive(Debug)]
pub enum CaptureError {
    UnsupportedPlatform,
    DisplayNotFound(String),
    NoDisplays,
    Timeout,
    AccessLost,
    WindowsApi {
        operation: &'static str,
        code: i32,
        message: String,
    },
}

pub trait CaptureBackend {
    fn enumerate_displays(&self) -> Result<Vec<DisplayInfo>, CaptureError>;

    fn open(
        &self,
        target: CaptureTarget,
        config: CaptureConfig,
    ) -> Result<Box<dyn CaptureSession>, CaptureError>;
}

pub trait CaptureSession {
    fn display_info(&self) -> &DisplayInfo;

    fn acquire_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError>;

    /// Number of times the session recovered from access-lost errors.
    fn access_lost_recoveries(&self) -> u32 { 0 }

    /// Check whether the underlying D3D device is still healthy.
    /// Returns `Ok(())` if the device is fine, or a descriptive error string.
    fn check_device_health(&self) -> Result<(), String> { Ok(()) }

    #[cfg(windows)]
    fn d3d11_device(&self) -> ID3D11Device;
}

#[cfg_attr(not(windows), allow(dead_code))]
enum CapturedFrameInner {
    #[cfg(windows)]
    Dxgi(Option<windows_backend::WindowsFrame>),
    #[cfg(not(windows))]
    Unsupported,
}

pub struct CapturedFrame {
    metadata: FrameMetadata,
    #[cfg_attr(not(windows), allow(dead_code))]
    inner: CapturedFrameInner,
}

impl CapturedFrame {
    pub fn metadata(&self) -> &FrameMetadata {
        &self.metadata
    }

    #[cfg(windows)]
    pub fn texture(&self) -> Option<&ID3D11Texture2D> {
        match &self.inner {
            CapturedFrameInner::Dxgi(Some(frame)) => Some(frame.texture()),
            CapturedFrameInner::Dxgi(None) => None,
        }
    }

    #[cfg(windows)]
    pub(crate) fn from_windows(
        metadata: FrameMetadata,
        frame: Option<windows_backend::WindowsFrame>,
    ) -> Self {
        Self {
            metadata,
            inner: CapturedFrameInner::Dxgi(frame),
        }
    }
}

impl FrameMetadata {
    pub fn image_updated(&self) -> bool {
        self.update_kind.has_image_update()
    }

    pub fn pointer_updated(&self) -> bool {
        self.update_kind.has_pointer_update()
    }
}

impl fmt::Debug for CapturedFrame {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CapturedFrame")
            .field("metadata", &self.metadata)
            .finish()
    }
}

impl fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => {
                formatter.write_str("capture is only supported on Windows")
            }
            Self::DisplayNotFound(display) => {
                write!(formatter, "display not found: {display}")
            }
            Self::NoDisplays => formatter.write_str("no attached displays found"),
            Self::Timeout => formatter.write_str("timed out waiting for a new frame"),
            Self::AccessLost => {
                formatter.write_str("desktop duplication access was lost")
            }
            Self::WindowsApi {
                operation,
                code,
                message,
            } => write!(
                formatter,
                "{operation} failed with HRESULT 0x{code:08x}: {message}",
                code = *code as u32
            ),
        }
    }
}

impl Error for CaptureError {}

#[cfg(windows)]
impl CaptureError {
    pub(crate) fn from_windows(
        operation: &'static str,
        error: windows::core::Error,
    ) -> Self {
        Self::WindowsApi {
            operation,
            code: error.code().0,
            message: error.to_string(),
        }
    }
}

#[cfg_attr(not(any(test, windows)), allow(dead_code))]
pub(crate) fn select_display_info(
    displays: &[DisplayInfo],
    target: &CaptureTarget,
) -> Result<DisplayInfo, CaptureError> {
    match target {
        CaptureTarget::Primary => displays
            .iter()
            .find(|display| display.is_primary)
            .cloned()
            .or_else(|| displays.first().cloned())
            .ok_or(CaptureError::NoDisplays),
        CaptureTarget::Display(display_id) => displays
            .iter()
            .find(|display| display.id == *display_id)
            .cloned()
            .ok_or_else(|| CaptureError::DisplayNotFound(display_id.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn display(
        adapter_luid: i64,
        output_index: u32,
        is_primary: bool,
    ) -> DisplayInfo {
        DisplayInfo {
            id: DisplayId {
                adapter_luid,
                output_index,
            },
            adapter_name: format!("Adapter {adapter_luid}"),
            output_name: format!("Output {output_index}"),
            is_primary,
            desktop_bounds: DesktopBounds {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
            rotation: DisplayRotation::Identity,
        }
    }

    #[test]
    fn display_id_round_trips_through_string_format() {
        let original = DisplayId {
            adapter_luid: -42,
            output_index: 7,
        };

        let parsed = original.to_string().parse::<DisplayId>().unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn capture_target_selection_prefers_primary_display() {
        let displays = vec![display(10, 0, false), display(20, 1, true)];

        let selected = select_display_info(&displays, &CaptureTarget::Primary).unwrap();
        assert!(selected.is_primary);
        assert_eq!(selected.id.adapter_luid, 20);
    }

    #[test]
    fn capture_target_selection_supports_explicit_display_id() {
        let displays = vec![display(10, 0, true), display(20, 1, false)];
        let target = CaptureTarget::Display(DisplayId {
            adapter_luid: 20,
            output_index: 1,
        });

        let selected = select_display_info(&displays, &target).unwrap();
        assert_eq!(selected.id.adapter_luid, 20);
        assert_eq!(selected.id.output_index, 1);
    }

    #[test]
    fn frame_update_kind_distinguishes_pointer_and_image_updates() {
        assert_eq!(FrameUpdateKind::from_flags(true, false), FrameUpdateKind::ImageOnly);
        assert_eq!(FrameUpdateKind::from_flags(false, true), FrameUpdateKind::PointerOnly);
        assert_eq!(
            FrameUpdateKind::from_flags(true, true),
            FrameUpdateKind::ImageAndPointer
        );
        assert_eq!(FrameUpdateKind::from_flags(false, false), FrameUpdateKind::None);
    }

    #[cfg(not(windows))]
    #[test]
    fn non_windows_backend_reports_unsupported_platform() {
        let backend = DxgiCaptureBackend::new().unwrap();
        let err = backend.enumerate_displays().unwrap_err();
        assert!(matches!(err, CaptureError::UnsupportedPlatform));
    }

    #[cfg(windows)]
    #[test]
    fn windows_backend_enumerates_structured_display_info() {
        let backend = DxgiCaptureBackend::new().unwrap();
        let displays = backend.enumerate_displays().unwrap();

        assert!(!displays.is_empty());
        assert!(displays.iter().all(|display| !display.adapter_name.is_empty()));
        assert!(displays.iter().all(|display| !display.output_name.is_empty()));
    }

    #[cfg(windows)]
    #[test]
    fn windows_primary_selection_matches_marked_primary_display() {
        let backend = DxgiCaptureBackend::new().unwrap();
        let displays = backend.enumerate_displays().unwrap();

        let selected = select_display_info(&displays, &CaptureTarget::Primary).unwrap();
        assert!(selected.is_primary);
    }
}
