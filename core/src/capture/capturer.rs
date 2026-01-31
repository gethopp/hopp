use base64::prelude::*;
use image::codecs::jpeg::JpegEncoder;
use livekit::webrtc::{
    desktop_capturer::{CaptureError, DesktopCapturer, DesktopCapturerOptions, DesktopFrame},
    native::yuv_helper,
    video_frame::{native::VideoFrameBufferExt, NV12Buffer},
    video_source::native::NativeVideoSource,
};

use socket_lib::{CaptureContent, Content, ContentType};
use winit::{event_loop::EventLoopProxy, monitor::MonitorHandle};

use crate::{
    utils::geometry::{aspect_fit, Extent},
    UserEvent, STREAM_FAILURE_EXIT_CODE,
};
use std::sync::{mpsc, Arc, Mutex};
use std::vec;

#[path = "stream.rs"]
mod stream;
use stream::{Stream, StreamRuntimeMessage};

// Constants for magic numbers
const JPEG_QUALITY: u8 = 70;
const THUMBNAIL_WIDTH: f64 = 960.0;
const THUMBNAIL_HEIGHT: f64 = 720.0;
const SCREENSHOT_CAPTURE_SLEEP_MS: u64 = 33;
const SCREENSHOT_CAPTURE_RATE_SLEEP_MS: u64 = 16;
const SCREENSHOT_TIMEOUT: u64 = 10;
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

    /// Failed to capture screenshot frames within the expected timeout.
    ///
    /// This error occurs when the screenshot capture process cannot complete
    /// successfully within the retry limit ({SCREENSHOT_TIMEOUT} attempts).
    /// Common causes include:
    #[error("Failed to capture frames")]
    FailedToCaptureFrames,

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
    /// Retrieves the size dimensions of a specific monitor.
    ///
    /// # Parameters
    /// - `monitors`: A list of all available monitors from the window system
    /// - `input_id`: The identifier of the target monitor to get dimensions for
    ///
    /// # Returns
    /// An `Extent` containing the width and height of the specified monitor.
    /// If the monitor ID is not found, returns the dimensions of the first monitor.
    fn get_monitor_size(monitors: &[MonitorHandle], input_id: u32) -> Extent;

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
}

fn raw_image_to_jpeg(raw_image: Vec<u8>, width: u32, height: u32) -> Vec<u8> {
    let mut jpeg = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY);
    match encoder.encode(&raw_image, width, height, image::ExtendedColorType::Rgb8) {
        Ok(_) => jpeg,
        Err(e) => {
            log::error!("Failed to encode raw image to jpeg: {e:?}");
            Vec::new()
        }
    }
}

fn screenshot_capture_callback(
    target_extent: Extent,
    display_id: u32,
    display_title: String,
    content: Arc<Mutex<Vec<CaptureContent>>>,
) -> impl Fn(Result<DesktopFrame, CaptureError>) {
    log::debug!(
        "screenshot_capture_callback: display_id: {display_id}, display_title: {display_title}"
    );
    move |result: Result<DesktopFrame, CaptureError>| {
        let frame = match result {
            Ok(frame) => frame,
            Err(CaptureError::Temporary) => {
                log::warn!("Capture frame, temporary error");
                return;
            }
            Err(CaptureError::Permanent) => {
                log::info!("Capture frame, permanent error for display: {display_id}, title: {display_title}");
                let mut content = content.lock().unwrap();
                content.push(CaptureContent {
                    content: Content {
                        content_type: ContentType::Display,
                        id: display_id,
                    },
                    base64: "".to_string(),
                    title: display_title.clone(),
                });
                return;
            }
            Err(CaptureError::UserStopped) => {
                log::info!(
                    "Capture frame, user stopped for display: {display_id}, title: {display_title}"
                );
                return;
            }
        };

        /* Skip processing if there is content for this display */
        {
            let content = content.lock().unwrap();
            for c in content.iter() {
                if c.content.id == display_id {
                    return;
                }
            }
        }

        let frame_height = frame.height();
        let frame_width = frame.width();
        let frame_stride = frame.stride();
        let frame_data = frame.data();
        log::info!(
            "screenshot_capture_callback: Frame: {frame_width}x{frame_height}, stride: {frame_stride}",
        );

        let (width, height) = aspect_fit(
            frame_width as u32,
            frame_height as u32,
            target_extent.width as u32,
            target_extent.height as u32,
        );
        log::info!(
            "screenshot_capture_callback: Frame: {frame_width}x{frame_height}, target: {width}x{height}"
        );

        /* Remove extra padding */
        let stride = 4 * frame_width;
        let mut raw_image: Vec<u8> = Vec::with_capacity((stride * frame_height) as usize);
        for i in 0..(frame_height as usize) {
            let start = i * (frame_stride as usize);
            let end = start + (stride as usize);
            raw_image.extend_from_slice(&frame_data[start..end]);
        }

        let mut framebuffer = NV12Buffer::new(frame_width as u32, frame_height as u32);
        let (stride_y, stride_uv) = framebuffer.strides();
        let (data_y, data_uv) = framebuffer.data_mut();
        yuv_helper::argb_to_nv12(
            &raw_image,
            stride as u32,
            data_y,
            stride_y,
            data_uv,
            stride_uv,
            frame_width,
            frame_height,
        );

        let scaled_framebuffer = framebuffer.scale(width as i32, height as i32);
        let capacity = width * 4 * height;
        let mut raw_image_rgba: Vec<u8> = Vec::with_capacity((capacity) as usize);
        for _ in 0..(capacity) {
            raw_image_rgba.extend_from_slice(&[0]);
        }
        scaled_framebuffer.to_argb(
            livekit::webrtc::prelude::VideoFormatType::BGRA,
            &mut raw_image_rgba,
            width * 4,
            width as i32,
            height as i32,
        );

        let mut raw_image: Vec<u8> = Vec::with_capacity((width * 3 * height) as usize);
        for chunk in raw_image_rgba.chunks_exact(4) {
            raw_image.extend_from_slice(&chunk[1..4]);
        }

        let buffer = raw_image_to_jpeg(raw_image, width, height);

        let base64 = BASE64_STANDARD.encode(&buffer);
        let base64 = format!("data:image/{};base64,{}", "jpeg", base64);

        let mut content = content.lock().unwrap();
        content.push(CaptureContent {
            content: Content {
                content_type: ContentType::Display,
                id: display_id,
            },
            base64,
            title: display_title.clone(),
        });
        log::info!(
            "screenshot_capture_callback: Added display: {display_id}, title: {display_title}"
        );
    }
}

/// Main interface for managing screen capture operations and stream lifecycle.
///
/// The `Capturer` serves as the primary coordinator for screen capture functionality,
/// managing stream creation, lifecycle events, error handling, and communication
/// with the UI layer. It maintains a single active capture stream and provides
/// methods for discovering available capture sources, starting/stopping captures,
/// and handling runtime errors through automatic stream recovery.
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
    /// A new `Capturer` instance ready to discover and capture screen sources.
    ///
    /// # Notes
    /// The capturer is created in an idle state with no active streams.
    /// Use `get_available_content()` to discover sources and `start_capture()` to begin capturing.
    pub fn new(event_loop_proxy: EventLoopProxy<UserEvent>) -> Self {
        let (tx, rx) = mpsc::channel();
        Capturer {
            rx: Arc::new(Mutex::new(rx)),
            tx,
            active_stream: None,
            event_loop_proxy,
        }
    }

    /// Discovers and captures thumbnails of all available screen sources.
    ///
    /// # Returns
    /// - `Ok(Vec<CaptureContent>)`: List of available capture sources with thumbnail previews
    /// - `Err(CapturerError)`: Failed to enumerate sources or capture thumbnails
    ///
    /// # Behavior
    /// - Creates temporary capturers for each available display/window
    /// - Captures a single frame from each source at THUMBNAIL_WIDTH x THUMBNAIL_HEIGHT resolution
    /// - Converts frames to base64-encoded JPEG thumbnails for display in UI
    /// - Times out after SCREENSHOT_TIMEOUT if sources don't respond
    ///
    /// # Notes
    /// This method assumes that source list IDs match the display IDs from winit.
    /// The thumbnails are intended for source selection UI and are not suitable for streaming.
    pub fn get_available_content(&mut self) -> Result<Vec<CaptureContent>, CapturerError> {
        #[cfg(any(target_os = "windows", target_os = "macos"))]
        {
            use livekit::webrtc::desktop_capturer::DesktopCaptureSourceType;

            #[allow(unused_mut)]
            let mut options = DesktopCapturerOptions::new(DesktopCaptureSourceType::Screen);
            #[cfg(target_os = "macos")]
            {
                options.set_sck_system_picker(false);
            }
            // We can't clone options so we pass it by value.
            // For subsequent usages we need to recreate it.
            let first_capturer = DesktopCapturer::new(options);
            if first_capturer.is_none() {
                return Err(CapturerError::DesktopCapturerCreationError);
            }
            let first_capturer = first_capturer.unwrap();
            // We are making the assumption that the source list id
            // is matching the display id we get from winit.
            let displays = first_capturer.get_source_list();
            log::info!("get_available_content: displays: {}", displays.len());

            let mut handles = vec![];
            let result = Arc::new(Mutex::new(vec![]));
            let target_dims = Extent {
                width: THUMBNAIL_WIDTH,
                height: THUMBNAIL_HEIGHT,
            };
            for display in displays.iter() {
                let (sender, receiver) = std::sync::mpsc::channel();
                let display_clone = display.clone();
                let result_clone = result.clone();
                let handle = std::thread::spawn(move || {
                    let callback = screenshot_capture_callback(
                        target_dims,
                        display_clone.id() as u32,
                        if display_clone.title() != "" {
                            display_clone.title()
                        } else {
                            format!("Display {}", display_clone.id())
                        },
                        result_clone,
                    );
                    #[allow(unused_mut)]
                    let mut options = DesktopCapturerOptions::new(DesktopCaptureSourceType::Screen);
                    #[cfg(target_os = "macos")]
                    {
                        options.set_sck_system_picker(false);
                    }
                    let capturer = DesktopCapturer::new(options);
                    if capturer.is_none() {
                        log::error!(
                            "Failed to create DesktopCapturer for display: {}",
                            display_clone.id()
                        );
                        return;
                    }
                    let mut capturer = capturer.unwrap();
                    capturer.start_capture(Some(display_clone), callback);

                    loop {
                        match receiver.recv_timeout(std::time::Duration::from_millis(
                            SCREENSHOT_CAPTURE_RATE_SLEEP_MS,
                        )) {
                            Ok(()) => break,
                            Err(e) => match e {
                                mpsc::RecvTimeoutError::Timeout => {
                                    capturer.capture_frame();
                                }
                                mpsc::RecvTimeoutError::Disconnected => {
                                    log::error!("get_available_content: capture loop disconnected");
                                    break;
                                }
                            },
                        }
                    }
                });

                handles.push((handle, sender));
            }

            let now = std::time::Instant::now();
            let mut failed = false;
            loop {
                {
                    let res = result.lock().unwrap();
                    if res.len() == displays.len() {
                        break;
                    }
                }

                std::thread::sleep(std::time::Duration::from_millis(
                    SCREENSHOT_CAPTURE_SLEEP_MS,
                ));

                if now.elapsed().as_secs() > SCREENSHOT_TIMEOUT {
                    failed = true;
                    break;
                }
            }

            while let Some((handle, sender)) = handles.pop() {
                match sender.send(()) {
                    Ok(()) => {
                        let _ = handle.join();
                    }
                    Err(_) => {
                        log::error!("failed to send stop message to screenshot capture thread")
                    }
                }
            }

            if failed {
                return Err(CapturerError::FailedToCaptureFrames);
            }

            let res = result.lock().unwrap();
            Ok((*res).clone())
        }
        /*
         * On linux desktop capture is using the system picker so we can't get
         * screenshots the way we do on macos and windows.
         */
        #[cfg(target_os = "linux")]
        {
            let capturer = DesktopCapturer::new(|_, _| {}, false, false);
            if capturer.is_none() {
                return Err(CapturerError::DesktopCapturerCreationError);
            }
            let capturer = capturer.unwrap();
            let sources = capturer.get_source_list();
            if sources.is_empty() {
                log::error!("Capturer returned 0 sources");
                return Ok(vec![]);
            }
            let display = &sources[0];
            Ok(vec![CaptureContent {
                content: Content {
                    content_type: ContentType::Display,
                    id: display.id() as u32,
                },
                base64: "".to_string(),
                title: display.title().clone(),
            }])
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
    ) -> Result<(), CapturerError> {
        log::info!("start_capture: content {content:?} resolution: {stream_resolution:?} include_cursor: {include_cursor}");
        if self.active_stream.is_some() {
            log::warn!("start_capture: active stream, stopping it");
            self.active_stream.as_mut().unwrap().stop_capture();
            self.active_stream = None;
        }

        let scale = 1.0;
        let mut stream = Stream::new(stream_resolution, scale, self.tx.clone(), include_cursor)?;

        stream.start_capture(content.id)?;
        self.active_stream = Some(stream);
        Ok(())
    }

    /// Stops the currently active capture stream.
    ///
    /// # Behavior
    /// - Terminates the capture worker thread gracefully
    /// - Cleans up stream resources and buffers
    /// - Resets the capturer to idle state
    /// - Safe to call when no stream is active (no-op)
    ///
    /// # Notes
    /// This method blocks until the capture thread has fully terminated.
    /// After calling this method, `start_capture()` can be called to begin
    /// capturing from a new or different source.
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
                    std::process::exit(STREAM_FAILURE_EXIT_CODE);
                }

                let mut new_stream = match stream.copy() {
                    Ok(new_stream) => new_stream,
                    Err(_) => {
                        log::error!("restart_stream: Failed to copy stream");
                        sentry_utils::upload_logs_event("Stream copy failed".to_string());
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

    /// Gets the size of a specific monitor by ID.
    ///
    /// # Parameters
    /// - `monitors`: List of available monitor handles from the window system
    /// - `input_id`: The identifier of the target monitor
    ///
    /// # Returns
    /// An `Extent` containing the width and height of the specified monitor.
    /// Falls back to the first monitor if the specified ID is not found.
    ///
    /// # Notes
    /// This is a convenience wrapper around the platform-specific implementation
    /// provided by `ScreenshareFunctions`. The actual behavior varies by platform.
    pub fn get_monitor_size(monitors: &[MonitorHandle], input_id: u32) -> Extent {
        ScreenshareFunctions::get_monitor_size(monitors, input_id)
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

    pub fn set_buffer_source(&mut self, buffer_source: NativeVideoSource) {
        if self.active_stream.is_none() {
            log::error!("set_buffer_source: no active stream");
            return;
        }
        self.active_stream
            .as_mut()
            .unwrap()
            .set_buffer_source(buffer_source);
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
