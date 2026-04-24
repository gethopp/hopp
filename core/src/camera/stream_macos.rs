use super::{CameraStreamConfig, CameraStreamMessage};
use crate::livekit::video::VideoBufferManager;
use crate::utils::geometry::aspect_fit;
use livekit::webrtc::{
    prelude::VideoBuffer,
    video_frame::{I420Buffer, VideoFrame, VideoRotation},
    video_source::native::NativeVideoSource,
};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

use dispatch2::DispatchQueue;
use objc2::{
    define_class, msg_send,
    rc::Retained,
    runtime::{NSObjectProtocol, ProtocolObject},
    AnyThread, DefinedClass,
};
use objc2_av_foundation::{
    AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceDiscoverySession, AVCaptureDeviceInput,
    AVCaptureDevicePosition, AVCaptureDeviceTypeBuiltInWideAngleCamera,
    AVCaptureDeviceTypeExternal, AVCaptureOutput, AVCaptureSession, AVCaptureVideoDataOutput,
    AVCaptureVideoDataOutputSampleBufferDelegate, AVMediaTypeVideo,
};
use objc2_core_media::{CMSampleBuffer, CMTime};
use objc2_core_video::{
    kCVPixelFormatType_32BGRA, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange, kCVPixelFormatType_422YpCbCr8,
    kCVPixelFormatType_422YpCbCr8_yuvs, kCVReturnSuccess, CVPixelBufferGetBaseAddress,
    CVPixelBufferGetBaseAddressOfPlane, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetBytesPerRowOfPlane, CVPixelBufferGetHeight, CVPixelBufferGetPixelFormatType,
    CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
    CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSArray, NSObject, NSString};

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

/// Set FPS on device hardware. If target fps is outside supported ranges, uses the lowest supported fps.
unsafe fn set_device_fps(device: &AVCaptureDevice, fps: u32) {
    let fps_f64 = fps as f64;
    let format = device.activeFormat();
    let ranges = format.videoSupportedFrameRateRanges();

    if ranges.count() == 0 {
        log::warn!("Camera has no supported frame rate ranges, skipping FPS configuration");
        return;
    }

    let supported = (0..ranges.count()).any(|i| {
        let range = ranges.objectAtIndex(i);
        fps_f64 >= range.minFrameRate() && fps_f64 <= range.maxFrameRate()
    });
    if !supported {
        log::info!("Camera does not support {fps}fps, using device default");
        return;
    }

    let duration = CMTime::new(1, fps as i32);
    if device.lockForConfiguration().is_ok() {
        device.setActiveVideoMinFrameDuration(duration);
        device.setActiveVideoMaxFrameDuration(duration);
        device.unlockForConfiguration();
        log::info!("Camera FPS set to {fps}");
    } else {
        log::warn!("Failed to lock device for FPS configuration");
    }
}

struct CameraDelegateState {
    device: Retained<AVCaptureDevice>,
    buffer_source: NativeVideoSource,
    video_buffer_manager: Arc<VideoBufferManager>,
    config: Arc<CameraStreamConfig>,
    capture_start: Instant,
    current_fps: u32,
    i420: Option<I420Buffer>,
    stream_frame: Option<VideoFrame<I420Buffer>>,
    prev_stream_w: u32,
    prev_stream_h: u32,
    failures_count: Arc<Mutex<u32>>,
    error_tx: mpsc::Sender<CameraStreamMessage>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "HoppCameraDelegate"]
    #[ivars = std::sync::Mutex<Option<CameraDelegateState>>]
    struct CameraDelegate;

    unsafe impl AVCaptureVideoDataOutputSampleBufferDelegate for CameraDelegate {
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

            let cur_fps = state.config.target_fps();
            if cur_fps != state.current_fps {
                unsafe { set_device_fps(&state.device, cur_fps) };
                state.current_fps = cur_fps;
            }

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
                            let _ = state.error_tx.send(CameraStreamMessage::Failed(
                                "Format conversion failed".to_string(),
                            ));
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
                            state.i420 = Some(i420);
                        } else {
                            let frame = VideoFrame {
                                rotation: VideoRotation::VideoRotation0,
                                buffer: i420,
                                timestamp_us: state.capture_start.elapsed().as_micros() as i64,
                            };
                            state.buffer_source.capture_frame(&frame);
                            write_frame(&frame.buffer);
                            state.i420 = Some(frame.buffer);
                        }
                    }

                    CVPixelBufferUnlockBaseAddress(&pixel_buffer, lock_flags);
                }
            }
        }

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
    pub fn new(
        device_name: &str,
        error_tx: mpsc::Sender<CameraStreamMessage>,
        video_buffer_manager: Arc<VideoBufferManager>,
        buffer_source: NativeVideoSource,
        config: Arc<CameraStreamConfig>,
        failures_count: Arc<Mutex<u32>>,
    ) -> Result<Self, String> {
        unsafe {
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
            output.setVideoSettings(None);

            if !session.canAddOutput(&output) {
                return Err("Cannot add camera output to session".to_string());
            }
            session.addOutput(&output);

            let initial_fps = config.target_fps();
            set_device_fps(&device, initial_fps);

            let state = CameraDelegateState {
                device: device.clone(),
                buffer_source: buffer_source.clone(),
                video_buffer_manager: video_buffer_manager.clone(),
                config: config.clone(),
                capture_start: Instant::now(),
                // Use 0 as sentinel so first frame always re-applies FPS after session starts,
                // since set_device_fps above runs before startRunning() and may have no effect.
                current_fps: 0,
                i420: None,
                stream_frame: None,
                prev_stream_w: 0,
                prev_stream_h: 0,
                failures_count: failures_count.clone(),
                error_tx: error_tx.clone(),
            };

            let delegate = CameraDelegate::alloc().set_ivars(std::sync::Mutex::new(Some(state)));
            let delegate: Retained<CameraDelegate> = msg_send![super(delegate), init];

            let queue = DispatchQueue::new("HoppCameraQueue", None);
            let queue_retained: Retained<DispatchQueue> = queue.into();

            let protocol_obj: &ProtocolObject<dyn AVCaptureVideoDataOutputSampleBufferDelegate> =
                ProtocolObject::from_ref(&*delegate);
            output.setSampleBufferDelegate_queue(Some(protocol_obj), Some(&queue_retained));

            session.startRunning();

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
    }

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

    pub fn get_failures_count(&self) -> u32 {
        *self.failures_count.lock().unwrap()
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

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
