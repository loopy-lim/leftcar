use super::{Monitor, WindowsSession};
use crate::wire;
use std::mem::ManuallyDrop;
use std::net::UdpSocket;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use windows::core::{factory, Interface};
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Win32::Foundation::{HMODULE, VARIANT_TRUE};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Variant::{
    VARENUM, VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VT_BOOL, VT_UI4,
};
use windows::Win32::System::WinRT::Direct3D11::{
    CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess,
};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};

pub(super) fn run(
    monitor: Monitor,
    width: u32,
    height: u32,
    fps: u32,
    socket: UdpSocket,
    session: Arc<WindowsSession>,
) -> Result<(), String> {
    if !width.is_multiple_of(2) || !height.is_multiple_of(2) {
        return Err("Media Foundation NV12 dimensions must be even".into());
    }
    let _winrt = WinRtGuard::new()?;
    let _media_foundation = MfGuard::new()?;
    let (device, context, runtime_device) = create_d3d_device()?;
    let item = capture_item(windows::Win32::Graphics::Gdi::HMONITOR(
        monitor.handle as *mut _,
    ))?;
    let mut content_size = item.Size().map_err(win("read WGC item size"))?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &runtime_device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        2,
        content_size,
    )
    .map_err(win("create WGC free-threaded frame pool"))?;
    let capture = pool
        .CreateCaptureSession(&item)
        .map_err(win("create WGC capture session"))?;
    capture
        .StartCapture()
        .map_err(win("start Windows Graphics Capture"))?;

    let bitrate = session.stats.lock().unwrap().current_bitrate;
    let mut encoder = HardwareH264Encoder::new(width, height, fps, bitrate)?;
    let started = Instant::now();
    let mut staging: Option<(ID3D11Texture2D, u32, u32)> = None;
    let mut au_id = 0u16;
    let mut config_sent = false;
    if let Some(config) = wire::config_datagram(encoder.parameter_sets()) {
        send_packet(&socket, &config, &session, false)?;
        config_sent = true;
    }
    let mut last_frame = None;
    let mut interval_started = Instant::now();
    let mut interval_frames = 0u32;
    let mut interval_bytes = 0u64;

    while !session.stop.load(Ordering::Acquire) {
        let frame = match pool.TryGetNextFrame() {
            Ok(frame) => frame,
            Err(_) => {
                if started.elapsed() > Duration::from_secs(5)
                    && session.stats.lock().unwrap().frames == 0
                {
                    return Err(
                        "Windows Graphics Capture produced no frame within 5 seconds".into(),
                    );
                }
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
        };
        let capture_started = Instant::now();
        let next_size = frame.ContentSize().map_err(win("read WGC frame size"))?;
        if next_size.Width != content_size.Width || next_size.Height != content_size.Height {
            content_size = next_size;
            pool.Recreate(
                &runtime_device,
                DirectXPixelFormat::B8G8R8A8UIntNormalized,
                2,
                content_size,
            )
            .map_err(win("recreate WGC pool after display resize"))?;
            staging = None;
            frame.Close().ok();
            continue;
        }
        let surface = frame.Surface().map_err(win("read WGC frame surface"))?;
        let access: IDirect3DDxgiInterfaceAccess =
            surface.cast().map_err(win("cast WGC surface interop"))?;
        let texture: ID3D11Texture2D =
            unsafe { access.GetInterface() }.map_err(win("obtain D3D11 texture from WGC frame"))?;
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe { texture.GetDesc(&mut desc) };
        let needs_staging = staging
            .as_ref()
            .is_none_or(|(_, w, h)| *w != desc.Width || *h != desc.Height);
        if needs_staging {
            let mut staging_desc = desc;
            staging_desc.Usage = D3D11_USAGE_STAGING;
            staging_desc.BindFlags = 0;
            staging_desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            staging_desc.MiscFlags = 0;
            let mut texture = None;
            unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut texture)) }
                .map_err(win("create WGC CPU readback texture"))?;
            staging = Some((
                texture.ok_or("D3D11 returned no staging texture")?,
                desc.Width,
                desc.Height,
            ));
        }
        let (staging_texture, source_width, source_height) = staging.as_ref().unwrap();
        unsafe { context.CopyResource(staging_texture, &texture) };
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        unsafe { context.Map(staging_texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }
            .map_err(win("map WGC readback texture"))?;
        let nv12 = unsafe {
            bgra_to_nv12(
                mapped.pData.cast(),
                mapped.RowPitch as usize,
                *source_width,
                *source_height,
                width,
                height,
            )
        };
        unsafe { context.Unmap(staging_texture, 0) };
        frame.Close().ok();

        let capture_to_encode_us = capture_started.elapsed().as_micros() as u64;
        if session.force_keyframe.swap(false, Ordering::AcqRel) {
            encoder.force_keyframe();
        }
        let encode_started = Instant::now();
        let outputs = encoder.encode(&nv12)?;
        let encode_us = encode_started.elapsed().as_micros() as u64;
        if outputs.is_empty() {
            continue;
        }

        if session.stats.lock().unwrap().first_capture_ms == 0 {
            session.stats.lock().unwrap().first_capture_ms = started.elapsed().as_millis() as u64;
        }
        for encoded in outputs {
            let Some(annex_b) = wire::normalize_h264(&encoded) else {
                return Err("Media Foundation emitted malformed H.264".into());
            };
            let parameter_sets = wire::h264_parameter_sets(&annex_b);
            let keyframe = contains_idr(&annex_b);
            if (!config_sent || keyframe) && !parameter_sets.is_empty() {
                if let Some(config) = wire::config_datagram(&parameter_sets) {
                    send_packet(&socket, &config, &session, false)?;
                    config_sent = true;
                }
            }
            // Do not send an undecodable access unit before SPS/PPS are known.
            if !config_sent {
                session.force_keyframe.store(true, Ordering::Release);
                continue;
            }
            au_id = au_id.wrapping_add(1);
            let wall_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let datagrams = wire::media_datagrams(au_id, wall_ms, &annex_b);
            if datagrams.is_empty() {
                return Err("encoded H.264 access unit exceeds Leftcar fragment limit".into());
            }
            let send_started = Instant::now();
            let mut bytes = 0usize;
            for datagram in datagrams {
                send_packet(&socket, &datagram, &session, true)?;
                bytes += datagram.len();
            }
            let send_us = send_started.elapsed().as_micros() as u64;
            interval_frames += 1;
            interval_bytes += bytes as u64;
            update_stats(
                &session,
                started,
                last_frame.replace(Instant::now()),
                capture_to_encode_us,
                encode_us,
                send_us,
                bytes,
            );
        }
        if interval_started.elapsed() >= Duration::from_secs(1) {
            let elapsed = interval_started.elapsed().as_secs_f64();
            let mut stats = session.stats.lock().unwrap();
            stats.fps = (interval_frames as f64 / elapsed).round() as u32;
            stats.kbps = (interval_bytes as f64 * 8.0 / 1_000.0 / elapsed).round() as u32;
            interval_frames = 0;
            interval_bytes = 0;
            interval_started = Instant::now();
        }
    }
    capture.Close().ok();
    pool.Close().ok();
    session.stats.lock().unwrap().state = "stopped".into();
    Ok(())
}

fn create_d3d_device() -> Result<(ID3D11Device, ID3D11DeviceContext, IDirect3DDevice), String> {
    let mut device = None;
    let mut context = None;
    let flags = D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            flags,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None::<*mut D3D_FEATURE_LEVEL>,
            Some(&mut context),
        )
    }
    .map_err(win("create D3D11 hardware device"))?;
    let device = device.ok_or("D3D11 returned no device")?;
    let context = context.ok_or("D3D11 returned no immediate context")?;
    let dxgi: IDXGIDevice = device.cast().map_err(win("cast D3D11 device to DXGI"))?;
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }
        .map_err(win("create WinRT D3D11 device"))?;
    let runtime_device: IDirect3DDevice =
        inspectable.cast().map_err(win("cast WinRT D3D11 device"))?;
    Ok((device, context, runtime_device))
}

fn capture_item(
    monitor: windows::Win32::Graphics::Gdi::HMONITOR,
) -> Result<GraphicsCaptureItem, String> {
    let interop: IGraphicsCaptureItemInterop =
        factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
            .map_err(win("open WGC desktop interop factory"))?;
    unsafe { interop.CreateForMonitor(monitor) }.map_err(win("create WGC item for monitor"))
}

struct HardwareH264Encoder {
    transform: IMFTransform,
    event_generator: Option<IMFMediaEventGenerator>,
    pending_need_input: u32,
    output_info: MFT_OUTPUT_STREAM_INFO,
    frame_duration: i64,
    frame_index: i64,
    force_keyframe: bool,
    parameter_sets: Vec<Vec<u8>>,
}

impl HardwareH264Encoder {
    fn new(width: u32, height: u32, fps: u32, bitrate: u32) -> Result<Self, String> {
        let input_info = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_NV12,
        };
        let output_info = MFT_REGISTER_TYPE_INFO {
            guidMajorType: MFMediaType_Video,
            guidSubtype: MFVideoFormat_H264,
        };
        let mut activates: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0;
        unsafe {
            MFTEnumEx(
                MFT_CATEGORY_VIDEO_ENCODER,
                MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER,
                Some(&input_info),
                Some(&output_info),
                &mut activates,
                &mut count,
            )
        }
        .map_err(win("enumerate Media Foundation hardware H.264 encoders"))?;
        if count == 0 || activates.is_null() {
            return Err("no Media Foundation hardware H.264 encoder is available".into());
        }
        let activation_slice = unsafe { std::slice::from_raw_parts_mut(activates, count as usize) };
        let activate = activation_slice[0].take();
        for unused in &mut activation_slice[1..] {
            drop(unused.take());
        }
        unsafe { CoTaskMemFree(Some(activates.cast())) };
        let activate = activate.ok_or("Media Foundation returned an empty encoder activation")?;
        let transform: IMFTransform = unsafe { activate.ActivateObject() }
            .map_err(win("activate Media Foundation hardware H.264 encoder"))?;

        let output_type = media_type(MFVideoFormat_H264, width, height, fps, Some(bitrate))?;
        let input_type = media_type(MFVideoFormat_NV12, width, height, fps, None)?;
        unsafe {
            // Microsoft H.264 encoder requires output before input.
            transform.SetOutputType(0, &output_type, 0)
        }
        .map_err(win("set Media Foundation H.264 output type"))?;
        unsafe { transform.SetInputType(0, &input_type, 0) }
            .map_err(win("set Media Foundation NV12 input type"))?;
        let parameter_sets = sequence_parameter_sets(&output_type);

        let attributes = unsafe { transform.GetAttributes() }
            .map_err(win("read Media Foundation encoder attributes"))?;
        let asynchronous = unsafe { attributes.GetUINT32(&MF_TRANSFORM_ASYNC) }.unwrap_or(0) != 0;
        if asynchronous {
            unsafe { attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1) }
                .map_err(win("unlock asynchronous hardware encoder"))?;
        }
        configure_codec_api(&transform, bitrate);
        unsafe { transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0) }
            .map_err(win("flush Media Foundation encoder"))?;
        unsafe { transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0) }
            .map_err(win("begin Media Foundation encoder streaming"))?;
        unsafe { transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0) }
            .map_err(win("start Media Foundation encoder stream"))?;
        let output_info = unsafe { transform.GetOutputStreamInfo(0) }
            .map_err(win("read Media Foundation output stream info"))?;
        let event_generator = if asynchronous {
            Some(
                transform
                    .cast::<IMFMediaEventGenerator>()
                    .map_err(win("open asynchronous Media Foundation event queue"))?,
            )
        } else {
            None
        };
        Ok(Self {
            transform,
            event_generator,
            pending_need_input: 0,
            output_info,
            frame_duration: 10_000_000 / i64::from(fps),
            frame_index: 0,
            force_keyframe: true,
            parameter_sets,
        })
    }

    fn parameter_sets(&self) -> &[Vec<u8>] {
        &self.parameter_sets
    }

    fn force_keyframe(&mut self) {
        self.force_keyframe = true;
    }

    fn encode(&mut self, nv12: &[u8]) -> Result<Vec<Vec<u8>>, String> {
        let mut output = Vec::new();
        if self.event_generator.is_some() {
            self.pump_events(&mut output)?;
            // Async hardware MFTs explicitly grant input slots. A busy encoder
            // drops this captured frame instead of blocking WGC or session
            // shutdown while waiting on the Media Foundation event queue.
            if self.pending_need_input == 0 {
                return Ok(output);
            }
            self.pending_need_input -= 1;
        }
        if self.force_keyframe {
            if let Ok(codec) = self.transform.cast::<ICodecAPI>() {
                let value = variant_bool(true);
                unsafe { codec.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &value) }.ok();
            }
            self.force_keyframe = false;
        }
        let buffer = unsafe { MFCreateMemoryBuffer(nv12.len() as u32) }
            .map_err(win("allocate Media Foundation NV12 buffer"))?;
        let mut pointer = std::ptr::null_mut();
        unsafe { buffer.Lock(&mut pointer, None, None) }
            .map_err(win("lock Media Foundation NV12 buffer"))?;
        unsafe { std::ptr::copy_nonoverlapping(nv12.as_ptr(), pointer, nv12.len()) };
        unsafe { buffer.Unlock() }.map_err(win("unlock Media Foundation NV12 buffer"))?;
        unsafe { buffer.SetCurrentLength(nv12.len() as u32) }
            .map_err(win("commit Media Foundation NV12 buffer"))?;
        let sample = unsafe { MFCreateSample() }.map_err(win("create Media Foundation sample"))?;
        unsafe { sample.AddBuffer(&buffer) }.map_err(win("attach NV12 sample buffer"))?;
        unsafe { sample.SetSampleTime(self.frame_index * self.frame_duration) }
            .map_err(win("set NV12 sample time"))?;
        unsafe { sample.SetSampleDuration(self.frame_duration) }
            .map_err(win("set NV12 sample duration"))?;
        unsafe { self.transform.ProcessInput(0, &sample, 0) }
            .map_err(win("submit NV12 frame to Media Foundation encoder"))?;
        self.frame_index += 1;

        if self.event_generator.is_some() {
            self.pump_events(&mut output)?;
        } else {
            self.drain_output(&mut output)?;
        }
        Ok(output)
    }

    fn pump_events(&mut self, output: &mut Vec<Vec<u8>>) -> Result<(), String> {
        loop {
            let generator = self.event_generator.as_ref().unwrap();
            let event = match unsafe { generator.GetEvent(MF_EVENT_FLAG_NO_WAIT) } {
                Ok(event) => event,
                // MF_E_NO_EVENTS_AVAILABLE is the expected queue-empty result.
                // Some vendor MFTs use a different empty HRESULT, so any
                // nonblocking miss ends this pump; real failures surface in
                // the status of an event that was actually returned.
                Err(_) => return Ok(()),
            };
            let status = unsafe { event.GetStatus() }.map_err(win("read encoder event status"))?;
            status.ok().map_err(win("hardware encoder event failed"))?;
            match unsafe { event.GetType() }.map_err(win("read encoder event type"))? as i32 {
                event_type if event_type == METransformNeedInput.0 => self.pending_need_input += 1,
                event_type if event_type == METransformHaveOutput.0 => self.drain_output(output)?,
                _ => {}
            }
        }
    }

    fn drain_output(&mut self, output: &mut Vec<Vec<u8>>) -> Result<(), String> {
        loop {
            let provides = self.output_info.dwFlags
                & (MFT_OUTPUT_STREAM_PROVIDES_SAMPLES.0 as u32
                    | MFT_OUTPUT_STREAM_CAN_PROVIDE_SAMPLES.0 as u32)
                != 0;
            let supplied = if provides {
                None
            } else {
                let sample = unsafe { MFCreateSample() }
                    .map_err(win("create Media Foundation output sample"))?;
                let capacity = self.output_info.cbSize.max(1_048_576);
                let buffer = unsafe { MFCreateMemoryBuffer(capacity) }
                    .map_err(win("allocate Media Foundation output buffer"))?;
                unsafe { sample.AddBuffer(&buffer) }
                    .map_err(win("attach Media Foundation output buffer"))?;
                Some(sample)
            };
            let mut buffer = MFT_OUTPUT_DATA_BUFFER {
                dwStreamID: 0,
                pSample: ManuallyDrop::new(supplied),
                dwStatus: 0,
                pEvents: ManuallyDrop::new(None),
            };
            let mut status = 0;
            let result = unsafe {
                self.transform
                    .ProcessOutput(0, std::slice::from_mut(&mut buffer), &mut status)
            };
            if let Err(error) = result {
                unsafe {
                    ManuallyDrop::drop(&mut buffer.pSample);
                    ManuallyDrop::drop(&mut buffer.pEvents);
                }
                if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT {
                    return Ok(());
                }
                return Err(format!("read Media Foundation H.264 output: {error}"));
            }
            let sample = unsafe { ManuallyDrop::take(&mut buffer.pSample) };
            let events = unsafe { ManuallyDrop::take(&mut buffer.pEvents) };
            drop(events);
            if let Some(sample) = sample {
                let contiguous = unsafe { sample.ConvertToContiguousBuffer() }
                    .map_err(win("join Media Foundation H.264 buffers"))?;
                let length = unsafe { contiguous.GetCurrentLength() }
                    .map_err(win("read Media Foundation H.264 length"))?;
                let mut pointer = std::ptr::null_mut();
                unsafe { contiguous.Lock(&mut pointer, None, None) }
                    .map_err(win("lock Media Foundation H.264 output"))?;
                let bytes =
                    unsafe { std::slice::from_raw_parts(pointer, length as usize) }.to_vec();
                unsafe { contiguous.Unlock() }.map_err(win("unlock H.264 output"))?;
                if !bytes.is_empty() {
                    output.push(bytes);
                }
            }
            if buffer.dwStatus & MFT_OUTPUT_DATA_BUFFER_INCOMPLETE.0 as u32 == 0 {
                return Ok(());
            }
        }
    }
}

impl Drop for HardwareH264Encoder {
    fn drop(&mut self) {
        unsafe {
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0)
                .ok();
            self.transform
                .ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0)
                .ok();
        }
    }
}

fn media_type(
    subtype: windows::core::GUID,
    width: u32,
    height: u32,
    fps: u32,
    bitrate: Option<u32>,
) -> Result<IMFMediaType, String> {
    let media_type = unsafe { MFCreateMediaType() }.map_err(win("create Media Foundation type"))?;
    unsafe { media_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video) }
        .map_err(win("set Media Foundation major type"))?;
    unsafe { media_type.SetGUID(&MF_MT_SUBTYPE, &subtype) }
        .map_err(win("set Media Foundation subtype"))?;
    unsafe {
        media_type.SetUINT64(
            &MF_MT_FRAME_SIZE,
            (u64::from(width) << 32) | u64::from(height),
        )
    }
    .map_err(win("set Media Foundation frame size"))?;
    unsafe { media_type.SetUINT64(&MF_MT_FRAME_RATE, (u64::from(fps) << 32) | 1) }
        .map_err(win("set Media Foundation frame rate"))?;
    unsafe { media_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, (1u64 << 32) | 1) }
        .map_err(win("set Media Foundation pixel aspect ratio"))?;
    unsafe { media_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32) }
        .map_err(win("set Media Foundation progressive scan"))?;
    if let Some(bitrate) = bitrate {
        unsafe { media_type.SetUINT32(&MF_MT_AVG_BITRATE, bitrate) }
            .map_err(win("set Media Foundation bitrate"))?;
    }
    Ok(media_type)
}

fn sequence_parameter_sets(media_type: &IMFMediaType) -> Vec<Vec<u8>> {
    let Ok(size) = (unsafe { media_type.GetBlobSize(&MF_MT_MPEG_SEQUENCE_HEADER) }) else {
        return Vec::new();
    };
    let mut bytes = vec![0u8; size as usize];
    if unsafe { media_type.GetBlob(&MF_MT_MPEG_SEQUENCE_HEADER, &mut bytes, None) }.is_err() {
        return Vec::new();
    }
    if let Some(annex_b) = wire::normalize_h264(&bytes) {
        let sets = wire::h264_parameter_sets(&annex_b);
        if !sets.is_empty() {
            return sets;
        }
    }
    // MF may expose an ISO/IEC 14496-15 AVCDecoderConfigurationRecord instead
    // of Annex-B. Extract SPS and PPS without forwarding the avcC metadata.
    wire::avcc_parameter_sets(&bytes).unwrap_or_default()
}

fn configure_codec_api(transform: &IMFTransform, bitrate: u32) {
    let Ok(codec) = transform.cast::<ICodecAPI>() else {
        return;
    };
    let low_latency = variant_bool(true);
    let bitrate_value = variant_u32(bitrate);
    unsafe {
        codec
            .SetValue(&CODECAPI_AVLowLatencyMode, &low_latency)
            .ok();
        codec
            .SetValue(&CODECAPI_AVEncCommonMeanBitRate, &bitrate_value)
            .ok();
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
                    boolVal: if value {
                        VARIANT_TRUE
                    } else {
                        Default::default()
                    },
                },
            }),
        },
    }
}

fn variant_u32(value: u32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VARENUM(VT_UI4.0),
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { ulVal: value },
            }),
        },
    }
}

unsafe fn bgra_to_nv12(
    source: *const u8,
    source_stride: usize,
    source_width: u32,
    source_height: u32,
    output_width: u32,
    output_height: u32,
) -> Vec<u8> {
    let width = output_width as usize;
    let height = output_height as usize;
    let mut output = vec![0u8; width * height * 3 / 2];
    for y in 0..height {
        let source_y = y * source_height as usize / height;
        for x in 0..width {
            let source_x = x * source_width as usize / width;
            let pixel = unsafe { source.add(source_y * source_stride + source_x * 4) };
            let b = unsafe { *pixel } as i32;
            let g = unsafe { *pixel.add(1) } as i32;
            let r = unsafe { *pixel.add(2) } as i32;
            output[y * width + x] =
                ((66 * r + 129 * g + 25 * b + 128) / 256 + 16).clamp(0, 255) as u8;
        }
    }
    let uv_offset = width * height;
    for y in (0..height).step_by(2) {
        let source_y = y * source_height as usize / height;
        for x in (0..width).step_by(2) {
            let source_x = x * source_width as usize / width;
            let pixel = unsafe { source.add(source_y * source_stride + source_x * 4) };
            let b = unsafe { *pixel } as i32;
            let g = unsafe { *pixel.add(1) } as i32;
            let r = unsafe { *pixel.add(2) } as i32;
            output[uv_offset + (y / 2) * width + x] =
                ((-38 * r - 74 * g + 112 * b + 128) / 256 + 128).clamp(0, 255) as u8;
            output[uv_offset + (y / 2) * width + x + 1] =
                ((112 * r - 94 * g - 18 * b + 128) / 256 + 128).clamp(0, 255) as u8;
        }
    }
    output
}

fn contains_idr(annex_b: &[u8]) -> bool {
    let mut offset = 0;
    while offset + 4 < annex_b.len() {
        let prefix = if annex_b[offset..].starts_with(&[0, 0, 0, 1]) {
            4
        } else if annex_b[offset..].starts_with(&[0, 0, 1]) {
            3
        } else {
            offset += 1;
            continue;
        };
        if annex_b
            .get(offset + prefix)
            .is_some_and(|byte| byte & 0x1f == 5)
        {
            return true;
        }
        offset += prefix;
    }
    false
}

fn send_packet(
    socket: &UdpSocket,
    packet: &[u8],
    session: &WindowsSession,
    frame: bool,
) -> Result<(), String> {
    match socket.send(packet) {
        Ok(size) if size == packet.len() => Ok(()),
        Ok(size) => Err(format!(
            "partial UDP datagram send: {size}/{}",
            packet.len()
        )),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
            let mut stats = session.stats.lock().unwrap();
            stats.network_dropped += i64::from(frame);
            stats.dropped += i64::from(frame);
            session.force_keyframe.store(true, Ordering::Release);
            Ok(())
        }
        Err(error) => Err(format!("send Windows media datagram: {error}")),
    }
}

#[allow(clippy::too_many_arguments)]
fn update_stats(
    session: &WindowsSession,
    started: Instant,
    previous_frame: Option<Instant>,
    capture_us: u64,
    encode_us: u64,
    send_us: u64,
    bytes: usize,
) {
    let mut stats = session.stats.lock().unwrap();
    stats.state = "running".into();
    stats.frames += 1;
    stats.bytes += bytes as i64;
    stats.capture_to_encode_us = capture_us;
    stats.max_capture_to_encode_us = stats.max_capture_to_encode_us.max(capture_us);
    stats.capture_to_encode_p95_us = stats.capture_to_encode_p95_us.max(capture_us);
    stats.encode_output_us = encode_us;
    stats.max_encode_output_us = stats.max_encode_output_us.max(encode_us);
    stats.encode_output_p95_us = stats.encode_output_p95_us.max(encode_us);
    stats.send_block_us = send_us;
    stats.max_send_block_us = stats.max_send_block_us.max(send_us);
    stats.send_block_p95_us = stats.send_block_p95_us.max(send_us);
    if let Some(previous) = previous_frame {
        let interval = previous.elapsed().as_micros() as u64;
        stats.capture_interval_p95_us = stats.capture_interval_p95_us.max(interval);
    }
    if stats.first_encode_ms == 0 {
        stats.first_encode_ms = started.elapsed().as_millis() as u64;
    }
    if stats.first_send_ms == 0 {
        stats.first_send_ms = started.elapsed().as_millis() as u64;
    }
}

struct WinRtGuard;

impl WinRtGuard {
    fn new() -> Result<Self, String> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
            .map_err(win("initialize Windows Runtime"))?;
        Ok(Self)
    }
}

impl Drop for WinRtGuard {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

struct MfGuard;

impl MfGuard {
    fn new() -> Result<Self, String> {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.map_err(win("start Media Foundation"))?;
        Ok(Self)
    }
}

impl Drop for MfGuard {
    fn drop(&mut self) {
        unsafe { MFShutdown() }.ok();
    }
}

fn win(context: &'static str) -> impl FnOnce(windows::core::Error) -> String {
    move |error| format!("{context}: {error}")
}
