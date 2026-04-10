use std::{
    env,
    marker::PhantomData,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::SystemTime,
};

use windows::{
    core::Interface,
    Win32::{
        Foundation::{HMODULE, LUID},
        Graphics::{
            Direct3D::D3D_DRIVER_TYPE_UNKNOWN,
            Direct3D11::{
                D3D11CreateDevice, ID3D11Device, ID3D11Texture2D, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
            },
            Dxgi::{
                Common::{
                    DXGI_MODE_ROTATION_IDENTITY, DXGI_MODE_ROTATION_ROTATE180,
                    DXGI_MODE_ROTATION_ROTATE270, DXGI_MODE_ROTATION_ROTATE90,
                    DXGI_MODE_ROTATION_UNSPECIFIED,
                },
                CreateDXGIFactory1, IDXGIAdapter, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput1,
                IDXGIOutputDuplication, IDXGIResource, DXGI_ERROR_ACCESS_LOST,
                DXGI_ERROR_INVALID_CALL, DXGI_ERROR_NOT_FOUND, DXGI_ERROR_WAIT_TIMEOUT,
                DXGI_OUTDUPL_FRAME_INFO, DXGI_OUTDUPL_POINTER_SHAPE_INFO,
                DXGI_OUTDUPL_POINTER_SHAPE_TYPE_COLOR,
                DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MASKED_COLOR,
                DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MONOCHROME, DXGI_OUTPUT_DESC,
            },
        },
    },
};

use crate::{
    select_display_info, CaptureBackend, CaptureConfig, CaptureError, CaptureSession,
    CaptureTarget, CapturedFrame, DesktopBounds, DisplayId, DisplayInfo, DisplayRotation,
    FrameMetadata, FrameUpdateKind, PointerPosition, PointerShape, PointerShapeKind, PointerUpdate,
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
        Ok(enumerate_outputs()?
            .into_iter()
            .map(|output| output.info)
            .collect())
    }

    fn open(
        &self,
        target: CaptureTarget,
        config: CaptureConfig,
    ) -> Result<Box<dyn CaptureSession>, CaptureError> {
        let mut outputs = enumerate_outputs()?;
        let selected_info = select_display_info(
            &outputs
                .iter()
                .map(|output| output.info.clone())
                .collect::<Vec<_>>(),
            &target,
        )?;
        let selected = outputs
            .drain(..)
            .find(|output| output.info.id == selected_info.id)
            .ok_or_else(|| CaptureError::DisplayNotFound(target.to_string()))?;

        let device = create_device_for_adapter(&selected.adapter)?;

        // Retry DuplicateOutput up to 30 times (over ~30 s) to handle transient
        // failures such as E_INVALIDARG after a previous capture session was
        // abandoned while stuck in a GPU call, or ACCESS_LOST during a DWM
        // transition.  The long retry window gives the abandoned worker time to
        // exit after the MFT abort flush or Windows TDR kicks in.
        let mut duplication = None;
        for attempt in 0..30 {
            match unsafe { selected.output.DuplicateOutput(&device) } {
                Ok(dup) => {
                    trace_capture(format!(
                        "DuplicateOutput succeeded on attempt {}",
                        attempt + 1
                    ));
                    if attempt > 0 {
                        eprintln!(
                            "[capture] DuplicateOutput succeeded on attempt {}",
                            attempt + 1
                        );
                    }
                    duplication = Some(dup);
                    break;
                }
                Err(error) => {
                    trace_capture(format!(
                        "DuplicateOutput attempt {} failed: 0x{:08x} {}",
                        attempt + 1,
                        error.code().0 as u32,
                        error.message()
                    ));
                    eprintln!(
                        "[capture] DuplicateOutput attempt {} failed: 0x{:08x} {}",
                        attempt + 1,
                        error.code().0 as u32,
                        error.message()
                    );
                    if attempt < 29 {
                        std::thread::sleep(std::time::Duration::from_millis(1000));
                    } else {
                        eprintln!(
                            "[capture] DuplicateOutput failed after 30 attempts.  \
                             A previous capture worker may still be stuck in a GPU call.  \
                             Check Windows TDR settings: TdrLevel and TdrDelay in \
                             HKLM\\SYSTEM\\CurrentControlSet\\Control\\GraphicsDrivers."
                        );
                        return Err(map_duplication_error(
                            "IDXGIOutput1::DuplicateOutput",
                            error,
                        ));
                    }
                }
            }
        }
        let duplication = duplication.unwrap();

        Ok(Box::new(DxgiCaptureSession {
            display_info: selected.info,
            device,
            output: selected.output,
            duplication,
            config,
            outstanding_release: None,
            access_lost_recoveries: 0,
            _single_threaded: PhantomData,
        }))
    }
}

pub(crate) struct WindowsFrame {
    texture: ID3D11Texture2D,
    _release: Arc<FrameRelease>,
}

impl WindowsFrame {
    fn new(texture: ID3D11Texture2D, release: Arc<FrameRelease>) -> Self {
        Self {
            texture,
            _release: release,
        }
    }

    pub(crate) fn texture(&self) -> &ID3D11Texture2D {
        &self.texture
    }
}

struct DxgiCaptureSession {
    display_info: DisplayInfo,
    device: ID3D11Device,
    output: IDXGIOutput1,
    duplication: IDXGIOutputDuplication,
    config: CaptureConfig,
    outstanding_release: Option<Arc<FrameRelease>>,
    access_lost_recoveries: u32,
    _single_threaded: PhantomData<Rc<()>>,
}

impl CaptureSession for DxgiCaptureSession {
    fn display_info(&self) -> &DisplayInfo {
        &self.display_info
    }

    fn acquire_frame(&mut self) -> Result<Option<CapturedFrame>, CaptureError> {
        if let Some(release) = self.outstanding_release.take() {
            release.release();
        }

        let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
        let mut desktop_resource: Option<IDXGIResource> = None;

        match unsafe {
            self.duplication.AcquireNextFrame(
                self.config.timeout_ms,
                &mut frame_info,
                &mut desktop_resource,
            )
        } {
            Ok(()) => {}
            Err(error) if error.code() == DXGI_ERROR_WAIT_TIMEOUT => return Ok(None),
            Err(error) => {
                if let Some(reason) = recoverable_acquire_error_reason(error.code()) {
                    return self.recover_from_duplication_loss(reason);
                }
                trace_capture(format!(
                    "AcquireNextFrame error: 0x{:08x} {}",
                    error.code().0 as u32,
                    error.message()
                ));
                eprintln!(
                    "[capture] AcquireNextFrame error: 0x{:08x} {}",
                    error.code().0 as u32,
                    error.message()
                );
                return Err(CaptureError::from_windows(
                    "IDXGIOutputDuplication::AcquireNextFrame",
                    error,
                ));
            }
        }

        let pointer = self.capture_pointer_update(&frame_info)?;
        let update_kind =
            FrameUpdateKind::from_flags(frame_info.LastPresentTime != 0, pointer.is_some());
        trace_capture(format!(
            "frame update_kind={:?} accumulated_frames={} pointer_shape_bytes={}",
            update_kind, frame_info.AccumulatedFrames, frame_info.PointerShapeBufferSize
        ));

        let maybe_texture = if let Some(desktop_resource) = desktop_resource {
            Some(
                desktop_resource
                    .cast::<ID3D11Texture2D>()
                    .map_err(|error| CaptureError::from_windows("IDXGIResource::cast", error))?,
            )
        } else {
            None
        };

        if update_kind.has_image_update() && maybe_texture.is_none() {
            return Err(CaptureError::WindowsApi {
                operation: "IDXGIOutputDuplication::AcquireNextFrame",
                code: 0,
                message: "DXGI returned no desktop resource for an image update".to_owned(),
            });
        }

        let (width, height) = if let Some(texture) = &maybe_texture {
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            unsafe {
                texture.GetDesc(&mut desc);
            }
            (desc.Width, desc.Height)
        } else {
            (
                self.display_info.desktop_bounds.width(),
                self.display_info.desktop_bounds.height(),
            )
        };

        let release = Arc::new(FrameRelease::new(self.duplication.clone()));
        self.outstanding_release = Some(release.clone());

        Ok(Some(CapturedFrame::from_windows(
            FrameMetadata {
                acquired_at: SystemTime::now(),
                width,
                height,
                accumulated_frames: frame_info.AccumulatedFrames,
                last_present_qpc: frame_info.LastPresentTime,
                update_kind,
                pointer,
            },
            maybe_texture.map(|texture| WindowsFrame::new(texture, release)),
        )))
    }

    fn d3d11_device(&self) -> ID3D11Device {
        self.device.clone()
    }

    fn access_lost_recoveries(&self) -> u32 {
        self.access_lost_recoveries
    }

    fn check_device_health(&self) -> Result<(), String> {
        let hr = unsafe { self.device.GetDeviceRemovedReason() };
        if let Err(error) = hr {
            Err(format!(
                "D3D device removed: 0x{:08x} {}",
                error.code().0 as u32,
                error.message()
            ))
        } else {
            Ok(())
        }
    }
}

impl DxgiCaptureSession {
    fn refresh_display_info(&mut self) -> Result<(), CaptureError> {
        let output_desc = unsafe { self.output.GetDesc() }
            .map_err(|error| CaptureError::from_windows("IDXGIOutput::GetDesc", error))?;
        self.display_info = display_info_from_desc(
            self.display_info.id.adapter_luid,
            self.display_info.id.output_index,
            &self.display_info.adapter_name,
            &output_desc,
        );
        Ok(())
    }

    fn capture_pointer_update(
        &self,
        frame_info: &DXGI_OUTDUPL_FRAME_INFO,
    ) -> Result<Option<PointerUpdate>, CaptureError> {
        if frame_info.LastMouseUpdateTime == 0 {
            return Ok(None);
        }

        let position = PointerPosition {
            x: frame_info.PointerPosition.Position.x,
            y: frame_info.PointerPosition.Position.y,
            visible: frame_info.PointerPosition.Visible.as_bool(),
        };
        let shape = if frame_info.PointerShapeBufferSize > 0 {
            Some(self.capture_pointer_shape(frame_info.PointerShapeBufferSize, position)?)
        } else {
            None
        };

        Ok(Some(PointerUpdate {
            last_update_qpc: frame_info.LastMouseUpdateTime,
            position,
            shape,
        }))
    }

    fn capture_pointer_shape(
        &self,
        initial_len: u32,
        position: PointerPosition,
    ) -> Result<PointerShape, CaptureError> {
        let mut buffer_len = initial_len.max(256);
        loop {
            let mut buffer = vec![0u8; buffer_len as usize];
            let mut required_len = 0u32;
            let mut shape_info = DXGI_OUTDUPL_POINTER_SHAPE_INFO::default();
            match unsafe {
                self.duplication.GetFramePointerShape(
                    buffer_len,
                    buffer.as_mut_ptr().cast(),
                    &mut required_len,
                    &mut shape_info,
                )
            } {
                Ok(()) => {
                    buffer.truncate(required_len as usize);
                    return Ok(convert_pointer_shape(shape_info, position, &buffer));
                }
                Err(_error) if required_len > buffer_len => {
                    buffer_len = required_len;
                }
                Err(error) => {
                    return Err(CaptureError::from_windows(
                        "IDXGIOutputDuplication::GetFramePointerShape",
                        error,
                    ))
                }
            }
        }
    }

    /// Handle transient DXGI duplication invalidation by recreating the output duplication.
    ///
    /// This commonly happens on desktop composition changes such as window
    /// focus switches, resolution changes, or DWM recomposition events.
    /// Returns `Ok(None)` on success so the caller retries on the next tick.
    fn recover_from_duplication_loss(
        &mut self,
        reason: &'static str,
    ) -> Result<Option<CapturedFrame>, CaptureError> {
        eprintln!(
            "[capture] duplication invalidated ({reason}) — attempting recovery (previous recoveries: {})",
            self.access_lost_recoveries
        );
        trace_capture(format!(
            "duplication invalidated reason={} previous_recoveries={}",
            reason, self.access_lost_recoveries
        ));

        // Drop the outstanding frame release (the old duplication is invalid).
        self.outstanding_release = None;

        // Brief sleep to let the desktop compositor settle before recreating.
        std::thread::sleep(std::time::Duration::from_millis(100));

        match unsafe { self.output.DuplicateOutput(&self.device) } {
            Ok(new_duplication) => {
                let previous_bounds = self.display_info.desktop_bounds;
                self.duplication = new_duplication;
                self.refresh_display_info()?;
                self.access_lost_recoveries += 1;
                trace_capture(format!(
                    "DXGI recovery succeeded reason={} total_recoveries={} bounds={}x{}",
                    reason,
                    self.access_lost_recoveries,
                    self.display_info.desktop_bounds.width(),
                    self.display_info.desktop_bounds.height()
                ));
                eprintln!(
                    "[capture] DXGI recovery succeeded ({reason}, total recoveries: {})",
                    self.access_lost_recoveries
                );
                if self.display_info.desktop_bounds != previous_bounds {
                    eprintln!(
                        "[capture] desktop bounds changed after recovery: {}x{} -> {}x{}",
                        previous_bounds.width(),
                        previous_bounds.height(),
                        self.display_info.desktop_bounds.width(),
                        self.display_info.desktop_bounds.height()
                    );
                }
                Ok(None)
            }
            Err(error) if error.code() == DXGI_ERROR_ACCESS_LOST => {
                // Still not ready — caller will retry on the next acquire_frame call.
                trace_capture("DuplicateOutput during recovery returned ACCESS_LOST");
                eprintln!("[capture] DuplicateOutput also returned ACCESS_LOST — will retry");
                Ok(None)
            }
            Err(error) => {
                trace_capture(format!(
                    "DuplicateOutput recovery failed: 0x{:08x} {}",
                    error.code().0 as u32,
                    error.message()
                ));
                eprintln!(
                    "[capture] DuplicateOutput recovery failed: 0x{:08x} {}",
                    error.code().0 as u32,
                    error.message()
                );
                Err(map_duplication_error(
                    "IDXGIOutput1::DuplicateOutput (recovery)",
                    error,
                ))
            }
        }
    }
}

fn convert_pointer_shape(
    shape_info: DXGI_OUTDUPL_POINTER_SHAPE_INFO,
    position: PointerPosition,
    raw_shape: &[u8],
) -> PointerShape {
    let kind_raw = shape_info.Type;
    let kind = match kind_raw {
        value if value == DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MONOCHROME.0 as u32 => {
            PointerShapeKind::Monochrome
        }
        value if value == DXGI_OUTDUPL_POINTER_SHAPE_TYPE_COLOR.0 as u32 => PointerShapeKind::Color,
        value if value == DXGI_OUTDUPL_POINTER_SHAPE_TYPE_MASKED_COLOR.0 as u32 => {
            PointerShapeKind::MaskedColor
        }
        _ => PointerShapeKind::Unknown(kind_raw),
    };

    let pixels_rgba = match kind {
        PointerShapeKind::Monochrome => decode_monochrome_pointer_shape(
            raw_shape,
            shape_info.Width,
            shape_info.Height,
            position.visible,
        ),
        PointerShapeKind::Color | PointerShapeKind::MaskedColor | PointerShapeKind::Unknown(_) => {
            decode_color_pointer_shape(
                raw_shape,
                shape_info.Width,
                shape_info.Height,
                shape_info.Pitch,
            )
        }
    };

    PointerShape {
        kind,
        width: shape_info.Width,
        height: shape_info.Height,
        pitch: shape_info.Pitch,
        hotspot_x: shape_info.HotSpot.x,
        hotspot_y: shape_info.HotSpot.y,
        pixels_rgba,
    }
}

fn decode_color_pointer_shape(raw_shape: &[u8], width: u32, height: u32, pitch: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    let row_pitch = pitch.max(width * 4) as usize;
    for y in 0..height as usize {
        let src_row = y * row_pitch;
        let dst_row = y * width as usize * 4;
        if src_row >= raw_shape.len() {
            break;
        }
        for x in 0..width as usize {
            let src = src_row + (x * 4);
            let dst = dst_row + (x * 4);
            if src + 4 > raw_shape.len() || dst + 4 > rgba.len() {
                break;
            }
            let b = raw_shape[src];
            let g = raw_shape[src + 1];
            let r = raw_shape[src + 2];
            let a = raw_shape[src + 3];
            rgba[dst] = r;
            rgba[dst + 1] = g;
            rgba[dst + 2] = b;
            rgba[dst + 3] = a;
        }
    }
    rgba
}

fn decode_monochrome_pointer_shape(
    raw_shape: &[u8],
    width: u32,
    height: u32,
    visible: bool,
) -> Vec<u8> {
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    if !visible || width == 0 || height == 0 {
        return rgba;
    }

    let bytes_per_row = width.div_ceil(8) as usize;
    let mask_height = height as usize;
    let xor_plane_start = bytes_per_row * mask_height;
    for y in 0..mask_height {
        let and_row_start = y * bytes_per_row;
        let xor_row_start = xor_plane_start + (y * bytes_per_row);
        for x in 0..width as usize {
            let bit = 0x80 >> (x % 8);
            let and_set = raw_shape
                .get(and_row_start + (x / 8))
                .map(|value| value & bit != 0)
                .unwrap_or(false);
            let xor_set = raw_shape
                .get(xor_row_start + (x / 8))
                .map(|value| value & bit != 0)
                .unwrap_or(false);
            let offset = (y * width as usize + x) * 4;
            match (and_set, xor_set) {
                (false, false) => {
                    rgba[offset] = 0;
                    rgba[offset + 1] = 0;
                    rgba[offset + 2] = 0;
                    rgba[offset + 3] = 255;
                }
                (false, true) => {
                    rgba[offset] = 255;
                    rgba[offset + 1] = 255;
                    rgba[offset + 2] = 255;
                    rgba[offset + 3] = 255;
                }
                (true, false) => {
                    rgba[offset] = 0;
                    rgba[offset + 1] = 0;
                    rgba[offset + 2] = 0;
                    rgba[offset + 3] = 0;
                }
                (true, true) => {
                    rgba[offset] = 255;
                    rgba[offset + 1] = 255;
                    rgba[offset + 2] = 255;
                    rgba[offset + 3] = 255;
                }
            }
        }
    }
    rgba
}

fn trace_capture(message: impl AsRef<str>) {
    if env::var_os("HOLOBRIDGE_CAPTURE_TRACE").is_some() {
        eprintln!("[holobridge-capture] {}", message.as_ref());
    }
}

struct FrameRelease {
    duplication: IDXGIOutputDuplication,
    released: AtomicBool,
}

impl FrameRelease {
    fn new(duplication: IDXGIOutputDuplication) -> Self {
        Self {
            duplication,
            released: AtomicBool::new(false),
        }
    }

    fn release(&self) {
        if self.released.swap(true, Ordering::AcqRel) {
            return;
        }

        let _ = unsafe { self.duplication.ReleaseFrame() };
    }
}

impl Drop for FrameRelease {
    fn drop(&mut self) {
        if self.released.swap(true, Ordering::AcqRel) {
            return;
        }

        let _ = unsafe { self.duplication.ReleaseFrame() };
    }
}

struct EnumeratedOutput {
    info: DisplayInfo,
    adapter: IDXGIAdapter1,
    output: IDXGIOutput1,
}

fn enumerate_outputs() -> Result<Vec<EnumeratedOutput>, CaptureError> {
    let factory: IDXGIFactory1 = unsafe { CreateDXGIFactory1() }
        .map_err(|error| CaptureError::from_windows("CreateDXGIFactory1", error))?;

    let mut outputs = Vec::new();
    let mut adapter_index = 0u32;
    while let Some(adapter) = next_adapter(&factory, adapter_index)? {
        let adapter_desc = unsafe { adapter.GetDesc1() }
            .map_err(|error| CaptureError::from_windows("IDXGIAdapter1::GetDesc1", error))?;
        let adapter_name = utf16_slice_to_string(&adapter_desc.Description);
        let adapter_luid = luid_to_i64(adapter_desc.AdapterLuid);

        let mut output_index = 0u32;
        while let Some(output) = next_output(&adapter, output_index)? {
            let output_desc = unsafe { output.GetDesc() }
                .map_err(|error| CaptureError::from_windows("IDXGIOutput::GetDesc", error))?;
            if output_desc.AttachedToDesktop.as_bool() {
                let output1: IDXGIOutput1 = output
                    .cast()
                    .map_err(|error| CaptureError::from_windows("IDXGIOutput::cast", error))?;
                outputs.push(EnumeratedOutput {
                    info: display_info_from_desc(
                        adapter_luid,
                        output_index,
                        &adapter_name,
                        &output_desc,
                    ),
                    adapter: adapter.clone(),
                    output: output1,
                });
            }

            output_index += 1;
        }

        adapter_index += 1;
    }

    if outputs.is_empty() {
        return Err(CaptureError::NoDisplays);
    }

    if !outputs.iter().any(|output| output.info.is_primary) {
        if let Some(first) = outputs.first_mut() {
            first.info.is_primary = true;
        }
    }

    Ok(outputs)
}

fn next_adapter(
    factory: &IDXGIFactory1,
    adapter_index: u32,
) -> Result<Option<IDXGIAdapter1>, CaptureError> {
    match unsafe { factory.EnumAdapters1(adapter_index) } {
        Ok(adapter) => Ok(Some(adapter)),
        Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => Ok(None),
        Err(error) => Err(CaptureError::from_windows(
            "IDXGIFactory1::EnumAdapters1",
            error,
        )),
    }
}

fn next_output(
    adapter: &IDXGIAdapter1,
    output_index: u32,
) -> Result<Option<windows::Win32::Graphics::Dxgi::IDXGIOutput>, CaptureError> {
    match unsafe { adapter.EnumOutputs(output_index) } {
        Ok(output) => Ok(Some(output)),
        Err(error) if error.code() == DXGI_ERROR_NOT_FOUND => Ok(None),
        Err(error) => Err(CaptureError::from_windows(
            "IDXGIAdapter::EnumOutputs",
            error,
        )),
    }
}

fn create_device_for_adapter(adapter: &IDXGIAdapter1) -> Result<ID3D11Device, CaptureError> {
    let adapter: IDXGIAdapter = adapter
        .cast()
        .map_err(|error| CaptureError::from_windows("IDXGIAdapter1::cast", error))?;
    let mut device = None;

    unsafe {
        D3D11CreateDevice(
            &adapter,
            D3D_DRIVER_TYPE_UNKNOWN,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            None,
        )
    }
    .map_err(|error| CaptureError::from_windows("D3D11CreateDevice", error))?;

    device.ok_or(CaptureError::WindowsApi {
        operation: "D3D11CreateDevice",
        code: 0,
        message: "Direct3D returned no device".to_owned(),
    })
}

fn display_info_from_desc(
    adapter_luid: i64,
    output_index: u32,
    adapter_name: &str,
    output_desc: &DXGI_OUTPUT_DESC,
) -> DisplayInfo {
    DisplayInfo {
        id: DisplayId {
            adapter_luid,
            output_index,
        },
        adapter_name: adapter_name.to_owned(),
        output_name: utf16_slice_to_string(&output_desc.DeviceName),
        is_primary: output_desc.DesktopCoordinates.left == 0
            && output_desc.DesktopCoordinates.top == 0,
        desktop_bounds: DesktopBounds {
            left: output_desc.DesktopCoordinates.left,
            top: output_desc.DesktopCoordinates.top,
            right: output_desc.DesktopCoordinates.right,
            bottom: output_desc.DesktopCoordinates.bottom,
        },
        rotation: match output_desc.Rotation {
            DXGI_MODE_ROTATION_IDENTITY => DisplayRotation::Identity,
            DXGI_MODE_ROTATION_ROTATE90 => DisplayRotation::Rotate90,
            DXGI_MODE_ROTATION_ROTATE180 => DisplayRotation::Rotate180,
            DXGI_MODE_ROTATION_ROTATE270 => DisplayRotation::Rotate270,
            DXGI_MODE_ROTATION_UNSPECIFIED => DisplayRotation::Unspecified,
            _ => DisplayRotation::Unspecified,
        },
    }
}

fn map_duplication_error(operation: &'static str, error: windows::core::Error) -> CaptureError {
    match error.code() {
        DXGI_ERROR_ACCESS_LOST => CaptureError::AccessLost,
        _ => CaptureError::from_windows(operation, error),
    }
}

fn recoverable_acquire_error_reason(code: windows::core::HRESULT) -> Option<&'static str> {
    if code == DXGI_ERROR_ACCESS_LOST {
        Some("access_lost")
    } else if code == DXGI_ERROR_INVALID_CALL {
        Some("invalid_call")
    } else {
        None
    }
}

fn utf16_slice_to_string(value: &[u16]) -> String {
    let end = value.iter().position(|ch| *ch == 0).unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end]).trim().to_owned()
}

fn luid_to_i64(luid: LUID) -> i64 {
    ((luid.HighPart as i64) << 32) | i64::from(luid.LowPart)
}

#[cfg(test)]
mod tests {
    use super::recoverable_acquire_error_reason;
    use windows::Win32::Graphics::Dxgi::{
        DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_INVALID_CALL, DXGI_ERROR_NOT_FOUND,
    };

    #[test]
    fn acquire_recovery_reason_classifies_transient_duplication_loss() {
        assert_eq!(
            recoverable_acquire_error_reason(DXGI_ERROR_ACCESS_LOST),
            Some("access_lost")
        );
        assert_eq!(
            recoverable_acquire_error_reason(DXGI_ERROR_INVALID_CALL),
            Some("invalid_call")
        );
        assert_eq!(recoverable_acquire_error_reason(DXGI_ERROR_NOT_FOUND), None);
    }
}
