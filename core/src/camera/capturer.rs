use livekit::webrtc::video_source::native::NativeVideoSource;
use socket_lib::{CameraDevice, CameraInputResolution, Message, SocketSender};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use crate::livekit::video::VideoBufferManager;

use super::stream::{CameraStream, CameraStreamConfig, CameraStreamMessage};

pub const MAX_CAMERA_FAILURES_BEFORE_STOP: u32 = 1;
const POLL_CAMERA_TIMEOUT_SECS: u64 = 100;

pub struct CameraCapturer {
    stream: Option<CameraStream>,
    rx: Arc<Mutex<mpsc::Receiver<CameraStreamMessage>>>,
    tx: mpsc::Sender<CameraStreamMessage>,
    socket: Option<SocketSender>,
    stream_config: Arc<CameraStreamConfig>,
}

impl Default for CameraCapturer {
    /// Creates an idle capturer with the standard input and output defaults.
    fn default() -> Self {
        Self::new()
    }
}

impl CameraCapturer {
    /// Creates the shared stream configuration and failure-monitor channel.
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            stream: None,
            rx: Arc::new(Mutex::new(rx)),
            tx,
            socket: None,
            stream_config: Arc::new(CameraStreamConfig::new_high_quality()),
        }
    }

    /// Enumerates selectable cameras through the current platform's backend.
    pub fn list_devices() -> Vec<CameraDevice> {
        super::stream::list_devices()
    }

    /// Returns the actual selected camera, including any missing-device fallback.
    pub fn active_device_name(&self) -> Option<&str> {
        self.stream.as_ref().map(|s| s.device_name())
    }

    /// Replaces existing capture and associates failures with a fresh generation.
    pub fn start_capture(
        &mut self,
        device_name: Option<&str>,
        socket: SocketSender,
        video_buffer_manager: Arc<VideoBufferManager>,
        buffer_source: NativeVideoSource,
    ) -> Result<(), String> {
        self.stop_capture();
        self.stream_config.next_capture_generation();
        self.socket = Some(socket);
        let name = device_name.unwrap_or("");
        log::info!("CameraCapturer::start_capture: device='{name}'");
        let stream = CameraStream::new(
            name,
            self.tx.clone(),
            video_buffer_manager.clone(),
            buffer_source,
            self.stream_config.clone(),
            #[cfg(target_os = "macos")]
            Arc::new(Mutex::new(0u32)),
        )?;
        self.stream = Some(stream);
        Ok(())
    }

    /// Invalidates old failure events before releasing the active capture stream.
    pub fn stop_capture(&mut self) {
        self.stream_config.next_capture_generation();
        if let Some(mut stream) = self.stream.take() {
            log::info!("CameraCapturer::stop_capture");
            stream.stop_capture();
        }
    }

    /// Wakes the failure-monitor loop so it can exit during capturer shutdown.
    pub fn stop_monitor(&self) {
        let _ = self.tx.send(CameraStreamMessage::Stop);
    }

    /// Stops the old device before reopening it under a fresh capture generation.
    /// Exhausted retries leave capture stopped and report the failure to the UI.
    pub fn restart_stream(&mut self) {
        std::thread::sleep(Duration::from_millis(200));

        self.stream = match self.stream.take() {
            Some(mut stream) => {
                stream.stop_capture();
                self.stream_config.next_capture_generation();

                let failures = stream.get_failures_count();
                if failures > MAX_CAMERA_FAILURES_BEFORE_STOP {
                    log::error!("CameraCapturer: too many failures ({failures}), giving up");
                    if let Some(socket) = &self.socket {
                        let _ = socket.send(Message::CameraFailed("Too many failures".to_string()));
                    }
                    return;
                }

                // Try to create a new stream (up to 5 attempts)
                for i in 0..MAX_CAMERA_FAILURES_BEFORE_STOP {
                    std::thread::sleep(Duration::from_millis(100));
                    match stream.copy() {
                        Ok(new_stream) => {
                            log::info!("CameraCapturer: restarted on attempt {i}");
                            return self.stream = Some(new_stream);
                        }
                        Err(e) => {
                            log::warn!("CameraCapturer: restart attempt {i} failed: {e}");
                        }
                    }
                }

                log::error!("CameraCapturer: all restart attempts exhausted");
                if let Some(socket) = &self.socket {
                    let _ = socket.send(Message::CameraFailed("Failed after retries".to_string()));
                }
                None
            }
            None => None,
        };
    }

    /// Adjusts only the outgoing video budget while retaining the input request.
    pub fn set_screensharing_active(&self, active: bool) {
        let target = if active {
            self.stream_config.set_low_quality();
            "640x360@15fps"
        } else {
            self.stream_config.set_high_quality();
            "1280x720@30fps"
        };
        log::info!("CameraCapturer::set_screensharing_active: active={active}, target={target}");
    }

    /// Saves the input preset and immediately reopens an active camera if changed.
    pub fn set_input_resolution(&mut self, resolution: CameraInputResolution) {
        if self.stream_config.input_resolution() == resolution {
            return;
        }
        self.stream_config.set_input_resolution(resolution);
        log::info!("CameraCapturer::set_input_resolution: {resolution:?}");
        if self.stream.is_some() {
            self.restart_stream();
        }
    }
}

impl Drop for CameraCapturer {
    /// Stops device capture before terminating asynchronous failure monitoring.
    fn drop(&mut self) {
        self.stop_capture();
        self.stop_monitor();
    }
}

/// Forwards current-session failures to the UI and discards retired generations.
pub fn poll_camera_stream(capturer: Arc<Mutex<CameraCapturer>>) {
    let rx = { capturer.lock().unwrap().rx.clone() };
    loop {
        let rx_lock = rx.lock();
        if rx_lock.is_err() {
            log::error!("poll_camera_stream: rx lock error");
            break;
        }
        let rx_lock = rx_lock.unwrap();
        match rx_lock.recv_timeout(Duration::from_secs(POLL_CAMERA_TIMEOUT_SECS)) {
            Ok(CameraStreamMessage::Failed { reason, generation }) => {
                log::info!("poll_camera_stream: camera failed: {reason}");
                let capturer = capturer.lock().unwrap();
                if generation != capturer.stream_config.capture_generation() {
                    continue;
                }
                if let Some(socket) = &capturer.socket {
                    let _ = socket.send(Message::CameraFailed(reason));
                }
            }
            Ok(CameraStreamMessage::Stop) => {
                log::info!("poll_camera_stream: stop message");
                break;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {} // timeout, loop again
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                log::info!("poll_camera_stream: channel disconnected");
                break;
            }
            _ => {}
        }
    }
    log::info!("poll_camera_stream: exiting");
}
