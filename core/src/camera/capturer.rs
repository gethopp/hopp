use livekit::webrtc::video_source::native::NativeVideoSource;
use socket_lib::{CameraDevice, Message, SocketSender};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use super::stream::{CameraStream, CameraStreamMessage};

const MAX_CAMERA_FAILURES_BEFORE_STOP: u32 = 5;
const POLL_CAMERA_TIMEOUT_SECS: u64 = 100;

pub struct CameraCapturer {
    stream: Option<CameraStream>,
    rx: Arc<Mutex<mpsc::Receiver<CameraStreamMessage>>>,
    tx: mpsc::Sender<CameraStreamMessage>,
    socket: Option<SocketSender>,
}

impl Default for CameraCapturer {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraCapturer {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            stream: None,
            rx: Arc::new(Mutex::new(rx)),
            tx,
            socket: None,
        }
    }

    pub fn list_devices() -> Vec<CameraDevice> {
        super::stream::list_devices()
    }

    pub fn start_capture(
        &mut self,
        device_name: &str,
        socket: SocketSender,
    ) -> Result<(u32, u32), String> {
        self.stop_capture();
        self.socket = Some(socket);

        log::info!("CameraCapturer::start_capture: device='{device_name}'");
        let stream = CameraStream::new(device_name, self.tx.clone())?;
        let extent = stream.extent();
        self.stream = Some(stream);
        Ok(extent)
    }

    pub fn stop_capture(&mut self) {
        if let Some(mut stream) = self.stream.take() {
            log::info!("CameraCapturer::stop_capture");
            stream.stop_capture();
        }
    }

    pub fn set_buffer_source(&self, source: NativeVideoSource) {
        if let Some(stream) = self.stream.as_ref() {
            stream.set_buffer_source(source);
        }
    }

    pub fn stop_monitor(&self) {
        let _ = self.tx.send(CameraStreamMessage::Stop);
    }

    pub fn restart_stream(&mut self) {
        std::thread::sleep(Duration::from_millis(200));

        self.stream = match self.stream.take() {
            Some(mut stream) => {
                stream.stop_capture();

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
}

impl Drop for CameraCapturer {
    fn drop(&mut self) {
        self.stop_capture();
        self.stop_monitor();
    }
}

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
            Ok(CameraStreamMessage::Failed(reason)) => {
                log::info!("poll_camera_stream: camera failed: {reason}");
                let mut capturer = capturer.lock().unwrap();
                capturer.restart_stream();
            }
            Ok(CameraStreamMessage::Stop) => {
                log::info!("poll_camera_stream: stop message");
                break;
            }
            Err(_) => {} // timeout, loop again
            _ => {}
        }
    }
    log::info!("poll_camera_stream: exiting");
}
