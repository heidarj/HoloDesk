use std::{
    mem::ManuallyDrop,
    ptr,
    rc::Rc,
    slice,
};

use holobridge_capture::CapturedFrame;
use windows::{
    core::{Error as WindowsError, Interface, IUnknown},
    Win32::{
        Foundation::{
            RECT, RPC_E_CHANGED_MODE, VARIANT_FALSE, VARIANT_TRUE,
        },
        Graphics::{
            Direct3D11::{
                D3D11_BIND_RENDER_TARGET,
                D3D11_BIND_SHADER_RESOURCE, D3D11_RESOURCE_MISC_FLAG,
                D3D11_TEX2D_VPIV, D3D11_TEX2D_VPOV,
                D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT,
                D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
                D3D11_VIDEO_PROCESSOR_CONTENT_DESC,
                D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC,
                D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0,
                D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC,
                D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0,
                D3D11_VIDEO_PROCESSOR_OUTPUT_RATE_NORMAL,
                D3D11_VIDEO_PROCESSOR_STREAM,
                D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
                D3D11_VPIV_DIMENSION_TEXTURE2D,
                D3D11_VPOV_DIMENSION_TEXTURE2D, ID3D11Device,
                ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
                ID3D11VideoContext, ID3D11VideoDevice,
                ID3D11VideoProcessor, ID3D11VideoProcessorEnumerator,
            },
            Dxgi::Common::{
                DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_NV12,
                DXGI_RATIONAL, DXGI_SAMPLE_DESC,
            },
        },
        Media::MediaFoundation::{
            ICodecAPI, IMFActivate, IMFDXGIDeviceManager,
            IMFMediaEventGenerator, IMFMediaType, IMFSample, IMFTransform,
            MF_E_NO_EVENTS_AVAILABLE, MF_E_NOTACCEPTING,
            MF_E_TRANSFORM_NEED_MORE_INPUT, MF_E_TRANSFORM_STREAM_CHANGE,
            MF_MT_AVG_BITRATE, MF_MT_FRAME_RATE,
            MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE,
            MF_MT_MPEG2_PROFILE, MF_MT_MPEG_SEQUENCE_HEADER,
            MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SUBTYPE,
            MFSampleExtension_CleanPoint, MFSTARTUP_FULL, MFStartup,
            MFShutdown, MFCreateDXGIDeviceManager, MFCreateDXGISurfaceBuffer,
            MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
            MFMediaType_Video, MFVideoFormat_H264, MFVideoFormat_NV12,
            MFVideoInterlace_Progressive, MFT_ENUM_FLAG_HARDWARE,
            MFT_ENUM_FLAG_SORTANDFILTER, MFT_MESSAGE_COMMAND_DRAIN,
            MFT_MESSAGE_NOTIFY_BEGIN_STREAMING,
            MFT_MESSAGE_NOTIFY_END_OF_STREAM,
            MFT_MESSAGE_NOTIFY_END_STREAMING,
            MFT_MESSAGE_NOTIFY_START_OF_STREAM, MFT_MESSAGE_SET_D3D_MANAGER,
            MFT_CATEGORY_VIDEO_ENCODER, MFT_OUTPUT_DATA_BUFFER,
            MFT_OUTPUT_STREAM_INFO,
            MFT_OUTPUT_STREAM_PROVIDES_SAMPLES, MFT_REGISTER_TYPE_INFO,
            MFTEnumEx, MF_VERSION, METransformHaveOutput,
            METransformNeedInput,
            MF_EVENT_FLAG_NO_WAIT, CODECAPI_AVEncCommonMeanBitRate,
            CODECAPI_AVEncCommonRateControlMode,
            CODECAPI_AVEncMPVDefaultBPictureCount,
            CODECAPI_AVEncMPVGOPSize, CODECAPI_AVLowLatencyMode, MF_TRANSFORM_ASYNC_UNLOCK,
            eAVEncCommonRateControlMode_CBR, eAVEncH264VProfile_Main,
        },
        System::{
            Com::{CoInitializeEx, CoTaskMemFree, CoUninitialize, COINIT_MULTITHREADED},
            Variant::{
                VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0,
                VT_BOOL, VT_UI4,
            },
        },
    },
};

use crate::{
    assemble_annex_b_access_unit, h264_sequence_header_to_annex_b,
    pack_ratio_u64, EncodeError, EncodedAccessUnit, H264Profile,
    VideoEncoder, VideoEncoderConfig,
};

const INPUT_STREAM_ID: u32 = 0;
const OUTPUT_STREAM_ID: u32 = 0;

pub struct MfH264Encoder {
    _com_guard: ComGuard,
    _mf_guard: MediaFoundationGuard,
    _activate: IMFActivate,
    transform: IMFTransform,
    _device_manager: IMFDXGIDeviceManager,
    config: VideoEncoderConfig,
    frame_duration_100ns: i64,
    next_pts_100ns: i64,
    sequence_header_annex_b: Vec<u8>,
    nal_length_size: usize,
    output_stream_info: MFT_OUTPUT_STREAM_INFO,
    event_generator: Option<IMFMediaEventGenerator>,
    output_pending: bool,
    input_needed: bool,
    color_converter: GpuBgraToNv12Converter,
    _single_threaded: Rc<()>,
}

impl MfH264Encoder {
    pub fn new(
        device: &ID3D11Device,
        config: VideoEncoderConfig,
    ) -> Result<Self, EncodeError> {
        config.validate()?;

        let com_guard = ComGuard::new()?;
        let mf_guard = MediaFoundationGuard::new()?;
        let activate = create_hardware_encoder_activate()?;
        let transform: IMFTransform = unsafe {
            activate
                .ActivateObject()
                .map_err(|error| map_windows_error("IMFActivate::ActivateObject", error))?
        };
        let event_generator = transform.cast::<IMFMediaEventGenerator>().ok();

        if let Ok(attributes) = unsafe { transform.GetAttributes() } {
            let _ = unsafe { attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) };
        }

        let device_manager = create_dxgi_device_manager(device)?;
        unsafe {
            transform
                .ProcessMessage(
                    MFT_MESSAGE_SET_D3D_MANAGER,
                    device_manager.as_raw() as usize,
                )
                .map_err(|error| map_windows_error("IMFTransform::ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER)", error))?;
        }

        configure_output_type(&transform, &config)?;
        configure_codec_properties(&transform, &config)?;
        configure_input_type(&transform, &config)?;

        unsafe {
            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0)
                .map_err(|error| map_windows_error("IMFTransform::ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING)", error))?;
            transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0)
                .map_err(|error| map_windows_error("IMFTransform::ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM)", error))?;
        }

        let output_stream_info = unsafe {
            transform
                .GetOutputStreamInfo(OUTPUT_STREAM_ID)
                .map_err(|error| map_windows_error("IMFTransform::GetOutputStreamInfo", error))?
        };
        let (nal_length_size, sequence_header_annex_b) =
            read_sequence_header(&transform)?;
        let color_converter = GpuBgraToNv12Converter::new(device, &config)?;

        let frame_duration_100ns = config.frame_duration_100ns()?;

        Ok(Self {
            _com_guard: com_guard,
            _mf_guard: mf_guard,
            _activate: activate,
            transform,
            _device_manager: device_manager,
            config,
            frame_duration_100ns,
            next_pts_100ns: 0,
            sequence_header_annex_b,
            nal_length_size,
            output_stream_info,
            event_generator,
            output_pending: false,
            input_needed: true,
            color_converter,
            _single_threaded: Rc::new(()),
        })
    }
}

impl VideoEncoder for MfH264Encoder {
    fn encode(
        &mut self,
        frame: &CapturedFrame,
    ) -> Result<Vec<EncodedAccessUnit>, EncodeError> {
        let metadata = frame.metadata();
        if metadata.width != self.config.width || metadata.height != self.config.height {
            return Err(EncodeError::InvalidConfig(
                "captured frame dimensions do not match encoder configuration",
            ));
        }

        self.pump_events()?;

        let nv12_texture = self.color_converter.convert(frame.texture())?;
        let sample = create_input_sample(
            &nv12_texture,
            self.next_pts_100ns,
            self.frame_duration_100ns,
        )?;
        self.next_pts_100ns = self
            .next_pts_100ns
            .saturating_add(self.frame_duration_100ns);

        match unsafe { self.transform.ProcessInput(INPUT_STREAM_ID, &sample, 0) }
        {
            Ok(()) => {
                self.input_needed = false;
            }
            Err(error) if error.code() == MF_E_NOTACCEPTING => {
                let mut output = self.drain_available_output()?;
                self.pump_events()?;
                unsafe {
                    self.transform
                        .ProcessInput(INPUT_STREAM_ID, &sample, 0)
                        .map_err(|error| map_windows_error("IMFTransform::ProcessInput", error))?;
                }
                self.input_needed = false;
                output.extend(self.drain_available_output()?);
                return Ok(output);
            }
            Err(error) => {
                return Err(map_windows_error("IMFTransform::ProcessInput", error));
            }
        }

        self.drain_available_output()
    }

    fn flush(&mut self) -> Result<Vec<EncodedAccessUnit>, EncodeError> {
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
                .map_err(|error| map_windows_error("IMFTransform::ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM)", error))?;
            self.transform
                .ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0)
                .map_err(|error| map_windows_error("IMFTransform::ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN)", error))?;
        }
        self.output_pending = true;
        self.drain_available_output()
    }
}

impl Drop for MfH264Encoder {
    fn drop(&mut self) {
        let _ = unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0)
        };
        let _ = unsafe { self._activate.ShutdownObject() };
    }
}

impl MfH264Encoder {
    fn pump_events(&mut self) -> Result<(), EncodeError> {
        let Some(event_generator) = &self.event_generator else {
            self.input_needed = true;
            return Ok(());
        };

        loop {
            let event = match unsafe { event_generator.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
                Ok(event) => event,
                Err(error) if error.code() == MF_E_NO_EVENTS_AVAILABLE => break,
                Err(error) => {
                    return Err(map_windows_error(
                        "IMFMediaEventGenerator::GetEvent",
                        error,
                    ));
                }
            };

            let event_type = unsafe {
                event
                    .GetType()
                    .map_err(|error| map_windows_error("IMFMediaEvent::GetType", error))?
            };

            if event_type == METransformNeedInput.0 as u32 {
                self.input_needed = true;
            } else if event_type == METransformHaveOutput.0 as u32 {
                self.output_pending = true;
            }
        }

        Ok(())
    }

    fn drain_available_output(
        &mut self,
    ) -> Result<Vec<EncodedAccessUnit>, EncodeError> {
        let mut encoded = Vec::new();
        loop {
            self.pump_events()?;

            if self.event_generator.is_some() && !self.output_pending {
                break;
            }

            match self.process_output_once()? {
                Some(access_unit) => {
                    encoded.push(access_unit);
                    self.output_pending = false;
                }
                None => break,
            }
        }

        Ok(encoded)
    }

    fn process_output_once(
        &mut self,
    ) -> Result<Option<EncodedAccessUnit>, EncodeError> {
        let maybe_output_sample = if (self.output_stream_info.dwFlags
            & MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32)
            != 0
        {
            None
        } else {
            Some(create_output_sample(self.output_stream_info.cbSize)?)
        };

        let mut output_buffer = MFT_OUTPUT_DATA_BUFFER {
            dwStreamID: OUTPUT_STREAM_ID,
            pSample: ManuallyDrop::new(maybe_output_sample),
            dwStatus: 0,
            pEvents: ManuallyDrop::new(None),
        };
        let mut output_status = 0u32;

        match unsafe {
            self.transform.ProcessOutput(
                0,
                slice::from_mut(&mut output_buffer),
                &mut output_status,
            )
        } {
            Ok(()) => {}
            Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => {
                self.input_needed = true;
                drop_output_events(&mut output_buffer);
                return Ok(None);
            }
            Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => {
                drop_output_events(&mut output_buffer);
                let (nal_length_size, sequence_header_annex_b) =
                    read_sequence_header(&self.transform)?;
                self.nal_length_size = nal_length_size;
                self.sequence_header_annex_b = sequence_header_annex_b;
                return Ok(None);
            }
            Err(error) => {
                drop_output_events(&mut output_buffer);
                return Err(map_windows_error(
                    "IMFTransform::ProcessOutput",
                    error,
                ));
            }
        }

        let sample = take_output_sample(&mut output_buffer);
        drop_output_events(&mut output_buffer);
        let Some(sample) = sample else {
            return Ok(None);
        };

        let buffer = unsafe {
            sample
                .ConvertToContiguousBuffer()
                .map_err(|error| map_windows_error("IMFSample::ConvertToContiguousBuffer", error))?
        };
        let payload = copy_buffer_bytes(&buffer)?;
        if payload.is_empty() {
            return Ok(None);
        }

        let is_keyframe = unsafe {
            sample.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) != 0
        };
        let pts_100ns = unsafe { sample.GetSampleTime().unwrap_or(0) };
        let duration_100ns =
            unsafe { sample.GetSampleDuration().unwrap_or(self.frame_duration_100ns) };
        let data = assemble_annex_b_access_unit(
            &payload,
            Some(&self.sequence_header_annex_b),
            Some(self.nal_length_size),
            is_keyframe,
        )?;

        Ok(Some(EncodedAccessUnit {
            data,
            is_keyframe,
            pts_100ns,
            duration_100ns,
        }))
    }
}

struct GpuBgraToNv12Converter {
    device: ID3D11Device,
    immediate_context: ID3D11DeviceContext,
    video_device: ID3D11VideoDevice,
    video_context: ID3D11VideoContext,
    enumerator: ID3D11VideoProcessorEnumerator,
    processor: ID3D11VideoProcessor,
    width: u32,
    height: u32,
}

impl GpuBgraToNv12Converter {
    fn new(
        device: &ID3D11Device,
        config: &VideoEncoderConfig,
    ) -> Result<Self, EncodeError> {
        let immediate_context = unsafe {
            device
                .GetImmediateContext()
                .map_err(|error| map_windows_error("ID3D11Device::GetImmediateContext", error))?
        };
        let video_device: ID3D11VideoDevice = device
            .cast()
            .map_err(|error| map_windows_error("ID3D11Device::cast(ID3D11VideoDevice)", error))?;
        let video_context: ID3D11VideoContext = immediate_context
            .cast()
            .map_err(|error| map_windows_error("ID3D11DeviceContext::cast(ID3D11VideoContext)", error))?;

        let content_desc = D3D11_VIDEO_PROCESSOR_CONTENT_DESC {
            InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE,
            InputFrameRate: DXGI_RATIONAL {
                Numerator: config.frame_rate_num,
                Denominator: config.frame_rate_den,
            },
            InputWidth: config.width,
            InputHeight: config.height,
            OutputFrameRate: DXGI_RATIONAL {
                Numerator: config.frame_rate_num,
                Denominator: config.frame_rate_den,
            },
            OutputWidth: config.width,
            OutputHeight: config.height,
            Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED,
        };
        let enumerator = unsafe {
            video_device
                .CreateVideoProcessorEnumerator(&content_desc)
                .map_err(|error| map_windows_error("ID3D11VideoDevice::CreateVideoProcessorEnumerator", error))?
        };
        let processor = unsafe {
            video_device
                .CreateVideoProcessor(&enumerator, 0)
                .map_err(|error| map_windows_error("ID3D11VideoDevice::CreateVideoProcessor", error))?
        };

        Ok(Self {
            device: device.clone(),
            immediate_context,
            video_device,
            video_context,
            enumerator,
            processor,
            width: config.width,
            height: config.height,
        })
    }

    fn convert(
        &self,
        input_texture: &ID3D11Texture2D,
    ) -> Result<ID3D11Texture2D, EncodeError> {
        let mut input_desc = D3D11_TEXTURE2D_DESC::default();
        unsafe {
            input_texture.GetDesc(&mut input_desc);
        }
        if input_desc.Width != self.width
            || input_desc.Height != self.height
            || input_desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM
        {
            return Err(EncodeError::Bitstream(
                "captured texture format does not match the expected BGRA frame layout"
                    .to_owned(),
            ));
        }

        let output_texture = create_nv12_texture(&self.device, self.width, self.height)?;
        let input_resource: ID3D11Resource = input_texture
            .cast()
            .map_err(|error| map_windows_error("ID3D11Texture2D::cast(ID3D11Resource)", error))?;
        let output_resource: ID3D11Resource = output_texture
            .cast()
            .map_err(|error| map_windows_error("ID3D11Texture2D::cast(ID3D11Resource)", error))?;

        let input_view_desc = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC {
            FourCC: 0,
            ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPIV {
                    MipSlice: 0,
                    ArraySlice: 0,
                },
            },
        };
        let output_view_desc = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC {
            ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D,
            Anonymous: D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC_0 {
                Texture2D: D3D11_TEX2D_VPOV { MipSlice: 0 },
            },
        };

        let mut input_view = None;
        unsafe {
            self.video_device
                .CreateVideoProcessorInputView(
                    &input_resource,
                    &self.enumerator,
                    &input_view_desc,
                    Some(&mut input_view),
                )
                .map_err(|error| map_windows_error("ID3D11VideoDevice::CreateVideoProcessorInputView", error))?;
        }
        let input_view = input_view.ok_or_else(|| {
            EncodeError::Bitstream(
                "video processor did not return an input view".to_owned(),
            )
        })?;

        let mut output_view = None;
        unsafe {
            self.video_device
                .CreateVideoProcessorOutputView(
                    &output_resource,
                    &self.enumerator,
                    &output_view_desc,
                    Some(&mut output_view),
                )
                .map_err(|error| map_windows_error("ID3D11VideoDevice::CreateVideoProcessorOutputView", error))?;
        }
        let output_view = output_view.ok_or_else(|| {
            EncodeError::Bitstream(
                "video processor did not return an output view".to_owned(),
            )
        })?;

        let full_rect = RECT {
            left: 0,
            top: 0,
            right: self.width as i32,
            bottom: self.height as i32,
        };
        unsafe {
            self.video_context.VideoProcessorSetStreamSourceRect(
                &self.processor,
                0,
                true,
                Some(&full_rect),
            );
            self.video_context.VideoProcessorSetStreamDestRect(
                &self.processor,
                0,
                true,
                Some(&full_rect),
            );
            self.video_context.VideoProcessorSetOutputTargetRect(
                &self.processor,
                true,
                Some(&full_rect),
            );
            self.video_context.VideoProcessorSetStreamOutputRate(
                &self.processor,
                0,
                D3D11_VIDEO_PROCESSOR_OUTPUT_RATE_NORMAL,
                false,
                None,
            );
        }

        let stream = D3D11_VIDEO_PROCESSOR_STREAM {
            Enable: true.into(),
            OutputIndex: 0,
            InputFrameOrField: 0,
            PastFrames: 0,
            FutureFrames: 0,
            ppPastSurfaces: ptr::null_mut(),
            pInputSurface: ManuallyDrop::new(Some(input_view)),
            ppFutureSurfaces: ptr::null_mut(),
            ppPastSurfacesRight: ptr::null_mut(),
            pInputSurfaceRight: ManuallyDrop::new(None),
            ppFutureSurfacesRight: ptr::null_mut(),
        };
        unsafe {
            self.video_context
                .VideoProcessorBlt(&self.processor, &output_view, 0, &[stream])
                .map_err(|error| map_windows_error("ID3D11VideoContext::VideoProcessorBlt", error))?;
            self.immediate_context.Flush();
        }

        Ok(output_texture)
    }
}

fn create_hardware_encoder_activate() -> Result<IMFActivate, EncodeError> {
    let input_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_NV12,
    };
    let output_type = MFT_REGISTER_TYPE_INFO {
        guidMajorType: MFMediaType_Video,
        guidSubtype: MFVideoFormat_H264,
    };

    let mut activates_ptr: *mut Option<IMFActivate> = ptr::null_mut();
    let mut activate_count = 0u32;
    unsafe {
        MFTEnumEx(
            MFT_CATEGORY_VIDEO_ENCODER,
            MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
            Some(&input_type),
            Some(&output_type),
            &mut activates_ptr,
            &mut activate_count,
        )
        .map_err(|error| map_windows_error("MFTEnumEx", error))?;
    }

    if activate_count == 0 || activates_ptr.is_null() {
        return Err(EncodeError::HardwareEncoderUnavailable);
    }

    let activates = unsafe {
        slice::from_raw_parts(activates_ptr, activate_count as usize)
            .iter()
            .filter_map(|activate| activate.clone())
            .collect::<Vec<_>>()
    };
    unsafe {
        CoTaskMemFree(Some(activates_ptr.cast()));
    }

    activates
        .into_iter()
        .next()
        .ok_or(EncodeError::HardwareEncoderUnavailable)
}

fn configure_output_type(
    transform: &IMFTransform,
    config: &VideoEncoderConfig,
) -> Result<(), EncodeError> {
    let output_type = unsafe {
        MFCreateMediaType()
            .map_err(|error| map_windows_error("MFCreateMediaType", error))?
    };
    set_video_type_common_attributes(&output_type, config, MFVideoFormat_H264)?;
    unsafe {
        output_type
            .SetUINT32(&MF_MT_AVG_BITRATE, config.bitrate_bps)
            .map_err(|error| map_windows_error("IMFMediaType::SetUINT32(MF_MT_AVG_BITRATE)", error))?;
        output_type
            .SetUINT32(
                &MF_MT_MPEG2_PROFILE,
                profile_to_mf_profile(config.profile),
            )
            .map_err(|error| map_windows_error("IMFMediaType::SetUINT32(MF_MT_MPEG2_PROFILE)", error))?;
        transform
            .SetOutputType(OUTPUT_STREAM_ID, &output_type, 0)
            .map_err(|error| map_windows_error("IMFTransform::SetOutputType", error))?;
    }
    Ok(())
}

fn configure_input_type(
    transform: &IMFTransform,
    config: &VideoEncoderConfig,
) -> Result<(), EncodeError> {
    let input_type = unsafe {
        MFCreateMediaType()
            .map_err(|error| map_windows_error("MFCreateMediaType", error))?
    };
    set_video_type_common_attributes(&input_type, config, MFVideoFormat_NV12)?;
    unsafe {
        transform
            .SetInputType(INPUT_STREAM_ID, &input_type, 0)
            .map_err(|error| map_windows_error("IMFTransform::SetInputType", error))?;
    }
    Ok(())
}

fn set_video_type_common_attributes(
    media_type: &IMFMediaType,
    config: &VideoEncoderConfig,
    subtype: windows::core::GUID,
) -> Result<(), EncodeError> {
    unsafe {
        media_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .map_err(|error| map_windows_error("IMFMediaType::SetGUID(MF_MT_MAJOR_TYPE)", error))?;
        media_type
            .SetGUID(&MF_MT_SUBTYPE, &subtype)
            .map_err(|error| map_windows_error("IMFMediaType::SetGUID(MF_MT_SUBTYPE)", error))?;
        media_type
            .SetUINT64(&MF_MT_FRAME_SIZE, pack_ratio_u64(config.width, config.height))
            .map_err(|error| map_windows_error("IMFMediaType::SetUINT64(MF_MT_FRAME_SIZE)", error))?;
        media_type
            .SetUINT64(
                &MF_MT_FRAME_RATE,
                pack_ratio_u64(config.frame_rate_num, config.frame_rate_den),
            )
            .map_err(|error| map_windows_error("IMFMediaType::SetUINT64(MF_MT_FRAME_RATE)", error))?;
        media_type
            .SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_ratio_u64(1, 1))
            .map_err(|error| map_windows_error("IMFMediaType::SetUINT64(MF_MT_PIXEL_ASPECT_RATIO)", error))?;
        media_type
            .SetUINT32(
                &MF_MT_INTERLACE_MODE,
                MFVideoInterlace_Progressive.0 as u32,
            )
            .map_err(|error| map_windows_error("IMFMediaType::SetUINT32(MF_MT_INTERLACE_MODE)", error))?;
    }
    Ok(())
}

fn configure_codec_properties(
    transform: &IMFTransform,
    config: &VideoEncoderConfig,
) -> Result<(), EncodeError> {
    let codec_api: ICodecAPI = transform
        .cast()
        .map_err(|error| map_windows_error("IMFTransform::cast(ICodecAPI)", error))?;
    try_set_codec_api_u32(
        &codec_api,
        &CODECAPI_AVEncCommonRateControlMode,
        eAVEncCommonRateControlMode_CBR.0 as u32,
    )?;
    try_set_codec_api_u32(
        &codec_api,
        &CODECAPI_AVEncCommonMeanBitRate,
        config.bitrate_bps,
    )?;
    try_set_codec_api_u32(
        &codec_api,
        &CODECAPI_AVEncMPVGOPSize,
        config.gop_size()?,
    )?;
    try_set_codec_api_u32(
        &codec_api,
        &CODECAPI_AVEncMPVDefaultBPictureCount,
        0,
    )?;
    try_set_codec_api_bool(
        &codec_api,
        &CODECAPI_AVLowLatencyMode,
        config.low_latency,
    )?;
    Ok(())
}

fn create_dxgi_device_manager(
    device: &ID3D11Device,
) -> Result<IMFDXGIDeviceManager, EncodeError> {
    let mut reset_token = 0u32;
    let mut manager = None;
    unsafe {
        MFCreateDXGIDeviceManager(&mut reset_token, &mut manager)
            .map_err(|error| map_windows_error("MFCreateDXGIDeviceManager", error))?;
    }
    let manager = manager.ok_or_else(|| {
        EncodeError::Bitstream(
            "Media Foundation did not return a DXGI device manager".to_owned(),
        )
    })?;
    let device_unknown: IUnknown = device
        .cast()
        .map_err(|error| map_windows_error("ID3D11Device::cast(IUnknown)", error))?;
    unsafe {
        manager
            .ResetDevice(&device_unknown, reset_token)
            .map_err(|error| map_windows_error("IMFDXGIDeviceManager::ResetDevice", error))?;
    }
    Ok(manager)
}

fn create_input_sample(
    nv12_texture: &ID3D11Texture2D,
    pts_100ns: i64,
    duration_100ns: i64,
) -> Result<IMFSample, EncodeError> {
    let sample = unsafe {
        MFCreateSample().map_err(|error| map_windows_error("MFCreateSample", error))?
    };
    let buffer = unsafe {
        MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, nv12_texture, 0, false)
            .map_err(|error| map_windows_error("MFCreateDXGISurfaceBuffer", error))?
    };
    unsafe {
        sample
            .AddBuffer(&buffer)
            .map_err(|error| map_windows_error("IMFSample::AddBuffer", error))?;
        sample
            .SetSampleTime(pts_100ns)
            .map_err(|error| map_windows_error("IMFSample::SetSampleTime", error))?;
        sample
            .SetSampleDuration(duration_100ns)
            .map_err(|error| map_windows_error("IMFSample::SetSampleDuration", error))?;
    }
    Ok(sample)
}

fn create_output_sample(
    max_length: u32,
) -> Result<IMFSample, EncodeError> {
    let sample = unsafe {
        MFCreateSample().map_err(|error| map_windows_error("MFCreateSample", error))?
    };
    let buffer = unsafe {
        MFCreateMemoryBuffer(max_length)
            .map_err(|error| map_windows_error("MFCreateMemoryBuffer", error))?
    };
    unsafe {
        sample
            .AddBuffer(&buffer)
            .map_err(|error| map_windows_error("IMFSample::AddBuffer", error))?;
    }
    Ok(sample)
}

fn create_nv12_texture(
    device: &ID3D11Device,
    width: u32,
    height: u32,
) -> Result<ID3D11Texture2D, EncodeError> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: (D3D11_BIND_RENDER_TARGET | D3D11_BIND_SHADER_RESOURCE).0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: D3D11_RESOURCE_MISC_FLAG(0).0 as u32,
    };

    let mut texture = None;
    unsafe {
        device
            .CreateTexture2D(&desc, None, Some(&mut texture))
            .map_err(|error| map_windows_error("ID3D11Device::CreateTexture2D", error))?;
    }
    texture.ok_or_else(|| {
        EncodeError::Bitstream(
            "D3D11 did not return an NV12 texture".to_owned(),
        )
    })
}

fn read_sequence_header(
    transform: &IMFTransform,
) -> Result<(usize, Vec<u8>), EncodeError> {
    let media_type = unsafe {
        transform
            .GetOutputCurrentType(OUTPUT_STREAM_ID)
            .map_err(|error| map_windows_error("IMFTransform::GetOutputCurrentType", error))?
    };
    let blob_size = unsafe { media_type.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER) }
        .map_err(|_| EncodeError::MissingSequenceHeader)?;
    let mut blob = vec![0u8; blob_size as usize];
    unsafe {
        media_type
            .GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut blob, None)
            .map_err(|error| map_windows_error("IMFMediaType::GetBlob(MF_MT_MPEG_SEQUENCE_HEADER)", error))?;
    }
    h264_sequence_header_to_annex_b(&blob)
}

fn copy_buffer_bytes(
    buffer: &windows::Win32::Media::MediaFoundation::IMFMediaBuffer,
) -> Result<Vec<u8>, EncodeError> {
    let mut pointer = ptr::null_mut();
    let mut max_length = 0u32;
    let mut current_length = 0u32;
    unsafe {
        buffer
            .Lock(&mut pointer, Some(&mut max_length), Some(&mut current_length))
            .map_err(|error| map_windows_error("IMFMediaBuffer::Lock", error))?;
    }
    let bytes = unsafe {
        slice::from_raw_parts(pointer, current_length as usize).to_vec()
    };
    unsafe {
        buffer
            .Unlock()
            .map_err(|error| map_windows_error("IMFMediaBuffer::Unlock", error))?;
    }
    Ok(bytes)
}

fn try_set_codec_api_u32(
    codec_api: &ICodecAPI,
    property: &windows::core::GUID,
    value: u32,
) -> Result<(), EncodeError> {
    if !codec_api_property_is_writable(codec_api, property) {
        return Ok(());
    }

    let mut variant = variant_u32(value);
    match unsafe { codec_api.SetValue(property, &mut variant) } {
        Ok(()) => Ok(()),
        Err(error) if error.code().0 == 0x80070057u32 as i32 => Ok(()),
        Err(error) => Err(map_windows_error("ICodecAPI::SetValue", error)),
    }
}

fn try_set_codec_api_bool(
    codec_api: &ICodecAPI,
    property: &windows::core::GUID,
    value: bool,
) -> Result<(), EncodeError> {
    if !codec_api_property_is_writable(codec_api, property) {
        return Ok(());
    }

    let mut variant = variant_bool(value);
    match unsafe { codec_api.SetValue(property, &mut variant) } {
        Ok(()) => Ok(()),
        Err(error) if error.code().0 == 0x80070057u32 as i32 => Ok(()),
        Err(error) => Err(map_windows_error("ICodecAPI::SetValue", error)),
    }
}

fn codec_api_property_is_writable(
    codec_api: &ICodecAPI,
    property: &windows::core::GUID,
) -> bool {
    unsafe { codec_api.IsSupported(property).is_ok() && codec_api.IsModifiable(property).is_ok() }
}

fn variant_u32(value: u32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_UI4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { ulVal: value },
            }),
        },
    }
}

fn variant_bool(value: bool) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_BOOL,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 {
                    boolVal: if value { VARIANT_TRUE } else { VARIANT_FALSE },
                },
            }),
        },
    }
}

fn take_output_sample(
    output_buffer: &mut MFT_OUTPUT_DATA_BUFFER,
) -> Option<IMFSample> {
    unsafe { ManuallyDrop::take(&mut output_buffer.pSample) }
}

fn drop_output_events(output_buffer: &mut MFT_OUTPUT_DATA_BUFFER) {
    unsafe {
        let _ = ManuallyDrop::take(&mut output_buffer.pEvents);
    }
}

fn profile_to_mf_profile(profile: H264Profile) -> u32 {
    match profile {
        H264Profile::Main => eAVEncH264VProfile_Main.0 as u32,
    }
}

fn map_windows_error(
    operation: &'static str,
    error: WindowsError,
) -> EncodeError {
    EncodeError::WindowsApi {
        operation,
        code: error.code().0,
        message: error.message().to_string(),
    }
}

struct ComGuard {
    should_uninitialize: bool,
}

impl ComGuard {
    fn new() -> Result<Self, EncodeError> {
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr.is_ok() {
            return Ok(Self {
                should_uninitialize: true,
            });
        }
        if hr == RPC_E_CHANGED_MODE {
            return Ok(Self {
                should_uninitialize: false,
            });
        }
        Err(EncodeError::WindowsApi {
            operation: "CoInitializeEx",
            code: hr.0,
            message: format!("COM initialization failed with 0x{:08x}", hr.0),
        })
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.should_uninitialize {
            unsafe {
                CoUninitialize();
            }
        }
    }
}

struct MediaFoundationGuard;

impl MediaFoundationGuard {
    fn new() -> Result<Self, EncodeError> {
        unsafe {
            MFStartup(MF_VERSION, MFSTARTUP_FULL)
                .map_err(|error| map_windows_error("MFStartup", error))?;
        }
        Ok(Self)
    }
}

impl Drop for MediaFoundationGuard {
    fn drop(&mut self) {
        let _ = unsafe { MFShutdown() };
    }
}
