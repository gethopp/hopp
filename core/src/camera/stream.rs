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
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::livekit::video::VideoBufferManager;
use crate::utils::geometry::aspect_fit;

const CAMERA_FPS: u32 = 20;
const CAMERA_WIDTH: u32 = 1280;
const CAMERA_HEIGHT: u32 = 720;

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
    stream_width: u32,
    stream_height: u32,
    device_name: String,
    failures_count: Arc<Mutex<u32>>,
}

impl CameraStream {
    pub fn new(
        device_name: &str,
        error_tx: mpsc::Sender<CameraStreamMessage>,
        video_buffer_manager: Arc<VideoBufferManager>,
        buffer_source: NativeVideoSource,
    ) -> Result<Self, String> {
        let cameras =
            nokhwa::query(ApiBackend::Auto).map_err(|e| format!("Failed to query cameras: {e}"))?;

        let camera_info = if device_name.is_empty() {
            cameras
                .first()
                .ok_or_else(|| "No cameras available".to_string())?
        } else {
            cameras
                .iter()
                .find(|c| c.human_name() == device_name)
                .ok_or_else(|| format!("Camera '{}' not found", device_name))?
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
        let (stream_width, stream_height) = aspect_fit(width, height, CAMERA_WIDTH, CAMERA_HEIGHT);
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
            stream_width,
            stream_height,
            device_name: device_name.to_string(),
            failures_count: Arc::new(Mutex::new(0)),
        };

        stream.start_capture_with_camera(camera, buffer_source)?;
        Ok(stream)
    }

    fn try_open_camera(index: &CameraIndex, frame_format: FrameFormat) -> Result<Camera, String> {
        let format =
            RequestedFormat::new::<RgbFormat>(RequestedFormatType::Closest(CameraFormat::new(
                Resolution::new(CAMERA_WIDTH, CAMERA_HEIGHT),
                frame_format,
                CAMERA_FPS,
            )));
        Camera::new(index.clone(), format)
            .map_err(|e| format!("Failed to open camera with {frame_format:?}: {e}"))
    }

    fn start_capture_with_camera(
        &mut self,
        mut camera: Camera,
        buffer_source: NativeVideoSource,
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
        let (stream_width, stream_height) = aspect_fit(width, height, CAMERA_WIDTH, CAMERA_HEIGHT);
        let needs_scaling = stream_width != width || stream_height != height;
        log::info!("CameraStream: native {width}x{height}, stream {stream_width}x{stream_height}, scaling: {needs_scaling}");
        let buffer_source = self.buffer_source.clone();

        let handle = std::thread::spawn(move || {
            let frame_duration = Duration::from_micros(1_000_000 / CAMERA_FPS as u64);

            // Pre-allocate buffers once and reuse every frame
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
                                let write_frame = |buffer: &I420Buffer, fw: u32, fh: u32| {
                                    let mut write_buf =
                                        video_buffer_manager.write_buffer().lock().unwrap();
                                    write_buf.copy_from_i420(buffer, fw, fh);
                                    drop(write_buf);
                                    video_buffer_manager.advance_write();
                                };

                                if needs_scaling {
                                    let scaled =
                                        i420.scale(stream_width as i32, stream_height as i32);
                                    let frame = VideoFrame {
                                        rotation: VideoRotation::VideoRotation0,
                                        buffer: scaled,
                                        timestamp_us: capture_start.elapsed().as_micros() as i64,
                                    };
                                    buffer_source.capture_frame(&frame);
                                    write_frame(&frame.buffer, stream_width, stream_height);
                                } else {
                                    let frame = VideoFrame {
                                        rotation: VideoRotation::VideoRotation0,
                                        buffer: i420,
                                        timestamp_us: capture_start.elapsed().as_micros() as i64,
                                    };
                                    buffer_source.capture_frame(&frame);
                                    write_frame(&frame.buffer, width, height);
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
            let _ = handle.join();
        }
    }

    pub fn extent(&self) -> (u32, u32) {
        (self.stream_width, self.stream_height)
    }

    pub fn get_failures_count(&self) -> u32 {
        *self.failures_count.lock().unwrap()
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

        let (stream_width, stream_height) =
            aspect_fit(self.width, self.height, CAMERA_WIDTH, CAMERA_HEIGHT);
        let mut new_stream = Self {
            capture_thread: None,
            tx: None,
            error_tx: self.error_tx.clone(),
            buffer_source: self.buffer_source.clone(),
            video_buffer_manager: self.video_buffer_manager.clone(),
            width: self.width,
            height: self.height,
            stream_width,
            stream_height,
            device_name: self.device_name.clone(),
            failures_count: self.failures_count.clone(),
        };

        new_stream.start_capture_with_camera(camera, self.buffer_source.clone())?;
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

pub fn list_devices() -> Vec<socket_lib::CameraDevice> {
    match nokhwa::query(ApiBackend::Auto) {
        Ok(cameras) => cameras
            .iter()
            .enumerate()
            .map(|(i, c)| socket_lib::CameraDevice {
                name: c.human_name(),
                id: c.index().to_string(),
                default: i == 0,
            })
            .collect(),
        Err(e) => {
            log::error!("Failed to query cameras: {e}");
            vec![]
        }
    }
}
