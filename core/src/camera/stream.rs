use livekit::webrtc::{
    video_frame::{I420Buffer, VideoFrame, VideoRotation},
    video_source::native::NativeVideoSource,
};
use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{
    ApiBackend, CameraFormat, CameraIndex, FrameFormat, RequestedFormat, RequestedFormatType,
    Resolution,
};
use nokhwa::Camera;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::livekit::video::VideoBufferManager;
use crate::utils::geometry::aspect_fit;

const CAMERA_CAPTURE_FPS: u32 = 30;
const CAMERA_CAPTURE_WIDTH: u32 = 1920;
const CAMERA_CAPTURE_HEIGHT: u32 = 1080;

const CAMERA_STREAM_WIDTH_HIGH: u32 = 1920;
const CAMERA_STREAM_HEIGHT_HIGH: u32 = 1080;
const CAMERA_STREAM_FPS_HIGH: u32 = 30;

const CAMERA_STREAM_WIDTH_LOW: u32 = 640;
const CAMERA_STREAM_HEIGHT_LOW: u32 = 360;
const CAMERA_STREAM_FPS_LOW: u32 = 15;

pub struct CameraStreamConfig {
    target_width: AtomicU32,
    target_height: AtomicU32,
    target_fps: AtomicU32,
}

impl CameraStreamConfig {
    pub fn new_high_quality() -> Self {
        Self {
            target_width: AtomicU32::new(CAMERA_STREAM_WIDTH_HIGH),
            target_height: AtomicU32::new(CAMERA_STREAM_HEIGHT_HIGH),
            target_fps: AtomicU32::new(CAMERA_STREAM_FPS_HIGH),
        }
    }

    pub fn set_high_quality(&self) {
        self.target_width
            .store(CAMERA_STREAM_WIDTH_HIGH, Ordering::Relaxed);
        self.target_height
            .store(CAMERA_STREAM_HEIGHT_HIGH, Ordering::Relaxed);
        self.target_fps
            .store(CAMERA_STREAM_FPS_HIGH, Ordering::Relaxed);
    }

    pub fn set_low_quality(&self) {
        self.target_width
            .store(CAMERA_STREAM_WIDTH_LOW, Ordering::Relaxed);
        self.target_height
            .store(CAMERA_STREAM_HEIGHT_LOW, Ordering::Relaxed);
        self.target_fps
            .store(CAMERA_STREAM_FPS_LOW, Ordering::Relaxed);
    }

    pub fn target_width(&self) -> u32 {
        self.target_width.load(Ordering::Relaxed)
    }
    pub fn target_height(&self) -> u32 {
        self.target_height.load(Ordering::Relaxed)
    }
    pub fn target_fps(&self) -> u32 {
        self.target_fps.load(Ordering::Relaxed)
    }
}

pub enum CameraStreamMessage {
    Failed(String),
    Stop,
    StopCapture,
}

pub struct CameraStream {
    capture_thread: Option<JoinHandle<()>>,
    tx: Option<mpsc::Sender<CameraStreamMessage>>,
    error_tx: mpsc::Sender<CameraStreamMessage>,
    buffer_source: NativeVideoSource,
    video_buffer_manager: Arc<VideoBufferManager>,
    width: u32,
    height: u32,
    device_name: String,
    failures_count: Arc<Mutex<u32>>,
    config: Arc<CameraStreamConfig>,
}

impl CameraStream {
    pub fn new(
        device_name: &str,
        error_tx: mpsc::Sender<CameraStreamMessage>,
        video_buffer_manager: Arc<VideoBufferManager>,
        buffer_source: NativeVideoSource,
        config: Arc<CameraStreamConfig>,
    ) -> Result<Self, String> {
        let mut cameras =
            nokhwa::query(ApiBackend::Auto).map_err(|e| format!("Failed to query cameras: {e}"))?;
        // Sort cameras like list_devices
        cameras.sort_by_key(|c| c.human_name());

        let camera_info = if device_name.is_empty() {
            cameras
                .first()
                .ok_or_else(|| "No cameras available".to_string())?
        } else {
            cameras
                .iter()
                .find(|c| c.human_name() == device_name)
                .or_else(|| {
                    log::warn!("Camera '{device_name}' not found, falling back to default");
                    cameras.first()
                })
                .ok_or_else(|| "No cameras available".to_string())?
        };

        let device_name = camera_info.human_name();
        let index = camera_info.index().clone();

        // Try YUYV first, then MJPEG, then accept any
        let camera = Self::try_open_camera(&index, FrameFormat::YUYV)
            .or_else(|_| Self::try_open_camera(&index, FrameFormat::MJPEG))
            .or_else(|_| Self::try_open_camera(&index, FrameFormat::NV12))
            .or_else(|_| {
                let format = RequestedFormat::new::<RgbFormat>(
                    RequestedFormatType::AbsoluteHighestFrameRate,
                );
                Camera::new(index, format)
                    .map_err(|e| format!("Failed to open camera with any format: {e}"))
            })?;

        let resolution = camera.resolution();
        let width = resolution.width_x;
        let height = resolution.height_y;
        log::info!(
            "CameraStream::new: opened camera '{}' at {}x{}",
            device_name,
            width,
            height
        );

        let mut stream = Self {
            capture_thread: None,
            tx: None,
            error_tx,
            buffer_source: buffer_source.clone(),
            video_buffer_manager,
            width,
            height,
            device_name: device_name.to_string(),
            failures_count: Arc::new(Mutex::new(0)),
            config: config.clone(),
        };

        stream.start_capture_with_camera(camera, config.clone())?;
        Ok(stream)
    }

    fn try_open_camera(index: &CameraIndex, frame_format: FrameFormat) -> Result<Camera, String> {
        let format =
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(CameraFormat::new(
                Resolution::new(CAMERA_CAPTURE_WIDTH, CAMERA_CAPTURE_HEIGHT),
                frame_format,
                CAMERA_CAPTURE_FPS,
            )));
        Camera::new(index.clone(), format)
            .map_err(|e| format!("Failed to open camera with {frame_format:?}: {e}"))
    }

    fn start_capture_with_camera(
        &mut self,
        mut camera: Camera,
        config: Arc<CameraStreamConfig>,
    ) -> Result<(), String> {
        camera
            .open_stream()
            .map_err(|e| format!("Failed to open camera stream: {e}"))?;

        let frame_format = camera.frame_format();
        log::info!("CameraStream: camera frame format: {frame_format:?}");

        let (tx, rx) = mpsc::channel();
        self.tx = Some(tx);

        let video_buffer_manager = self.video_buffer_manager.clone();
        let error_tx = self.error_tx.clone();
        let failures_count = self.failures_count.clone();
        let width = self.width;
        let height = self.height;
        let buffer_source = self.buffer_source.clone();

        let handle = std::thread::spawn(move || {
            let mut prev_stream_w: u32 = 0;
            let mut prev_stream_h: u32 = 0;
            let mut stream_frame = VideoFrame {
                rotation: VideoRotation::VideoRotation0,
                buffer: I420Buffer::new(1, 1),
                timestamp_us: 0,
            };
            let mut needs_scaling = false;

            let mut rgb_buf =
                if frame_format != FrameFormat::YUYV && frame_format != FrameFormat::NV12 {
                    log::info!("CameraStream: allocating RGB buffer at {width}x{height}");
                    vec![0u8; (width * height * 3) as usize]
                } else {
                    vec![]
                };
            log::info!("CameraStream: allocating I420 buffer at {width}x{height}");
            let mut i420 = I420Buffer::new(width, height);

            let capture_start = Instant::now();

            loop {
                let frame_start = Instant::now();

                if let Ok(CameraStreamMessage::StopCapture) = rx.try_recv() {
                    log::info!("CameraStream: StopCapture received, stopping");
                    video_buffer_manager.set_inactive(true);
                    break;
                }

                let cur_target_w = config.target_width();
                let cur_target_h = config.target_height();
                let cur_fps = config.target_fps();
                let frame_duration = Duration::from_micros(1_000_000 / cur_fps as u64);

                let (cur_stream_w, cur_stream_h) =
                    aspect_fit(width, height, cur_target_w, cur_target_h);
                if cur_stream_w != prev_stream_w || cur_stream_h != prev_stream_h {
                    stream_frame = VideoFrame {
                        rotation: VideoRotation::VideoRotation0,
                        buffer: I420Buffer::new(cur_stream_w, cur_stream_h),
                        timestamp_us: 0,
                    };
                    needs_scaling = cur_stream_w != width || cur_stream_h != height;
                    prev_stream_w = cur_stream_w;
                    prev_stream_h = cur_stream_h;
                    log::info!("CameraStream: target changed to {cur_stream_w}x{cur_stream_h} @ {cur_fps}fps");
                }

                match camera.frame() {
                    Ok(buf) => {
                        {
                            let mut fc = failures_count.lock().unwrap();
                            *fc = 0;
                        }

                        {
                            let converted = match frame_format {
                                FrameFormat::YUYV => {
                                    yuyv_write_i420(buf.buffer(), width, height, &mut i420);
                                    true
                                }
                                FrameFormat::NV12 => {
                                    nv12_write_i420(buf.buffer(), width, height, &mut i420);
                                    true
                                }
                                _ => match buf.decode_image_to_buffer::<RgbFormat>(&mut rgb_buf) {
                                    Ok(()) => {
                                        rgb_write_i420(&rgb_buf, width, height, &mut i420);
                                        true
                                    }
                                    Err(e) => {
                                        log::warn!("CameraStream: failed to decode frame: {e}");
                                        false
                                    }
                                },
                            };

                            if converted {
                                let write_frame = |buffer: &I420Buffer| {
                                    let mut write_buf =
                                        video_buffer_manager.write_buffer().lock().unwrap();
                                    write_buf.copy_from_i420(buffer, cur_stream_w, cur_stream_h);
                                    drop(write_buf);
                                    video_buffer_manager.advance_write();
                                };
                                if needs_scaling {
                                    let mut scaled =
                                        i420.scale(cur_stream_w as i32, cur_stream_h as i32);
                                    let (source_y, source_u, source_v) = scaled.data_mut();
                                    // TODO: check if this copy is needed
                                    let (data_y, data_u, data_v) = stream_frame.buffer.data_mut();
                                    data_y.copy_from_slice(source_y);
                                    data_u.copy_from_slice(source_u);
                                    data_v.copy_from_slice(source_v);
                                    stream_frame.timestamp_us =
                                        capture_start.elapsed().as_micros() as i64;
                                    write_frame(&stream_frame.buffer);
                                    buffer_source.capture_frame(&stream_frame);
                                } else {
                                    let frame = VideoFrame {
                                        rotation: VideoRotation::VideoRotation0,
                                        buffer: i420,
                                        timestamp_us: capture_start.elapsed().as_micros() as i64,
                                    };
                                    buffer_source.capture_frame(&frame);
                                    write_frame(&frame.buffer);
                                    i420 = frame.buffer; // recover for reuse
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let mut fc = failures_count.lock().unwrap();
                        *fc += 1;
                        log::error!("CameraStream: capture error: {e}");
                        let _ = error_tx.send(CameraStreamMessage::Failed(e.to_string()));
                        break;
                    }
                }

                let elapsed = frame_start.elapsed();
                if elapsed < frame_duration {
                    std::thread::sleep(frame_duration - elapsed);
                }
            }

            let _ = camera.stop_stream();
            log::info!("CameraStream: capture thread exiting");
        });

        self.capture_thread = Some(handle);
        Ok(())
    }

    pub fn stop_capture(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(CameraStreamMessage::StopCapture);
        }
        if let Some(handle) = self.capture_thread.take() {
            // Work around to camera.frame() blocking indefinitely.
            // we should fix it in nokhwa instead.
            for _ in 0..200 {
                if handle.is_finished() {
                    let _ = handle.join();
                    log::info!("CameraStream::stop_capture: thread joined");
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            log::warn!("CameraStream::stop_capture: thread did not finish in 2000ms, orphaning it");
            sentry_utils::upload_logs_event(
                "CameraStream::stop_capture: thread did not finish in 2000ms, orphaning it"
                    .to_string(),
            );
        }
    }

    pub fn get_failures_count(&self) -> u32 {
        *self.failures_count.lock().unwrap()
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn copy(&self) -> Result<Self, String> {
        let cameras =
            nokhwa::query(ApiBackend::Auto).map_err(|e| format!("Failed to query cameras: {e}"))?;

        let camera_info = cameras
            .iter()
            .find(|c| c.human_name() == self.device_name)
            .ok_or_else(|| format!("Camera '{}' not found", self.device_name))?;

        let index = camera_info.index().clone();

        // Try YUYV first, then MJPEG, then NV12, then accept any
        let camera = Self::try_open_camera(&index, FrameFormat::YUYV)
            .or_else(|_| Self::try_open_camera(&index, FrameFormat::MJPEG))
            .or_else(|_| Self::try_open_camera(&index, FrameFormat::NV12))
            .or_else(|_| {
                let format = RequestedFormat::new::<RgbFormat>(
                    RequestedFormatType::AbsoluteHighestFrameRate,
                );
                Camera::new(index, format)
                    .map_err(|e| format!("Failed to open camera with any format: {e}"))
            })?;

        let mut new_stream = Self {
            capture_thread: None,
            tx: None,
            error_tx: self.error_tx.clone(),
            buffer_source: self.buffer_source.clone(),
            video_buffer_manager: self.video_buffer_manager.clone(),
            width: self.width,
            height: self.height,
            device_name: self.device_name.clone(),
            failures_count: self.failures_count.clone(),
            config: self.config.clone(),
        };

        new_stream.start_capture_with_camera(camera, self.config.clone())?;
        Ok(new_stream)
    }
}

fn nv12_write_i420(nv12: &[u8], width: u32, height: u32, i420: &mut I420Buffer) {
    let (stride_y, stride_u, stride_v) = i420.strides();
    let (data_y, data_u, data_v) = i420.data_mut();
    let src_stride_y = width as i32;
    let src_stride_uv = width as i32;
    let uv_offset = (width * height) as usize;

    unsafe {
        yuv_sys::rs_NV12ToI420(
            nv12.as_ptr(),
            src_stride_y,
            nv12[uv_offset..].as_ptr(),
            src_stride_uv,
            data_y.as_mut_ptr(),
            stride_y as i32,
            data_u.as_mut_ptr(),
            stride_u as i32,
            data_v.as_mut_ptr(),
            stride_v as i32,
            width as i32,
            height as i32,
        );
    }
}

fn yuyv_write_i420(yuyv: &[u8], width: u32, height: u32, i420: &mut I420Buffer) {
    let (stride_y, stride_u, stride_v) = i420.strides();
    let (data_y, data_u, data_v) = i420.data_mut();
    let src_stride = width as i32 * 2; // YUYV is 2 bytes per pixel

    unsafe {
        yuv_sys::rs_YUY2ToI420(
            yuyv.as_ptr(),
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
    }
}

fn rgb_write_i420(rgb: &[u8], width: u32, height: u32, i420: &mut I420Buffer) {
    let (stride_y, stride_u, stride_v) = i420.strides();
    let (data_y, data_u, data_v) = i420.data_mut();
    let src_stride = width as i32 * 3;

    unsafe {
        yuv_sys::rs_RAWToI420(
            rgb.as_ptr(),
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
    }
}

/// List available cameras. We also sort them by name
/// so we can avoid listing them on consumers like dropdowns
/// and ensure consistent ordering between runs.
pub fn list_devices() -> Vec<socket_lib::CameraDevice> {
    match nokhwa::query(ApiBackend::Auto) {
        Ok(cameras) => {
            let mut devices: Vec<_> = cameras
                .iter()
                .map(|c| socket_lib::CameraDevice {
                    name: c.human_name(),
                    id: c.index().to_string(),
                    default: false,
                })
                .collect();
            devices.sort_by(|a, b| a.name.cmp(&b.name));
            if let Some(first) = devices.first_mut() {
                first.default = true;
            }
            devices
        }
        Err(e) => {
            log::error!("Failed to query cameras: {e}");
            vec![]
        }
    }
}
