use livekit::webrtc::video_source::native::NativeVideoSource;

use socket_lib::Content;
use winit::{dpi::PhysicalPosition, event_loop::EventLoopProxy, monitor::MonitorHandle};

use crate::{utils::geometry::Extent, UserEvent, STREAM_FAILURE_EXIT_CODE};

/// Platform-agnostic monitor identifier.
///
/// Different platforms use different types of identifiers for monitors:
/// - macOS uses numeric CGDirectDisplayID
/// - Windows uses device name strings like "\\.\DISPLAY1"
/// - Linux falls back to position-based identification
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonitorId {
    /// Numeric identifier (macOS CGDirectDisplayID)
    Numeric(u32),
    /// Named identifier (Windows device name)
    Named(String),
    /// Position-based identifier (Linux fallback)
    Position(PhysicalPosition<i32>),
}
use std::sync::{mpsc, Arc, Mutex};

#[cfg_attr(target_os = "macos", path = "macos_stream.rs")]
#[cfg_attr(not(target_os = "macos"), path = "stream.rs")]
mod stream;
use stream::{Stream, StreamRuntimeMessage};

// Constants for magic numbers
const MAX_STREAM_FAILURES_BEFORE_EXIT: u64 = 10;
const POLL_STREAM_TIMEOUT_SECS: u64 = 100;
const POLL_STREAM_DATA_SLEEP_MS: u64 = 100;

#[cfg_attr(target_os = "windows", path = "windows.rs")]
#[cfg_attr(target_os = "macos", path = "macos.rs")]
#[cfg_attr(target_os = "linux", path = "linux.rs")]
mod platform;
pub use platform::ScreenshareFunctions;

/// Errors that can occur during screen capturing operations.
///
/// This enum represents various failure modes that can occur when initializing
/// or operating the screen capture system.
#[derive(Debug, thiserror::Error)]
pub enum CapturerError {
    /// Failed to create the underlying desktop capturer instance.
    ///
    /// This error occurs when the system cannot initialize the platform-specific
    /// screen capture functionality. Common causes include:
    #[error("Failed to create DesktopCapturer")]
    DesktopCapturerCreationError,

    /// Capture source list is empty.
    ///
    /// This error could occur when the screen sharing engine fails from the os and
    /// then we try to restart the stream.
    #[error("Capture source list is empty")]
    CaptureSourceListEmpty,

    /// Couldn't find selected source.
    #[error("Couldn't find selected source")]
    SelectedSourceNotFound,
}

/// Platform-specific extensions for screen sharing and monitor management.
///
/// This trait provides platform-specific functionality for handling monitor
/// selection and sizing in the screen capture system. Implementations of this
/// trait are provided by platform-specific modules (windows.rs, macos.rs) and
/// handle the differences in how each operating system manages displays.
pub trait ScreenshareExt {
    /// Selects and returns a specific monitor handle by ID.
    ///
    /// # Parameters
    /// - `monitors`: A list of all available monitors from the window system
    /// - `input_id`: The identifier of the target monitor to select
    ///
    /// # Returns
    /// The `MonitorHandle` for the specified monitor. If the monitor ID is not found,
    /// returns the first available monitor as a fallback.
    fn get_selected_monitor(monitors: &[MonitorHandle], input_id: u32) -> MonitorHandle;

    /// Returns a platform-agnostic identifier for the given monitor.
    ///
    /// # Parameters
    /// - `monitor`: The monitor handle to get the ID for
    ///
    /// # Returns
    /// A `MonitorId` that uniquely identifies this monitor across position changes.
    fn get_monitor_id(monitor: &MonitorHandle) -> MonitorId;

    /// Reverse mapping: returns the capture content id (used by `Content.id`)
    /// for the given monitor, or `None` if it cannot be resolved.
    fn capture_content_id_for_monitor(monitor: &MonitorHandle) -> Option<u32>;
}

/// Main interface for managing screen capture operations and stream lifecycle.
///
/// The `Capturer` serves as the primary coordinator for screen capture functionality,
/// managing stream creation, lifecycle events, error handling, and communication
/// with the UI layer. It maintains a single active capture stream and provides
/// methods for starting/stopping captures and handling runtime errors through
/// automatic stream recovery.
pub struct Capturer {
    /// Receiver for runtime messages from capture streams.
    ///
    /// Wrapped in Arc<Mutex<>> to allow sharing with the polling thread
    /// that monitors for stream failures and other runtime events without
    /// keeping the main Capturer locked during message processing.
    rx: Arc<Mutex<mpsc::Receiver<StreamRuntimeMessage>>>,

    /// Sender for runtime messages to coordinate stream operations.
    ///
    /// Used internally to send control messages and by streams to report
    /// failures and status changes back to the main capturer.
    tx: mpsc::Sender<StreamRuntimeMessage>,

    /// The currently active capture stream, if any.
    ///
    /// Only one stream can be active at a time. When `None`, no capture
    /// is currently in progress.
    active_stream: Option<Stream>,

    /// Event loop proxy for triggering UI updates and application events.
    ///
    /// Used to communicate capture state changes back to the main application,
    /// particularly for updating the UI when users stop screen sharing through
    /// system controls. This ensures proper cleanup of tracks and room connections.
    event_loop_proxy: EventLoopProxy<UserEvent>,
}

impl Capturer {
    /// Creates a new capturer instance.
    ///
    /// # Parameters
    /// - `event_loop_proxy`: Proxy for sending events back to the main application event loop
    ///
    /// # Returns
    /// A new `Capturer` instance ready to capture screen sources.
    ///
    /// # Notes
    /// The capturer is created in an idle state with no active streams.
    /// Use `start_capture()` with a display `Content` id to begin capturing.
    pub fn new(event_loop_proxy: EventLoopProxy<UserEvent>) -> Self {
        let (tx, rx) = mpsc::channel();
        Capturer {
            rx: Arc::new(Mutex::new(rx)),
            tx,
            active_stream: None,
            event_loop_proxy,
        }
    }

    /// Starts capturing frames from the specified content source.
    ///
    /// # Parameters
    /// - `content`: The content source to capture (display or window with display_id)
    /// - `stream_resolution`: The resolution of the stream buffer
    /// - `include_cursor`: Whether to include the cursor in the capture
    ///
    /// # Returns
    /// - `Ok(())`: Successfully started the capture stream
    /// - `Err(CapturerError)`: Failed to create or start the capture stream
    ///
    /// # Behavior
    /// - Stops any existing active stream
    /// - Selects the appropriate monitor based on the content's display_id
    /// - Creates a new capture stream configured for the target resolution
    /// - Starts the capture loop and frame processing pipeline
    ///
    /// # Notes
    /// Only one stream can be active at a time. Starting a new capture automatically
    /// stops the previous one. The returned monitor handle represents the physical
    /// display being captured.
    pub fn start_capture(
        &mut self,
        content: Content,
        stream_resolution: Extent,
        include_cursor: bool,
        buffer_source: NativeVideoSource,
        scale: f64,
    ) -> Result<(), CapturerError> {
        log::info!("start_capture: content {content:?} resolution: {stream_resolution:?} include_cursor: {include_cursor} scale: {scale}");
        if self.active_stream.is_some() {
            log::warn!("start_capture: active stream, stopping it");
            self.active_stream.as_mut().unwrap().stop_capture();
            self.active_stream = None;
        }

        let mut stream = Stream::new(
            stream_resolution,
            scale,
            self.tx.clone(),
            include_cursor,
            buffer_source,
        )?;

        stream.start_capture(content.id)?;
        self.active_stream = Some(stream);
        Ok(())
    }

    /// Updates whether the active stream includes the system cursor.
    /// Restarts the stream only when the policy changes.
    pub fn set_include_cursor(&mut self, include_cursor: bool) {
        let Some(mut stream) = self.active_stream.take() else {
            return;
        };
        if !stream.set_include_cursor(include_cursor) {
            self.active_stream = Some(stream);
            return;
        }

        log::info!("set_include_cursor: applying cursor policy change");
        let Ok(mut stream) = stream.copy() else {
            log::error!("set_include_cursor: failed to copy capture stream");
            let _ = self.event_loop_proxy.send_event(UserEvent::StopScreenShare);
            return;
        };
        let source_id = stream.source_id();
        if let Err(error) = stream.start_capture(source_id) {
            log::error!("set_include_cursor: failed to restart capture stream: {error:?}");
            let _ = self.event_loop_proxy.send_event(UserEvent::StopScreenShare);
            return;
        }
        self.active_stream = Some(stream);
    }

    /// Signals the capture thread to stop and releases the active stream.
    /// The thread is detached and will exit on its own.
    /// Safe to call when no stream is active (no-op).
    pub fn stop_capture(&mut self) {
        log::info!("stop_capture");
        if self.active_stream.is_none() {
            log::warn!("stop_capture: no active stream");
            return;
        }
        self.active_stream.as_mut().unwrap().stop_capture();
        self.active_stream = None;
    }

    /// Restarts the current stream to recover from permanent errors.
    ///
    /// # Behavior
    /// - Stops the current stream if running
    /// - Checks failure count and exits process if too many consecutive failures
    /// - Creates a new stream instance sharing the same buffers and configuration
    /// - Restarts capture on the same source ID
    /// - Preserves failure tracking across restart
    ///
    /// # Error Handling
    /// If the failure count exceeds MAX_STREAM_FAILURES_BEFORE_EXIT, the process
    /// will exit with STREAM_FAILURE_EXIT_CODE to trigger application restart.
    /// This prevents infinite restart loops when the capture system is fundamentally broken.
    ///
    /// # Notes
    /// This method is typically called automatically by the polling thread when
    /// permanent capture errors are detected. Manual calls should be rare.
    pub fn restart_stream(&mut self) {
        log::info!("restart_stream");
        std::thread::sleep(std::time::Duration::from_millis(200));

        self.active_stream = match self.active_stream.take() {
            Some(mut stream) => {
                stream.stop_capture();

                // If something fails here we are killing the process in
                // order to trigger the health check in the tauri app.
                // The health check will instruct the user to restart.
                // We should do this via a message in the future.
                let failures_count = stream.get_failures_count();
                if failures_count > MAX_STREAM_FAILURES_BEFORE_EXIT {
                    log::error!("restart_stream: Too many failures, killing the process");
                    sentry_utils::upload_logs_event("Stream failed".to_string());
                    sentry_utils::flush(std::time::Duration::from_secs(2));
                    std::process::exit(STREAM_FAILURE_EXIT_CODE);
                }

                let mut new_stream = match stream.copy() {
                    Ok(new_stream) => new_stream,
                    Err(_) => {
                        log::error!("restart_stream: Failed to copy stream");
                        sentry_utils::upload_logs_event("Stream copy failed".to_string());
                        sentry_utils::flush(std::time::Duration::from_secs(2));
                        std::process::exit(STREAM_FAILURE_EXIT_CODE);
                    }
                };

                // Sometimes the capturer fails with a permanent error from the os.
                // We can't really do much about it, as we are relying on the os
                // and DesktopCapturer from libwebrtc for capturing the screen.
                // So we just sleep and retry a few times in case it's a temporary error.
                // If we can't restart the stream after 10 retries, we exit the process
                // and inform the user to restart the application.
                let mut res = new_stream.start_capture(new_stream.source_id());
                for i in 0..MAX_STREAM_FAILURES_BEFORE_EXIT {
                    if res.is_ok() {
                        break;
                    }

                    if matches!(res, Err(CapturerError::SelectedSourceNotFound)) {
                        log::info!("restart_stream: Source not found, stopping screen share");
                        let _ = self.event_loop_proxy.send_event(UserEvent::StopScreenShare);
                        return;
                    }

                    log::info!("restart_stream: Failed to start capture, retrying {i}/10 {res:?}");
                    std::thread::sleep(std::time::Duration::from_millis(100));

                    new_stream = match new_stream.copy() {
                        Ok(new_stream) => new_stream,
                        Err(_) => {
                            log::error!("restart_stream: Failed to copy stream");
                            sentry_utils::upload_logs_event("Stream copy failed".to_string());
                            sentry_utils::flush(std::time::Duration::from_secs(2));
                            std::process::exit(STREAM_FAILURE_EXIT_CODE);
                        }
                    };
                    res = new_stream.start_capture(new_stream.source_id());
                }

                if let Err(ref e) = res {
                    if matches!(e, CapturerError::SelectedSourceNotFound) {
                        log::info!(
                            "restart_stream: Source not found after retries, stopping screen share"
                        );
                        let _ = self.event_loop_proxy.send_event(UserEvent::StopScreenShare);
                        return;
                    }
                    log::error!("restart_stream: Failed to start capture after 10 retries {res:?}");
                    sentry_utils::upload_logs_event("Stream start capture failed".to_string());
                    sentry_utils::flush(std::time::Duration::from_secs(2));
                    std::process::exit(STREAM_FAILURE_EXIT_CODE);
                }

                log::info!("restart_stream: new stream created");
                Some(new_stream)
            }
            None => None,
        };
    }

    /// Checks if there is currently an active capture stream.
    ///
    /// # Returns
    /// - `true`: A capture stream is currently active and capturing frames
    /// - `false`: No capture is in progress
    pub fn has_active_stream(&self) -> bool {
        self.active_stream.is_some()
    }

    /// Signals the runtime stream monitoring thread to terminate.
    ///
    /// # Behavior
    /// Sends a `Stop` message to the polling thread that monitors for stream
    /// failures and runtime events. This is used during application shutdown
    /// to ensure all capture-related threads terminate cleanly.
    ///
    /// # Notes
    /// This method should be called before dropping the Capturer instance to
    /// prevent the polling thread from running indefinitely. The method is
    /// non-blocking and returns immediately after sending the stop signal.
    pub fn stop_runtime_stream_handler(&self) {
        let res = self.tx.send(StreamRuntimeMessage::Stop);
        if let Err(e) = res {
            log::error!("stop_runtime_stream_handler: error sending Stop message: {e}");
        }
    }

    pub fn get_selected_monitor(&self, monitors: &[MonitorHandle], input_id: u32) -> MonitorHandle {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            ScreenshareFunctions::get_selected_monitor(monitors, input_id)
        }
        #[cfg(target_os = "linux")]
        {
            if self.active_stream.is_none() {
                log::warn!("get_selected_monitor: no active stream");
                return monitors[0].clone();
            }
            let capturer = self.active_stream.as_ref().unwrap().capturer();
            let capturer = capturer.lock().unwrap();
            for _ in 0..150 {
                let rect = capturer.get_source_rect();
                if rect.top != 0 || rect.left != 0 || rect.width != 0 || rect.height != 0 {
                    for monitor in monitors {
                        let position = monitor.position();
                        let size = monitor.size();
                        if position.x == rect.left
                            && position.y == rect.top
                            && size.width == (rect.width as u32)
                            && size.height == (rect.height as u32)
                        {
                            return monitor.clone();
                        }
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(POLL_STREAM_DATA_SLEEP_MS));
            }
            log::error!("get_selected_monitor: capturer hasn't started");
            return monitors[0].clone();
        }
    }

    pub fn get_stream_extent(&self) -> Extent {
        if self.active_stream.is_none() {
            log::error!("get_stream_extent: no active stream");
            return Extent {
                width: 0.,
                height: 0.,
            };
        }
        let stream = self.active_stream.as_ref().unwrap();
        for i in 0..150 {
            let extent = stream.get_stream_extent();
            if extent.width > 1. && extent.height > 1. {
                log::info!("get_stream_extent: got extent in try {i}");
                return extent;
            }
            std::thread::sleep(std::time::Duration::from_millis(POLL_STREAM_DATA_SLEEP_MS));
        }
        Extent {
            width: 0.,
            height: 0.,
        }
    }
}

/*
 * This function is spawned in a separate thread and
 * is used for checking whether the stream failed, if it
 * failed it restarts it.
 *
 * This thread is owned by the Application struct.
 */
pub fn poll_stream(capturer: Arc<Mutex<Capturer>> /* mut socket: CursorSocket */) {
    let rx = { capturer.lock().unwrap().rx.clone() };
    loop {
        log::debug!("poll_stream: waiting for message");
        let rx_lock = rx.lock();
        if rx_lock.is_err() {
            log::error!("poll_stream: rx lock error");
            break;
        }
        let rx_lock = rx_lock.unwrap();
        match rx_lock.recv_timeout(std::time::Duration::from_secs(POLL_STREAM_TIMEOUT_SECS)) {
            Ok(StreamRuntimeMessage::Failed) => {
                log::info!("poll_stream: stream failed");
                let mut capturer = capturer.lock().unwrap();
                capturer.restart_stream();
            }
            Ok(StreamRuntimeMessage::UserStoppedCapture) => {
                log::info!("poll_stream: user stopped capture");
                let capturer = capturer.lock().unwrap();
                let _ = capturer
                    .event_loop_proxy
                    .send_event(UserEvent::StopScreenShare);
            }
            Ok(StreamRuntimeMessage::Stop) => {
                log::info!("poll_stream: stop message");
                break;
            }
            Err(_) => {}
            _ => {}
        };
    }
}
