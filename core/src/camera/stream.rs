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

const CAMERA_FPS: u32 = 30;
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
    buffer_source: Arc<Mutex<Option<NativeVideoSource>>>,
    width: u32,
    height: u32,
    device_name: String,
    failures_count: Arc<Mutex<u32>>,
}

impl CameraStream {
    pub fn new(
        device_name: &str,
        error_tx: mpsc::Sender<CameraStreamMessage>,
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
            buffer_source: Arc::new(Mutex::new(None)),
            width,
            height,
            device_name: device_name.to_string(),
            failures_count: Arc::new(Mutex::new(0)),
        };

        stream.start_capture_with_camera(camera)?;
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

    fn start_capture_with_camera(&mut self, mut camera: Camera) -> Result<(), String> {
        camera
            .open_stream()
            .map_err(|e| format!("Failed to open camera stream: {e}"))?;

        let frame_format = camera.frame_format();
        log::info!("CameraStream: camera frame format: {frame_format:?}");

        let (tx, rx) = mpsc::channel();
        self.tx = Some(tx);

        let buffer_source = self.buffer_source.clone();
        let error_tx = self.error_tx.clone();
        let failures_count = self.failures_count.clone();
        let width = self.width;
        let height = self.height;

        let handle = std::thread::spawn(move || {
            let frame_duration = Duration::from_micros(1_000_000 / CAMERA_FPS as u64);

            // Pre-allocate RGB decode buffer once (only used for non-YUYV formats)
            let mut rgb_buf = if frame_format != FrameFormat::YUYV {
                vec![0u8; (width * height * 3) as usize]
            } else {
                vec![]
            };

            loop {
                let frame_start = Instant::now();

                if let Ok(CameraStreamMessage::StopCapture) = rx.try_recv() {
                    log::info!("CameraStream: StopCapture received, stopping");
                    break;
                }

                match camera.frame() {
                    Ok(buf) => {
                        {
                            let mut fc = failures_count.lock().unwrap();
                            *fc = 0;
                        }

                        let source = {
                            let lock = buffer_source.lock().unwrap();
                            lock.clone()
                        };

                        if let Some(source) = source {
                            let frame = match frame_format {
                                FrameFormat::YUYV => yuyv_to_i420(buf.buffer(), width, height),
                                _ => match buf.decode_image_to_buffer::<RgbFormat>(&mut rgb_buf) {
                                    Ok(()) => rgb_to_i420(&rgb_buf, width, height),
                                    Err(e) => {
                                        log::warn!("CameraStream: failed to decode frame: {e}");
                                        continue;
                                    }
                                },
                            };
                            source.capture_frame(&frame);
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

    pub fn set_buffer_source(&self, source: NativeVideoSource) {
        let mut bs = self.buffer_source.lock().unwrap();
        *bs = Some(source);
    }

    pub fn extent(&self) -> (u32, u32) {
        (self.width, self.height)
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

        // Try YUYV first, then MJPEG, then accept any
        let camera = Self::try_open_camera(&index, FrameFormat::YUYV)
            .or_else(|_| Self::try_open_camera(&index, FrameFormat::MJPEG))
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
            width: self.width,
            height: self.height,
            device_name: self.device_name.clone(),
            failures_count: self.failures_count.clone(),
        };

        new_stream.start_capture_with_camera(camera)?;
        Ok(new_stream)
    }
}

fn yuyv_to_i420(yuyv: &[u8], width: u32, height: u32) -> VideoFrame<I420Buffer> {
    let mut i420 = I420Buffer::new(width, height);
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

    VideoFrame {
        rotation: VideoRotation::VideoRotation0,
        buffer: i420,
        timestamp_us: 0,
    }
}

fn rgb_to_i420(rgb: &[u8], width: u32, height: u32) -> VideoFrame<I420Buffer> {
    let mut i420 = I420Buffer::new(width, height);
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

    VideoFrame {
        rotation: VideoRotation::VideoRotation0,
        buffer: i420,
        timestamp_us: 0,
    }
}

pub fn list_devices() -> Vec<socket_lib::CameraDevice> {
    match nokhwa::query(ApiBackend::Auto) {
        Ok(cameras) => cameras
            .iter()
            .map(|c| socket_lib::CameraDevice {
                name: c.human_name(),
            })
            .collect(),
        Err(e) => {
            log::error!("Failed to query cameras: {e}");
            vec![]
        }
    }
}
