use super::{CameraStreamConfig, CameraStreamMessage};
use crate::utils::geometry::aspect_fit;
use crate::{camera::stream::CAMERA_STREAM_FPS_HIGH, livekit::video::VideoBufferManager};
use livekit::webrtc::{
    prelude::VideoBuffer,
    video_frame::{I420Buffer, VideoFrame, VideoRotation},
    video_source::native::NativeVideoSource,
};
use std::panic::AssertUnwindSafe;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

use dispatch2::DispatchQueue;
use objc2::{
    define_class, exception, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObjectProtocol, ProtocolObject},
    AnyThread, DefinedClass,
};
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceDiscoverySession, AVCaptureDeviceInput,
    AVCaptureDevicePosition, AVCaptureDeviceTypeBuiltInWideAngleCamera,
    AVCaptureDeviceTypeExternal, AVCaptureOutput, AVCaptureSession, AVCaptureVideoDataOutput,
    AVCaptureVideoDataOutputSampleBufferDelegate, AVFrameRateRange, AVMediaTypeVideo,
};
use objc2_core_media::{CMSampleBuffer, CMTime, CMVideoFormatDescriptionGetDimensions};
use objc2_core_video::{
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_32BGRA,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, kCVPixelFormatType_422YpCbCr8,
    kCVPixelFormatType_422YpCbCr8_yuvs, kCVReturnSuccess, CVPixelBufferGetBaseAddress,
    CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType,
    CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSArray, NSDictionary, NSNumber, NSObject, NSString};

const SUPPORTED_PIXEL_FORMATS: &[u32] = &[
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
    kCVPixelFormatType_32BGRA,
    kCVPixelFormatType_422YpCbCr8_yuvs,
    kCVPixelFormatType_422YpCbCr8,
];

/// Discovers built-in and external video devices using AVFoundation's device list.
fn discover_video_devices() -> Retained<NSArray<AVCaptureDevice>> {
    unsafe {
        let device_types = NSArray::<NSString>::from_slice(&[
            AVCaptureDeviceTypeBuiltInWideAngleCamera,
            AVCaptureDeviceTypeExternal,
        ]);
        let session =
            AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
                &device_types,
                AVMediaTypeVideo,
                AVCaptureDevicePosition::Unspecified,
            );
        session.devices()
    }
}

/// Returns named camera choices in stable order with the first as the default.
pub fn list_devices() -> Vec<socket_lib::CameraDevice> {
    unsafe {
        let devices = discover_video_devices();
        let mut result = Vec::new();
        for i in 0..devices.count() {
            let device = devices.objectAtIndex(i);
            let name = device.localizedName().to_string();
            result.push(socket_lib::CameraDevice {
                name: name.clone(),
                id: name,
                default: false,
            });
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        if let Some(first) = result.first_mut() {
            first.default = true;
        }
        result
    }
}

/// Formats pixel-format identifiers as readable FourCCs or hexadecimal values.
fn four_cc(pixel_type: u32) -> String {
    let bytes = pixel_type.to_be_bytes();
    if bytes.iter().all(|&b| (0x20..0x7f).contains(&b)) {
        format!("'{}'", std::str::from_utf8(&bytes).unwrap_or("????"))
    } else {
        format!("0x{pixel_type:08X}")
    }
}

/// Picks a frame duration supported by the active format's frame-rate ranges.
/// Uses `CMTime::with_seconds` then clamps to each range's `minFrameDuration` /
/// `maxFrameDuration` (device-provided), avoiding lossy `fps as i32` timescales
/// that can trigger `NSInvalidArgumentException` on external cameras.
fn frame_duration_for_target_fps(
    ranges: &NSArray<AVFrameRateRange>,
    target_fps: f64,
) -> Option<(CMTime, f64)> {
    let count = ranges.count();
    if count == 0 || !target_fps.is_finite() || target_fps <= 0.0 {
        return None;
    }

    unsafe {
        for i in 0..count {
            let range = ranges.objectAtIndex(i);
            let min_rate = range.minFrameRate();
            let max_rate = range.maxFrameRate();
            if target_fps >= min_rate && target_fps <= max_rate {
                let min_d = range.minFrameDuration();
                let max_d = range.maxFrameDuration();
                let preferred_timescale: i32 = 60_000;
                let mut desired = CMTime::with_seconds(1.0 / target_fps, preferred_timescale);
                if desired.compare(min_d) < 0 {
                    desired = min_d;
                } else if desired.compare(max_d) > 0 {
                    desired = max_d;
                }
                let actual_fps = 1.0 / desired.seconds();
                return Some((desired, actual_fps));
            }
        }

        let mut best_duration: Option<CMTime> = None;
        let mut best_distance = f64::MAX;

        for i in 0..count {
            let range = ranges.objectAtIndex(i);
            for (rate, duration) in [
                (range.minFrameRate(), range.maxFrameDuration()),
                (range.maxFrameRate(), range.minFrameDuration()),
            ] {
                let distance = (rate - target_fps).abs();
                if distance < best_distance {
                    best_distance = distance;
                    best_duration = Some(duration);
                }
            }
        }

        let duration = best_duration?;
        let actual_fps = 1.0 / duration.seconds();
        Some((duration, actual_fps))
    }
}

/// Keep the device's selected format in place until the session has started.
/// In particular, macOS can otherwise reconfigure it during `startRunning`.
struct DeviceConfigurationLock<'a>(&'a AVCaptureDevice);

impl<'a> DeviceConfigurationLock<'a> {
    /// Holds exclusive device configuration until startup has preserved its format.
    unsafe fn acquire(device: &'a AVCaptureDevice) -> Result<Self, String> {
        device
            .lockForConfiguration()
            .map_err(|error| format!("Cannot configure camera input: {error:?}"))?;
        Ok(Self(device))
    }
}

impl Drop for DeviceConfigurationLock<'_> {
    /// Unlocks the device without replacing an existing error with an exception.
    fn drop(&mut self) {
        // Do not let a cleanup exception replace a setup error or panic.
        if let Err(error) = exception::catch(AssertUnwindSafe(|| unsafe {
            self.0.unlockForConfiguration();
        })) {
            log::error!("Failed to unlock camera configuration: {error:?}");
        }
    }
}

struct SessionConfiguration<'a> {
    session: &'a AVCaptureSession,
    committed: bool,
}

impl<'a> SessionConfiguration<'a> {
    /// Begins a configuration transaction that must finish before session startup.
    unsafe fn begin(session: &'a AVCaptureSession) -> Self {
        session.beginConfiguration();
        Self {
            session,
            committed: false,
        }
    }

    /// Commits once and prevents a throwing commit from being retried by Drop.
    unsafe fn commit(mut self) {
        // A throwing commit must not be retried while unwinding.
        self.committed = true;
        self.session.commitConfiguration();
    }
}

impl Drop for SessionConfiguration<'_> {
    /// Finishes uncommitted configuration while preserving any original setup error.
    fn drop(&mut self) {
        if !self.committed {
            if let Err(error) = exception::catch(AssertUnwindSafe(|| unsafe {
                self.session.commitConfiguration();
            })) {
                log::error!("Failed to finish camera session configuration: {error:?}");
            }
        }
    }
}

struct CameraDelegateState {
    buffer_source: NativeVideoSource,
    video_buffer_manager: Arc<VideoBufferManager>,
    config: Arc<CameraStreamConfig>,
    generation: u64,
    capture_start: Instant,
    last_frame_time: Instant,
    i420: Option<I420Buffer>,
    stream_frame: Option<VideoFrame<I420Buffer>>,
    prev_stream_w: u32,
    prev_stream_h: u32,
    failures_count: Arc<Mutex<u32>>,
    error_tx: mpsc::Sender<CameraStreamMessage>,
    frame_counter: u64,
    requested_dimensions: (u32, u32),
    selected_dimensions: (u32, u32),
    delivered_dimensions: Option<(u32, u32)>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "HoppCameraDelegate"]
    #[ivars = std::sync::Mutex<Option<CameraDelegateState>>]
    struct CameraDelegate;

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CameraDelegate {
        /// Converts supported pixel buffers and publishes frames at the output cadence.
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn capture_output_did_output_sample_buffer(
            &self,
            _capture_output: &AVCaptureOutput,
            sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            let mut state_guard = self.ivars().lock().unwrap();
            let state = match state_guard.as_mut() {
                Some(s) => s,
                None => return,
            };

            let target_fps = state.config.target_fps();
            let min_interval = std::time::Duration::from_micros(1_000_000 / target_fps as u64);
            let now = Instant::now();
            if now.duration_since(state.last_frame_time) < min_interval {
                return;
            }
            state.last_frame_time = now;

            unsafe {
                let image_buffer = sample_buffer.image_buffer();
                if let Some(pixel_buffer) = image_buffer {
                    let lock_flags = CVPixelBufferLockFlags::ReadOnly;
                    let lock_result = CVPixelBufferLockBaseAddress(&pixel_buffer, lock_flags);
                    if lock_result != kCVReturnSuccess {
                        log::warn!("Failed to lock pixel buffer: {}", lock_result);
                        return;
                    }

                    let format = CVPixelBufferGetPixelFormatType(&pixel_buffer);
                    let width = CVPixelBufferGetWidth(&pixel_buffer) as u32;
                    let height = CVPixelBufferGetHeight(&pixel_buffer) as u32;

                    if state.delivered_dimensions != Some((width, height)) {
                        let (requested_width, requested_height) = state.requested_dimensions;
                        let (selected_width, selected_height) = state.selected_dimensions;
                        log::info!(
                            "Camera delivered {width}x{height} {} (requested {requested_width}x{requested_height}, selected {selected_width}x{selected_height})",
                            four_cc(format)
                        );
                        if (width, height) != state.selected_dimensions {
                            log::warn!(
                                "Delivered camera dimensions differ from the selected input format"
                            );
                        }
                        state.delivered_dimensions = Some((width, height));
                    }

                    let mut i420 = match state.i420.take() {
                        Some(buf) if buf.width() == width && buf.height() == height => buf,
                        _ => {
                            log::info!("Allocating I420 buffer at {width}x{height}");
                            I420Buffer::new(width, height)
                        }
                    };
                    let (stride_y, stride_u, stride_v) = i420.strides();
                    let (data_y, data_u, data_v) = i420.data_mut();

                    let converted = if format == kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange || format == kCVPixelFormatType_420YpCbCr8BiPlanarFullRange {
                        let y_ptr = CVPixelBufferGetBaseAddressOfPlane(&pixel_buffer, 0) as *const u8;
                        let uv_ptr = CVPixelBufferGetBaseAddressOfPlane(&pixel_buffer, 1) as *const u8;
                        let stride_y_src = CVPixelBufferGetBytesPerRowOfPlane(&pixel_buffer, 0) as i32;
                        let stride_uv_src = CVPixelBufferGetBytesPerRowOfPlane(&pixel_buffer, 1) as i32;
                        yuv_sys::rs_NV12ToI420(
                            y_ptr,
                            stride_y_src,
                            uv_ptr,
                            stride_uv_src,
                            data_y.as_mut_ptr(),
                            stride_y as i32,
                            data_u.as_mut_ptr(),
                            stride_u as i32,
                            data_v.as_mut_ptr(),
                            stride_v as i32,
                            width as i32,
                            height as i32,
                        );
                        true
                    } else if format == kCVPixelFormatType_32BGRA {
                        let src_ptr = CVPixelBufferGetBaseAddress(&pixel_buffer) as *const u8;
                        let src_stride = CVPixelBufferGetBytesPerRow(&pixel_buffer) as i32;
                        yuv_sys::rs_ARGBToI420(
                            src_ptr,
                            src_stride,
                            data_y.as_mut_ptr(),
                            stride_y as i32,
                            data_u.as_mut_ptr(),
                            stride_u as i32,
                            data_v.as_mut_ptr(),
                            stride_v as i32,
                            width as i32,
                            height as i32,
                        );
                        true
                    } else if format == kCVPixelFormatType_422YpCbCr8_yuvs {
                        let src_ptr = CVPixelBufferGetBaseAddress(&pixel_buffer) as *const u8;
                        let src_stride = CVPixelBufferGetBytesPerRow(&pixel_buffer) as i32;
                        yuv_sys::rs_YUY2ToI420(
                            src_ptr,
                            src_stride,
                            data_y.as_mut_ptr(),
                            stride_y as i32,
                            data_u.as_mut_ptr(),
                            stride_u as i32,
                            data_v.as_mut_ptr(),
                            stride_v as i32,
                            width as i32,
                            height as i32,
                        );
                        true
                    } else if format == kCVPixelFormatType_422YpCbCr8 {
                        let src_ptr = CVPixelBufferGetBaseAddress(&pixel_buffer) as *const u8;
                        let src_stride = CVPixelBufferGetBytesPerRow(&pixel_buffer) as i32;
                        yuv_sys::rs_UYVYToI420(
                            src_ptr,
                            src_stride,
                            data_y.as_mut_ptr(),
                            stride_y as i32,
                            data_u.as_mut_ptr(),
                            stride_u as i32,
                            data_v.as_mut_ptr(),
                            stride_v as i32,
                            width as i32,
                            height as i32,
                        );
                        true
                    } else {
                        log::warn!("Unsupported pixel format: {}", format);
                         *state.failures_count.lock().unwrap() += 1;
                            let _ = state.error_tx.send(CameraStreamMessage::Failed {
                                reason: "Format conversion failed".to_string(),
                                generation: state.generation,
                            });
                        false
                    };

                    if converted {
                        *state.failures_count.lock().unwrap() = 0;
                        let cur_target_w = state.config.target_width();
                        let cur_target_h = state.config.target_height();
                        let (cur_stream_w, cur_stream_h) = aspect_fit(width, height, cur_target_w, cur_target_h);

                        let needs_scaling = cur_stream_w != width || cur_stream_h != height;

                        let write_frame = |buffer: &I420Buffer| {
                            let mut write_buf = state.video_buffer_manager.write_buffer().lock().unwrap();
                            write_buf.copy_from_i420(buffer, cur_stream_w, cur_stream_h);
                            drop(write_buf);
                            state.video_buffer_manager.advance_write();
                        };

                        if needs_scaling {
                            if cur_stream_w != state.prev_stream_w || cur_stream_h != state.prev_stream_h {
                                state.stream_frame = Some(VideoFrame {
                                    rotation: VideoRotation::VideoRotation0,
                                    buffer: I420Buffer::new(cur_stream_w, cur_stream_h),
                                    timestamp_us: 0,
                                    frame_metadata: None,
                                });
                                state.prev_stream_w = cur_stream_w;
                                state.prev_stream_h = cur_stream_h;
                                log::info!("Target changed to {cur_stream_w}x{cur_stream_h}");
                            }
                            let mut scaled = i420.scale(cur_stream_w as i32, cur_stream_h as i32);
                            if let Some(ref mut stream_frame) = state.stream_frame {
                                let (src_y, src_u, src_v) = scaled.data_mut();
                                let (dst_y, dst_u, dst_v) = stream_frame.buffer.data_mut();
                                dst_y.copy_from_slice(src_y);
                                dst_u.copy_from_slice(src_u);
                                dst_v.copy_from_slice(src_v);
                                stream_frame.timestamp_us = state.capture_start.elapsed().as_micros() as i64;
                                write_frame(&stream_frame.buffer);
                                state.buffer_source.capture_frame(stream_frame);
                            }
                            state.video_buffer_manager.set_frame_id(state.frame_counter);
                            state.frame_counter += 1;
                            state.i420 = Some(i420);
                        } else {
                            let frame = VideoFrame {
                                rotation: VideoRotation::VideoRotation0,
                                buffer: i420,
                                timestamp_us: state.capture_start.elapsed().as_micros() as i64,
                                frame_metadata: None,
                            };
                            state.buffer_source.capture_frame(&frame);
                            write_frame(&frame.buffer);
                            state.video_buffer_manager.set_frame_id(state.frame_counter);
                            state.frame_counter += 1;
                            state.i420 = Some(frame.buffer);
                        }
                    }

                    CVPixelBufferUnlockBaseAddress(&pixel_buffer, lock_flags);
                }
            }
        }

        /// Accepts AVFoundation's dropped-frame notification without retaining data.
        #[unsafe(method(captureOutput:didDropSampleBuffer:fromConnection:))]
        fn capture_output_did_drop_sample_buffer(
            &self,
            _capture_output: &AVCaptureOutput,
            _sample_buffer: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
        }
    }
);

unsafe impl NSObjectProtocol for CameraDelegate {}

pub struct CameraStream {
    session: Retained<AVCaptureSession>,
    input: Retained<AVCaptureDeviceInput>,
    output: Retained<AVCaptureVideoDataOutput>,
    delegate: Retained<CameraDelegate>,
    #[allow(dead_code)] // Retained to keep dispatch queue alive for output delegate
    queue: Retained<DispatchQueue>,
    error_tx: mpsc::Sender<CameraStreamMessage>,
    buffer_source: NativeVideoSource,
    video_buffer_manager: Arc<VideoBufferManager>,
    device_name: String,
    failures_count: Arc<Mutex<u32>>,
    config: Arc<CameraStreamConfig>,
}

unsafe impl Send for CameraStream {}
unsafe impl Sync for CameraStream {}

impl CameraStream {
    /// Starts capture while converting Objective-C setup exceptions into Rust errors.
    pub fn new(
        device_name: &str,
        error_tx: mpsc::Sender<CameraStreamMessage>,
        video_buffer_manager: Arc<VideoBufferManager>,
        buffer_source: NativeVideoSource,
        config: Arc<CameraStreamConfig>,
        failures_count: Arc<Mutex<u32>>,
    ) -> Result<Self, String> {
        match exception::catch(AssertUnwindSafe(|| unsafe {
            Self::new_inner(
                device_name,
                error_tx,
                video_buffer_manager,
                buffer_source,
                config,
                failures_count,
            )
        })) {
            Ok(result) => result,
            Err(Some(exc)) => Err(format!("Objective-C exception during camera setup: {exc}")),
            Err(None) => {
                Err("Objective-C exception during camera setup (nil exception)".to_string())
            }
        }
    }

    /// Negotiates the input format and holds the device lock through session startup.
    /// Actual delivered dimensions are checked by the delegate on frame delivery.
    unsafe fn new_inner(
        device_name: &str,
        error_tx: mpsc::Sender<CameraStreamMessage>,
        video_buffer_manager: Arc<VideoBufferManager>,
        buffer_source: NativeVideoSource,
        config: Arc<CameraStreamConfig>,
        failures_count: Arc<Mutex<u32>>,
    ) -> Result<Self, String> {
        let devices = discover_video_devices();
        let mut sorted_devices: Vec<Retained<AVCaptureDevice>> = (0..devices.count())
            .map(|i| devices.objectAtIndex(i))
            .collect();
        sorted_devices.sort_by(|a, b| {
            a.localizedName()
                .to_string()
                .cmp(&b.localizedName().to_string())
        });

        let default_device = || {
            sorted_devices
                .first()
                .cloned()
                .ok_or("No cameras available".to_string())
        };

        let device = if device_name.is_empty() {
            default_device()?
        } else {
            match sorted_devices
                .iter()
                .find(|d| d.localizedName().to_string() == device_name)
            {
                Some(d) => d.clone(),
                None => {
                    log::warn!(
                        "Camera '{}' not found, falling back to default",
                        device_name
                    );
                    default_device()?
                }
            }
        };

        let actual_name = device.localizedName().to_string();

        let session = AVCaptureSession::new();

        let input_res = AVCaptureDeviceInput::deviceInputWithDevice_error(&device);
        let input = match input_res {
            Ok(i) => i,
            Err(e) => return Err(format!("Failed to create capture device input: {:?}", e)),
        };

        if !session.canAddInput(&input) {
            return Err("Cannot add camera input to session".to_string());
        }
        session.addInput(&input);

        let output = AVCaptureVideoDataOutput::new();
        output.setAlwaysDiscardsLateVideoFrames(true);

        if !session.canAddOutput(&output) {
            return Err("Cannot add camera output to session".to_string());
        }
        session.addOutput(&output);

        // Select from the device's supported formats rather than assuming the
        // requested input size or frame rate is available.
        let target_fps = CAMERA_STREAM_FPS_HIGH as f64;
        let requested = config.input_resolution().extent();
        let formats = device.formats();
        let mut selected = None;
        let mut best_score = (u64::MAX, u64::MAX);
        for index in 0..formats.count() {
            let format = formats.objectAtIndex(index);
            let dimensions = CMVideoFormatDescriptionGetDimensions(&format.formatDescription());
            if dimensions.width <= 0 || dimensions.height <= 0 {
                continue;
            }
            let Some((duration, actual_fps)) =
                frame_duration_for_target_fps(&format.videoSupportedFrameRateRanges(), target_fps)
            else {
                continue;
            };
            let score = (
                (dimensions.width as u64).abs_diff(requested.width as u64)
                    + (dimensions.height as u64).abs_diff(requested.height as u64),
                ((actual_fps - target_fps).abs() * 1000.0) as u64,
            );
            if score < best_score {
                best_score = score;
                selected = Some((format, dimensions, duration, actual_fps));
            }
        }
        let Some((format, dimensions, frame_duration, actual_fps)) = selected else {
            return Err("Camera has no supported input format".to_string());
        };
        let device_lock = DeviceConfigurationLock::acquire(&device)?;
        // Apply format and frame durations atomically, as required by
        // AVCaptureDevice.activeFormat. Retain the device lock through startup
        // so macOS cannot replace the format when starting the session.
        let session_configuration = SessionConfiguration::begin(&session);
        let configuration = exception::catch(AssertUnwindSafe(|| {
            device.setActiveFormat(&format);
            device.setActiveVideoMinFrameDuration(frame_duration);
            device.setActiveVideoMaxFrameDuration(frame_duration);
        }));
        match configuration {
            Ok(()) => {}
            Err(Some(error)) => {
                return Err(format!(
                    "Objective-C exception configuring camera input: {error}"
                ));
            }
            Err(None) => {
                return Err(
                    "Objective-C exception configuring camera input (nil exception)".to_string(),
                );
            }
        }
        log::info!(
            "Camera input selected {}x{} at {actual_fps:.2}fps (requested {}x{})",
            dimensions.width,
            dimensions.height,
            requested.width,
            requested.height
        );

        // Request first supported pixel format from device's available list.
        let available_formats = output.availableVideoCVPixelFormatTypes();
        let chosen_format = SUPPORTED_PIXEL_FORMATS.iter().find(|&&fmt| {
            (0..available_formats.count())
                .any(|i| available_formats.objectAtIndex(i).as_u32() == fmt)
        });
        let Some(&format) = chosen_format else {
            return Err("Camera does not support any required pixel format".to_string());
        };
        log::info!("Camera requesting pixel format: {}", four_cc(format));
        let format_number = NSNumber::new_u32(format);
        let key: &NSString = &*(kCVPixelBufferPixelFormatTypeKey as *const _ as *const NSString);
        // `setVideoSettings` expects `NSDictionary<NSString, AnyObject>`; `NSNumber` is an
        // NSObject subclass, so re-borrow the same pointer as `AnyObject`.
        let value_any: &AnyObject = &*(&*format_number as *const NSNumber as *const AnyObject);
        let settings = NSDictionary::<NSString, AnyObject>::from_slices(&[key], &[value_any]);
        output.setVideoSettings(Some(&settings));

        let state = CameraDelegateState {
            buffer_source: buffer_source.clone(),
            video_buffer_manager: video_buffer_manager.clone(),
            config: config.clone(),
            generation: config.capture_generation(),
            capture_start: Instant::now(),
            last_frame_time: Instant::now(),
            i420: None,
            stream_frame: None,
            prev_stream_w: 0,
            prev_stream_h: 0,
            failures_count: failures_count.clone(),
            error_tx: error_tx.clone(),
            frame_counter: 0,
            requested_dimensions: (requested.width as u32, requested.height as u32),
            selected_dimensions: (dimensions.width as u32, dimensions.height as u32),
            delivered_dimensions: None,
        };

        let delegate = CameraDelegate::alloc().set_ivars(std::sync::Mutex::new(Some(state)));
        let delegate: Retained<CameraDelegate> = msg_send![super(delegate), init];

        let queue = DispatchQueue::new("HoppCameraQueue", None);
        let queue_retained: Retained<DispatchQueue> = queue.into();

        let protocol_obj: &ProtocolObject<dyn AVCaptureVideoDataOutputSampleBufferDelegate> =
            ProtocolObject::from_ref(&*delegate);
        output.setSampleBufferDelegate_queue(Some(protocol_obj), Some(&queue_retained));

        // Starting a session while its configuration transaction is open is
        // invalid; commit first, while the device remains locked.
        session_configuration.commit();
        let startup = exception::catch(AssertUnwindSafe(|| session.startRunning()));
        drop(device_lock);
        match startup {
            Ok(()) => {}
            Err(Some(error)) => {
                return Err(format!(
                    "Objective-C exception starting camera session: {error}"
                ));
            }
            Err(None) => {
                return Err(
                    "Objective-C exception starting camera session (nil exception)".to_string(),
                );
            }
        }

        Ok(Self {
            session,
            input,
            output,
            delegate,
            queue: queue_retained,
            error_tx,
            buffer_source,
            video_buffer_manager,
            device_name: actual_name,
            failures_count,
            config,
        })
    }

    /// Retires delegate state before detaching outputs and stopping the session.
    pub fn stop_capture(&mut self) {
        unsafe {
            if let Ok(mut state) = self.delegate.ivars().lock() {
                *state = None;
            }

            self.output.setSampleBufferDelegate_queue(None, None);
            self.session.stopRunning();
            self.session.removeInput(&self.input);
            self.session.removeOutput(&self.output);
        }
        self.video_buffer_manager.set_inactive(true);
        log::info!("CameraStream::stop_capture complete");
    }

    /// Returns the shared conversion-failure count used by capture retry policy.
    pub fn get_failures_count(&self) -> u32 {
        *self.failures_count.lock().unwrap()
    }

    /// Returns the actual selected device, including startup fallback choices.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Reopens the same device with the current input preset and shared output source.
    pub fn copy(&self) -> Result<Self, String> {
        Self::new(
            &self.device_name,
            self.error_tx.clone(),
            self.video_buffer_manager.clone(),
            self.buffer_source.clone(),
            self.config.clone(),
            self.failures_count.clone(),
        )
    }
}
