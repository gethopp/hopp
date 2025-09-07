use crate::utils::geometry::{aspect_fit, Extent, Frame};
use livekit::webrtc::{
    desktop_capturer::{CaptureResult, DesktopCapturer, DesktopFrame},
    native::yuv_helper,
    prelude::{NV12Buffer, VideoBuffer, VideoFrame, VideoRotation},
    video_source::native::NativeVideoSource,
};
use std::{
    sync::{mpsc, Arc, Mutex},
    thread::JoinHandle,
};
use sysinfo::System;

use super::CapturerError;

const FRAME_CAPTURE_INTERVAL_MS: u64 = 16;

/// Messages used for inter-thread communication in the stream capture system.
///
/// This enum defines the control messages that can be sent between the main thread
/// and the capture worker threads to coordinate stream lifecycle events and error handling.
pub enum StreamRuntimeMessage {
    /// Indicates that the stream has permanently failed and needs to be restarted.
    ///
    /// This message is sent when the capture system encounters a permanent error
    /// that cannot be recovered from without creating a new stream instance.
    /// The main thread will attempt to restart the stream when receiving this message.
    Failed,

    /// Requests that the stream polling thread should terminate.
    ///
    /// This message is sent to gracefully shut down the stream monitoring thread
    /// that checks for runtime messages. Used during application shutdown or
    /// when stopping the capture system entirely.
    Stop,

    /// Requests that the frame capture thread should stop capturing.
    ///
    /// This message is sent to the worker thread that continuously captures frames
    /// to signal it should stop the capture loop and terminate. Used when stopping
    /// or restarting a stream.
    StopCapture,

    /// Indicates that the user has manually stopped the capture.
    ///
    /// This message is sent when the capture system detects that the user has
    /// stopped screen sharing through system controls (e.g., macOS screen recording
    /// permission dialog). This triggers a UI update to reflect the stopped state.
    UserStoppedCapture,
}

/// Buffer for holding video frame data in the streaming pipeline.
struct StreamBuffer {
    /// The video frame containing NV12-formatted pixel data.
    video_frame: VideoFrame<NV12Buffer>,
}

impl StreamBuffer {
    /// Creates a new stream buffer with the specified dimensions.
    ///
    /// # Parameters
    /// - `width`: The width of the video frame in pixels
    /// - `height`: The height of the video frame in pixels
    ///
    /// # Returns
    /// A new `StreamBuffer` instance with an initialized NV12 buffer of the given
    /// dimensions.
    pub fn new(width: u32, height: u32) -> Self {
        let video_frame = VideoFrame {
            rotation: VideoRotation::VideoRotation0,
            buffer: NV12Buffer::new(width, height),
            timestamp_us: 0,
        };
        StreamBuffer { video_frame }
    }
}

/*
 * This function is used to get the pids of the applications that we want to exclude
 * from the capturing.
 */
fn get_excluded_application_pids() -> Vec<u64> {
    let system = System::new_all();
    let mut pids = vec![];
    for (pid, process) in system.processes() {
        if let Some(name) = process.name().to_str() {
            if name.contains("hopp") {
                pids.push(pid.as_u32() as u64);
            }
        }
    }
    log::info!("get_excluded_application_pids: {pids:?}");
    pids
}

fn create_capture_callback(
    buffer_source: Arc<Mutex<Option<NativeVideoSource>>>,
    resolution: Extent,
    stream_buffer: Arc<Mutex<StreamBuffer>>,
    desktop_frame: Arc<Mutex<Frame>>,
    tx: mpsc::Sender<StreamRuntimeMessage>,
    failures_count: Arc<Mutex<u64>>,
) -> impl Fn(CaptureResult, DesktopFrame) {
    let capture_buffer = Arc::new(Mutex::new(NV12Buffer::new(0, 0)));
    move |result: CaptureResult, frame: DesktopFrame| {
        match result {
            CaptureResult::ErrorTemporary => {
                log::warn!("Capture frame, temporary error");
                return;
            }
            CaptureResult::ErrorPermanent => {
                log::info!("Capture frame, permanent error");
                let mut failures_count = failures_count.lock().unwrap();
                *failures_count += 1;
                let res = tx.send(StreamRuntimeMessage::Failed);
                if let Err(e) = res {
                    log::error!("Failed to send Failed message: {e}");
                }
                return;
            }
            CaptureResult::ErrorUserStopped => {
                log::info!("Capture frame, user stopped");
                let res = tx.send(StreamRuntimeMessage::UserStoppedCapture);
                if let Err(e) = res {
                    log::error!("Failed to send Failed message: {e}");
                }
                return;
            }
            _ => {
                let mut failures_count = failures_count.lock().unwrap();
                *failures_count = 0;
            }
        }
        let frame_height = frame.height();
        let frame_width = frame.width();
        let frame_stride = frame.stride();
        if frame_width == 0 || frame_height == 0 {
            log::warn!("Capture frame frame dims zero {frame_width}x{frame_height}");
            return;
        }
        let frame_top = frame.top();
        let frame_left = frame.left();
        let frame_data = frame.data();
        log::trace!(
            "capture_callback: Frame: {frame_width}x{frame_height}, stride: {frame_stride}",
        );

        {
            let mut frame = desktop_frame.lock().unwrap();
            if (frame_top != (frame.origin_y as i32))
                || (frame_left != (frame.origin_x as i32))
                || (frame_width != (frame.extent.width as i32))
                || (frame_height != (frame.extent.height as i32))
            {
                frame.extent.width = frame_width as f64;
                frame.extent.height = frame_height as f64;
                frame.origin_x = frame_left as f64;
                frame.origin_y = frame_top as f64;
            }
        }

        // Copy DesktopFrame to framebuffer
        let mut framebuffer = capture_buffer.lock().unwrap();
        let framebuffer_width = framebuffer.width();
        let framebuffer_height = framebuffer.height();
        if (framebuffer_width != (frame_width as u32))
            || (framebuffer_height != (frame_height as u32))
        {
            *framebuffer = NV12Buffer::new(frame_width as u32, frame_height as u32);
            let (stream_width, stream_height) = aspect_fit(
                frame_width as u32,
                frame_height as u32,
                resolution.width as u32,
                resolution.height as u32,
            );
            let mut stream_buffer = stream_buffer.lock().unwrap();
            *stream_buffer = StreamBuffer::new(stream_width, stream_height);
        }

        let (stride_y, stride_uv) = framebuffer.strides();
        let (data_y, data_uv) = framebuffer.data_mut();
        yuv_helper::argb_to_nv12(
            frame_data,
            frame_stride,
            data_y,
            stride_y,
            data_uv,
            stride_uv,
            frame_width,
            frame_height,
        );

        // Scale framebuffer to stream resolution
        let mut stream_buffer = stream_buffer.lock().unwrap();
        let stream_width = stream_buffer.video_frame.buffer.width();
        let stream_height = stream_buffer.video_frame.buffer.height();
        let mut scaled_buffer = framebuffer.scale(stream_width as i32, stream_height as i32);
        drop(framebuffer);

        // Copy scaled buffer to stream buffer
        let (data_y, data_uv) = scaled_buffer.data_mut();
        let (dst_y, dst_uv) = stream_buffer.video_frame.buffer.data_mut();
        dst_y.copy_from_slice(data_y);
        dst_uv.copy_from_slice(data_uv);

        let buffer_source = buffer_source.lock().unwrap();
        if buffer_source.is_some() {
            buffer_source
                .as_ref()
                .unwrap()
                .capture_frame(&stream_buffer.video_frame);
        }
    }
}

fn run_capture_frame(
    rx: mpsc::Receiver<StreamRuntimeMessage>,
    capturer: Arc<Mutex<DesktopCapturer>>,
) {
    loop {
        match rx.recv_timeout(std::time::Duration::from_millis(FRAME_CAPTURE_INTERVAL_MS)) {
            Ok(StreamRuntimeMessage::StopCapture) => {
                break;
            }
            Err(e) => match e {
                mpsc::RecvTimeoutError::Timeout => {
                    let mut capturer = capturer.lock().unwrap();
                    capturer.capture_frame();
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    log::error!("run_capture_frame: Disconnected");
                    break;
                }
            },
            _ => {}
        }
    }
}

/// Manages a single screen capture stream and its associated resources.
///
/// This struct encapsulates all the components needed to capture screen content
/// from a specific source and convert it into a video stream
/// suitable for WebRTC transmission. It handles the capture thread lifecycle,
/// frame buffering, format conversion, and error recovery.
pub struct Stream {
    /// The platform-specific desktop capturer that interfaces with the OS screen capture APIs.
    ///
    /// Wrapped in Arc<Mutex<>> to allow safe sharing between the main thread and
    /// the capture worker thread that continuously captures frames.
    capturer: Arc<Mutex<DesktopCapturer>>,

    /// Handle to the background thread that performs continuous frame capturing.
    ///
    /// This thread runs the capture loop at regular intervals. When `None`,
    /// the capture thread is not running.
    capture_frame_handle: Option<JoinHandle<()>>,

    /// Channel sender for controlling the capture thread lifecycle.
    ///
    /// Used to send stop messages to the capture worker thread. When `None`,
    /// no capture thread is active.
    tx: Option<mpsc::Sender<StreamRuntimeMessage>>,

    /// Channel sender for reporting permanent errors to the main capturer.
    ///
    /// Used to communicate critical failures that require stream restart
    /// or complete capture system shutdown.
    permanent_error_tx: mpsc::Sender<StreamRuntimeMessage>,

    /// Buffer containing the final video frame data for streaming.
    ///
    /// This buffer holds frames that have been processed, scaled, and converted
    /// to the target stream resolution for WebRTC transmission.
    stream_buffer: Arc<Mutex<StreamBuffer>>,

    /// Buffer source for the stream.
    buffer_source: Arc<Mutex<Option<NativeVideoSource>>>,

    /// Metadata about the current capture frame dimensions and position.
    ///
    /// Tracks the actual captured area size and position, which may change
    /// if the source window is resized or moved.
    frame: Arc<Mutex<Frame>>,

    /// The resolution of the stream buffer.
    stream_resolution: Extent,

    /// Identifier of the capture source (display or window ID).
    source_id: u32,

    /// Counter tracking consecutive stream failures for health monitoring.
    ///
    /// Incremented on capture failures and reset on successful captures.
    /// When this reaches MAX_STREAM_FAILURES_BEFORE_EXIT, the process exits
    /// to trigger application restart.
    failures_count: Arc<Mutex<u64>>,

    /// Whether to include the cursor in the capture
    include_cursor: bool,
}

impl Stream {
    /// Creates a new stream instance for capturing from a screen source.
    ///
    /// # Parameters
    /// - `stream_resolution`: The resolution of the stream buffer
    /// - `_scale`: Display scale factor (currently unused but reserved for future scaling)
    /// - `tx`: Channel sender for communicating runtime messages back to the main capturer
    /// - `include_cursor`: Whether to include the cursor in the capture
    ///
    /// # Returns
    /// - `Ok(Stream)`: Successfully created stream ready for capture
    /// - `Err(CapturerError::DesktopCapturerCreationError)`: Failed to initialize the underlying capture system
    pub fn new(
        stream_resolution: Extent,
        _scale: f64,
        tx: mpsc::Sender<StreamRuntimeMessage>,
        include_cursor: bool,
    ) -> Result<Self, CapturerError> {
        let buffer_source = Arc::new(Mutex::new(None));
        let stream_buffer = Arc::new(Mutex::new(StreamBuffer::new(0, 0)));
        let frame = Arc::new(Mutex::new(Frame {
            origin_x: 0.,
            origin_y: 0.,
            extent: Extent {
                width: 0.,
                height: 0.,
            },
        }));
        let failures_count = Arc::new(Mutex::new(0));

        let callback = create_capture_callback(
            buffer_source.clone(),
            stream_resolution,
            stream_buffer.clone(),
            frame.clone(),
            tx.clone(),
            failures_count.clone(),
        );
        let capturer = DesktopCapturer::new(callback, false, include_cursor);
        if capturer.is_none() {
            return Err(CapturerError::DesktopCapturerCreationError);
        }
        let capturer = capturer.unwrap();
        let apps_to_exclude = get_excluded_application_pids();
        capturer.set_excluded_applications(apps_to_exclude);
        Ok(Stream {
            capturer: Arc::new(Mutex::new(capturer)),
            capture_frame_handle: None,
            tx: None,
            permanent_error_tx: tx,
            stream_buffer,
            buffer_source,
            frame,
            stream_resolution,
            source_id: 0,
            failures_count,
            include_cursor,
        })
    }

    /// Starts capturing frames from the specified source.
    ///
    /// # Parameters
    /// - `id`: The identifier of the capture source (display or window ID)
    ///
    /// # Behavior
    /// - Finds the capture source matching the provided ID from available sources
    /// - Falls back to the first available source if the specified ID is not found
    /// - Spawns a background worker thread that continuously captures frames
    /// - Begins the frame capture loop at FRAME_CAPTURE_INTERVAL_MS intervals
    ///
    /// # Notes
    /// This method should only be called when the stream is not already capturing.
    /// The capture thread will run until `stop_capture()` is called.
    pub fn start_capture(&mut self, id: u32) {
        log::info!("stream::start_capture: Starting capture for id: {id}");
        let mut capturer = self.capturer.lock().unwrap();
        let sources = capturer.get_source_list();
        let mut source = sources[0].clone();
        for s in sources {
            if s.id() == (id as u64) {
                source = s;
                break;
            }
        }
        if source.id() != (id as u64) {
            log::warn!("start_capture: Source not found, capturing first source");
        }
        self.source_id = id;
        capturer.start_capture(source);
        let (tx, rx) = mpsc::channel();
        let capturer_clone = self.capturer.clone();
        self.capture_frame_handle = Some(std::thread::spawn(move || {
            run_capture_frame(rx, capturer_clone);
        }));
        self.tx = Some(tx);
    }

    /// Stops the capture process and terminates the worker thread.
    ///
    /// # Behavior
    /// - Sends a stop message to the capture worker thread
    /// - Waits for the worker thread to terminate gracefully
    /// - Cleans up thread handles and communication channels
    /// - Safe to call multiple times (no-op if already stopped)
    ///
    /// # Notes
    /// This method blocks until the capture thread has fully terminated.
    /// After calling this method, `start_capture()` can be called again to resume.
    pub fn stop_capture(&mut self) {
        if self.tx.is_none() {
            log::warn!("stop_capture: Stream is not running");
            return;
        }
        let _ = self
            .tx
            .as_mut()
            .unwrap()
            .send(StreamRuntimeMessage::StopCapture);
        self.tx.take();
        let handle = self.capture_frame_handle.take();
        if let Some(handle) = handle {
            let res = handle.join();
            if let Err(e) = res {
                log::error!("stop_capture: error joining thread: {e:?}");
            }
        }
    }

    /// Creates a new stream instance that shares buffers with the current stream.
    ///
    /// # Returns
    /// - `Ok(Stream)`: Successfully created a new stream instance
    /// - `Err(())`: Failed to create the underlying desktop capturer
    ///
    /// # Behavior
    /// - Stops the current stream if it's running
    /// - Creates a new desktop capturer with the same configuration
    /// - Shares the same buffers (stream_buffer, capture_buffer, frame) for memory efficiency
    /// - Preserves the source_id and failure count from the original stream
    /// - Sets up the same error reporting channel
    ///
    /// # Use Cases
    /// This method is primarily used for stream recovery when the capture system
    /// encounters permanent errors and needs to be restarted while maintaining
    /// the same buffer references and configuration.
    pub fn copy(mut self) -> Result<Self, ()> {
        if self.capture_frame_handle.is_some() {
            log::warn!("Stream::copy: Stream is running, stopping it");
            self.stop_capture();
        }

        let callback = create_capture_callback(
            self.buffer_source.clone(),
            self.stream_resolution,
            self.stream_buffer.clone(),
            self.frame.clone(),
            self.permanent_error_tx.clone(),
            self.failures_count.clone(),
        );
        let capturer = DesktopCapturer::new(callback, false, self.include_cursor);
        if capturer.is_none() {
            log::error!("Stream::copy: Failed to create DesktopCapturer");
            return Err(());
        }
        let capturer = capturer.unwrap();
        let apps_to_exclude = get_excluded_application_pids();
        capturer.set_excluded_applications(apps_to_exclude);

        let new_stream = Stream {
            capturer: Arc::new(Mutex::new(capturer)),
            capture_frame_handle: None,
            tx: None,
            permanent_error_tx: self.permanent_error_tx.clone(),
            stream_buffer: self.stream_buffer.clone(),
            buffer_source: self.buffer_source.clone(),
            frame: self.frame.clone(),
            stream_resolution: self.stream_resolution,
            source_id: self.source_id,
            failures_count: self.failures_count.clone(),
            include_cursor: self.include_cursor,
        };

        Ok(new_stream)
    }

    /// Returns the current count of consecutive capture failures.
    ///
    /// # Returns
    /// The number of consecutive failures that have occurred since the last
    /// successful frame capture. This counter is reset to 0 on each successful capture.
    ///
    /// # Use Cases
    /// Used for health monitoring and determining when the stream should be
    /// restarted or when the process should exit due to persistent failures.
    /// When this count reaches MAX_STREAM_FAILURES_BEFORE_EXIT, the application
    /// will terminate to trigger a restart.
    pub fn get_failures_count(&self) -> u64 {
        *self.failures_count.lock().unwrap()
    }

    /// Returns the identifier of the capture source.
    ///
    /// # Returns
    /// The ID of the display or window that this stream is currently configured
    /// to capture from. This corresponds to the ID passed to `start_capture()`.
    ///
    /// # Use Cases
    /// Used for identifying which source a stream is associated with, particularly
    /// useful when managing multiple streams or when restarting streams to ensure
    /// they reconnect to the same source.
    pub fn source_id(&self) -> u32 {
        self.source_id
    }

    pub fn get_stream_extent(&self) -> Extent {
        let stream_buffer = self.stream_buffer.lock().unwrap();
        Extent {
            width: stream_buffer.video_frame.buffer.width() as f64,
            height: stream_buffer.video_frame.buffer.height() as f64,
        }
    }

    pub fn set_buffer_source(&mut self, buffer_source: NativeVideoSource) {
        let mut b_source = self.buffer_source.lock().unwrap();
        *b_source = Some(buffer_source);
    }

    #[cfg(target_os = "linux")]
    pub fn capturer(&self) -> Arc<Mutex<DesktopCapturer>> {
        self.capturer.clone()
    }
}
