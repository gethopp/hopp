use socket_lib::CameraInputResolution;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub const CAMERA_STREAM_WIDTH_HIGH: u32 = 1280;
pub const CAMERA_STREAM_HEIGHT_HIGH: u32 = 720;
pub const CAMERA_STREAM_FPS_HIGH: u32 = 30;

pub const CAMERA_STREAM_WIDTH_LOW: u32 = 640;
pub const CAMERA_STREAM_HEIGHT_LOW: u32 = 360;
pub const CAMERA_STREAM_FPS_LOW: u32 = 15;

pub struct CameraStreamConfig {
    input_resolution: AtomicU32,
    generation: AtomicU64,
    target_width: AtomicU32,
    target_height: AtomicU32,
    target_fps: AtomicU32,
}

impl CameraStreamConfig {
    /// Starts with 1080p input requested and the normal 720p/30fps output policy.
    pub fn new_high_quality() -> Self {
        Self {
            input_resolution: AtomicU32::new(0),
            generation: AtomicU64::new(0),
            target_width: AtomicU32::new(CAMERA_STREAM_WIDTH_HIGH),
            target_height: AtomicU32::new(CAMERA_STREAM_HEIGHT_HIGH),
            target_fps: AtomicU32::new(CAMERA_STREAM_FPS_HIGH),
        }
    }

    /// Records the next capture size without changing outgoing video targets.
    pub fn set_input_resolution(&self, resolution: CameraInputResolution) {
        self.input_resolution.store(
            match resolution {
                CameraInputResolution::P1080 => 0,
                CameraInputResolution::P1440 => 1,
                CameraInputResolution::P4K => 2,
            },
            Ordering::Relaxed,
        );
    }

    /// Returns the requested device preset; unsupported values default to 1080p.
    pub fn input_resolution(&self) -> CameraInputResolution {
        match self.input_resolution.load(Ordering::Relaxed) {
            1 => CameraInputResolution::P1440,
            2 => CameraInputResolution::P4K,
            _ => CameraInputResolution::P1080,
        }
    }

    /// Identifies the capture session whose asynchronous events are still current.
    pub(crate) fn capture_generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Invalidates asynchronous errors belonging to earlier capture sessions.
    pub(crate) fn next_capture_generation(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Restores normal outgoing dimensions and cadence without reopening input.
    pub fn set_high_quality(&self) {
        self.target_width
            .store(CAMERA_STREAM_WIDTH_HIGH, Ordering::Relaxed);
        self.target_height
            .store(CAMERA_STREAM_HEIGHT_HIGH, Ordering::Relaxed);
        self.target_fps
            .store(CAMERA_STREAM_FPS_HIGH, Ordering::Relaxed);
    }

    /// Reduces outgoing camera cost during screen sharing, preserving input size.
    pub fn set_low_quality(&self) {
        self.target_width
            .store(CAMERA_STREAM_WIDTH_LOW, Ordering::Relaxed);
        self.target_height
            .store(CAMERA_STREAM_HEIGHT_LOW, Ordering::Relaxed);
        self.target_fps
            .store(CAMERA_STREAM_FPS_LOW, Ordering::Relaxed);
    }

    /// Returns the outgoing width bound used when scaling captured frames.
    pub fn target_width(&self) -> u32 {
        self.target_width.load(Ordering::Relaxed)
    }
    /// Returns the outgoing height bound used when scaling captured frames.
    pub fn target_height(&self) -> u32 {
        self.target_height.load(Ordering::Relaxed)
    }
    /// Returns the publication cadence, independent of the negotiated camera FPS.
    pub fn target_fps(&self) -> u32 {
        self.target_fps.load(Ordering::Relaxed)
    }
}

pub enum CameraStreamMessage {
    Failed { reason: String, generation: u64 },
    Stop,
    StopCapture,
}

#[cfg_attr(target_os = "windows", path = "stream_windows.rs")]
#[cfg_attr(target_os = "linux", path = "stream_nokhwa.rs")]
#[cfg_attr(target_os = "macos", path = "stream_macos.rs")]
mod platform;

pub use platform::{list_devices, CameraStream};

#[cfg(test)]
mod tests {
    use super::*;

    /// Input requests and the screen-sharing output policy must remain independent.
    #[test]
    fn camera_input_and_output_quality_are_independent() {
        let config = CameraStreamConfig::new_high_quality();
        assert_eq!(config.input_resolution(), CameraInputResolution::P1080);
        config.set_input_resolution(CameraInputResolution::P4K);
        config.set_low_quality();
        assert_eq!(config.input_resolution(), CameraInputResolution::P4K);
        assert_eq!(
            (
                config.target_width(),
                config.target_height(),
                config.target_fps()
            ),
            (640, 360, 15)
        );
        config.set_input_resolution(CameraInputResolution::P1440);
        assert_eq!(config.target_width(), 640);
        config.set_high_quality();
        assert_eq!(config.input_resolution(), CameraInputResolution::P1440);
        assert_eq!(
            (
                config.target_width(),
                config.target_height(),
                config.target_fps()
            ),
            (1280, 720, 30)
        );
    }

    /// Shared configuration exposes new presets and invalidates old session events.
    #[test]
    fn shared_camera_config_observes_input_changes_and_capture_generations() {
        let config = std::sync::Arc::new(CameraStreamConfig::new_high_quality());
        let observer = config.clone();
        config.set_input_resolution(CameraInputResolution::P1440);
        config.next_capture_generation();
        assert_eq!(observer.input_resolution(), CameraInputResolution::P1440);
        assert_eq!(observer.capture_generation(), 1);
    }
}
