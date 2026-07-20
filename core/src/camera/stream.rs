use std::sync::atomic::{AtomicU32, Ordering};

pub const CAMERA_STREAM_WIDTH_HIGH: u32 = 1280;
pub const CAMERA_STREAM_HEIGHT_HIGH: u32 = 720;
pub const CAMERA_STREAM_FPS_HIGH: u32 = 30;

pub const CAMERA_STREAM_WIDTH_LOW: u32 = 640;
pub const CAMERA_STREAM_HEIGHT_LOW: u32 = 360;
pub const CAMERA_STREAM_FPS_LOW: u32 = 15;

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

#[cfg_attr(
    any(target_os = "windows", target_os = "linux"),
    path = "stream_nokhwa.rs"
)]
#[cfg_attr(target_os = "macos", path = "stream_macos.rs")]
mod platform;

pub use platform::{list_devices, CameraStream};
