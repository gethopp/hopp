//! Windows virtual cameras exposed through the bundled WebRTC DirectShow backend.

use livekit::webrtc::{
    prelude::VideoBuffer,
    video_frame::{I420Buffer, VideoFrame, VideoRotation},
    video_source::native::NativeVideoSource,
};
use std::ffi::{c_char, c_void, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::camera::stream::{CameraStreamConfig, CameraStreamMessage};
use crate::livekit::video::VideoBufferManager;
use crate::utils::geometry::aspect_fit;

const DEVICE_PREFIX: &str = "directshow:";
const FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const START_TIMEOUT: Duration = Duration::from_secs(10);
const RATE_REPORT_INTERVAL: Duration = Duration::from_secs(5);

type FrameCallback = unsafe extern "C" fn(
    *mut c_void,
    i32,
    i32,
    *const u8,
    i32,
    *const u8,
    i32,
    *const u8,
    i32,
    i64,
);

extern "C" {
    /// Enumerates borrowed UTF-8 device strings through a synchronous native callback.
    fn hopp_windows_camera_list(
        callback: unsafe extern "C" fn(*const c_char, *const c_char, *mut c_void),
        userdata: *mut c_void,
        error: *mut c_char,
        error_len: usize,
    ) -> i32;
    /// Opens a native graph owned by the calling MTA worker until explicit stop.
    fn hopp_windows_camera_start(
        device_id: *const c_char,
        width: i32,
        height: i32,
        fps: i32,
        callback: FrameCallback,
        userdata: *mut c_void,
        error: *mut c_char,
        error_len: usize,
    ) -> *mut c_void;
    /// Stops the owned native graph and drains callbacks before releasing its handle.
    fn hopp_windows_camera_stop(camera: *mut c_void);
}

/// Copies the native enumerator's borrowed device strings into the Rust list.
unsafe extern "C" fn collect_device(name: *const c_char, id: *const c_char, userdata: *mut c_void) {
    // The native enumerator lends valid strings and exclusively borrows the Vec.
    let devices = &mut *userdata.cast::<Vec<socket_lib::CameraDevice>>();
    devices.push(socket_lib::CameraDevice {
        name: CStr::from_ptr(name).to_string_lossy().into_owned(),
        id: format!("{DEVICE_PREFIX}{}", CStr::from_ptr(id).to_string_lossy()),
        default: false,
    });
}

/// Enumerates DirectShow devices on an MTA thread, independently of winit's STA.
pub fn list_devices() -> Result<Vec<socket_lib::CameraDevice>, String> {
    // winit's UI thread may already be in an STA; DirectShow uses its own MTA.
    std::thread::spawn(|| {
        let mut devices = Vec::<socket_lib::CameraDevice>::new();
        let mut error = [0 as c_char; 512];
        let result = unsafe {
            hopp_windows_camera_list(
                collect_device,
                (&mut devices as *mut Vec<socket_lib::CameraDevice>).cast(),
                error.as_mut_ptr(),
                error.len(),
            )
        };
        if result < 0 {
            return Err(native_error(&error));
        }
        devices.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
        if let Some(first) = devices.first_mut() {
            first.default = true;
        }
        Ok(devices)
    })
    .join()
    .map_err(|_| "DirectShow enumeration thread panicked".to_string())?
}

/// Converts the shim's null-terminated error buffer into an owned message.
fn native_error(error: &[c_char]) -> String {
    // The native shim always null-terminates this non-empty buffer.
    unsafe { CStr::from_ptr(error.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

struct FrameState {
    last_frame: Instant,
    pacer: FramePacer,
    telemetry: FrameTelemetry,
    frame_id: u64,
    buffer: Option<I420Buffer>,
}

#[derive(Default)]
struct FrameTelemetry {
    started: Option<Instant>,
    received: u64,
    published: u64,
}

impl FrameTelemetry {
    /// Reports received and published rates in fixed windows after first delivery.
    fn record(&mut self, now: Instant, published: bool) -> Option<(f64, f64)> {
        let Some(started) = self.started else {
            // Begin after the first successful frame, excluding native startup.
            if published {
                self.started = Some(now);
            }
            return None;
        };
        self.received += 1;
        self.published += u64::from(published);
        let elapsed = now.duration_since(started);
        if elapsed < RATE_REPORT_INTERVAL {
            return None;
        }
        let rates = (
            self.received as f64 / elapsed.as_secs_f64(),
            self.published as f64 / elapsed.as_secs_f64(),
        );
        self.started = Some(now);
        self.received = 0;
        self.published = 0;
        Some(rates)
    }
}

#[derive(Default)]
struct FramePacer {
    next_frame: Option<Instant>,
    fps: u32,
}

impl FramePacer {
    /// Preserves cadence across callback jitter while resetting for rate changes.
    fn should_publish(&mut self, now: Instant, fps: u32) -> bool {
        let fps = fps.max(1);
        if self.fps != fps {
            self.fps = fps;
            self.next_frame = None;
        }
        let interval = Duration::from_micros(1_000_000 / fps as u64);
        let deadline = self.next_frame.unwrap_or(now);
        // Camera callbacks jitter around the negotiated cadence. Keep the
        // deadline's phase rather than restarting a full interval each frame.
        let tolerance = (interval / 10).min(Duration::from_millis(2));
        if now + tolerance < deadline {
            return false;
        }
        let next = deadline + interval;
        self.next_frame = Some(if next <= now { now + interval } else { next });
        true
    }
}

/// Revokes an individual native graph, including graphs sharing a capture generation.
#[derive(Default)]
struct CallbackLifetime {
    cancelled: AtomicBool,
    gate: Mutex<()>,
}

impl CallbackLifetime {
    /// Serializes side effects with cancellation and rejects retired callbacks.
    fn enter(&self) -> Option<MutexGuard<'_, ()>> {
        let guard = self.gate.lock().ok()?;
        (!self.cancelled.load(Ordering::Acquire)).then_some(guard)
    }

    /// Rejects future callbacks and waits for any callback already publishing.
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        // Once this lock is drained, fallback can safely reuse the shared buffers.
        drop(self.gate.lock());
    }
}

/// Owns cancellation separately from a native worker that may block during startup.
struct CaptureWorker {
    stop_tx: Option<mpsc::Sender<()>>,
    worker: Option<JoinHandle<()>>,
    lifetime: Arc<CallbackLifetime>,
}

impl CaptureWorker {
    /// Bounds startup even when native graph creation never returns.
    fn wait_for_startup(
        &mut self,
        startup_rx: mpsc::Receiver<Result<(), String>>,
        timeout: Duration,
    ) -> Result<(), String> {
        match startup_rx.recv_timeout(timeout) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(reason)) => {
                self.stop(true);
                Err(reason)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                self.stop(true);
                Err("DirectShow capture worker exited before its first frame".to_string())
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // A native start call cannot safely be interrupted or joined.
                // Its closure retains the callback context until native stop
                // drains callbacks if the driver eventually returns.
                self.stop(false);
                Err("Timed out waiting for the first camera frame".to_string())
            }
        }
    }

    /// Revokes callbacks, requests native stop, and optionally waits for cleanup.
    fn stop(&mut self, join: bool) {
        self.lifetime.cancel();
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        if let Some(worker) = self.worker.take() {
            if join && worker.join().is_err() {
                log::error!("DirectShow capture worker panicked");
            }
        }
    }
}

/// Defers callback outcomes until native graph creation has actually returned.
#[derive(Default)]
struct StartupHandshake {
    sender: Option<mpsc::Sender<Result<(), String>>>,
    native_returned: bool,
    outcome: Option<Result<(), String>>,
}

impl StartupHandshake {
    /// Creates a pending startup acknowledgement for a newly spawned graph.
    fn new(sender: mpsc::Sender<Result<(), String>>) -> Self {
        Self {
            sender: Some(sender),
            ..Self::default()
        }
    }

    /// Records first delivery or failure without acknowledging a still-blocked start.
    /// Returns whether the outcome belongs to startup rather than a running stream.
    fn record(&mut self, outcome: Result<(), String>) -> bool {
        if self.sender.is_none() {
            return false;
        }
        // A failure after an early frame must supersede its pending success.
        if self.outcome.is_none() || outcome.is_err() {
            self.outcome = Some(outcome);
        }
        self.deliver();
        true
    }

    /// Permits acknowledgement only once the native start call can no longer wedge.
    fn native_start_returned(&mut self) {
        self.native_returned = true;
        self.deliver();
    }

    /// Sends the startup result once both native completion and an outcome exist.
    fn deliver(&mut self) {
        if self.native_returned {
            if let Some(outcome) = self.outcome.take() {
                if let Some(sender) = self.sender.take() {
                    let _ = sender.send(outcome);
                }
            }
        }
    }
}

struct CallbackContext {
    device_name: String,
    generation: u64,
    state: Mutex<FrameState>,
    startup: Mutex<StartupHandshake>,
    failed: AtomicBool,
    failures: Arc<AtomicU32>,
    error_tx: mpsc::Sender<CameraStreamMessage>,
    buffers: Arc<VideoBufferManager>,
    source: NativeVideoSource,
    config: Arc<CameraStreamConfig>,
    lifetime: Arc<CallbackLifetime>,
}

impl CallbackContext {
    /// Claims a live graph's side effects; fallback can share the same generation.
    fn enter(&self) -> Option<MutexGuard<'_, ()>> {
        let guard = self.lifetime.enter()?;
        (self.config.capture_generation() == self.generation).then_some(guard)
    }

    /// Reports a graph failure once, unless the graph has already been retired.
    fn fail(&self, reason: String) {
        let Some(_guard) = self.enter() else {
            return;
        };
        if !self.failed.swap(true, Ordering::AcqRel) {
            self.failures.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut startup) = self.startup.lock() {
                if startup.record(Err(reason.clone())) {
                    // Let the constructor's caller try another backend before
                    // reporting a failed camera to the UI.
                    return;
                }
            }
            let _ = self.error_tx.send(CameraStreamMessage::Failed {
                reason,
                generation: self.generation,
            });
        }
    }

    /// Releases pending startup outcomes after the native start call has returned.
    fn native_start_returned(&self) {
        let Some(_guard) = self.enter() else {
            return;
        };
        if let Ok(mut startup) = self.startup.lock() {
            startup.native_start_returned();
        }
    }

    /// Logs native delivery and outgoing publication cadence independently.
    fn record_frame_rate(&self, telemetry: &mut FrameTelemetry, now: Instant, published: bool) {
        if let Some((received_fps, published_fps)) = telemetry.record(now, published) {
            log::info!(
                "DirectShow camera '{}': received {:.1}fps, published {:.1}fps (target {}fps)",
                self.device_name,
                received_fps,
                published_fps,
                self.config.target_fps()
            );
        }
    }

    /// Copies borrowed I420 planes while excluding cancellation and stale graphs.
    ///
    /// Plane pointers must remain readable for their validated extents until return.
    unsafe fn publish(
        &self,
        width: i32,
        height: i32,
        planes: [*const u8; 3],
        strides: [i32; 3],
        timestamp_us: i64,
    ) -> Result<(), String> {
        let Some(_guard) = self.enter() else {
            return Ok(());
        };
        if self.failed.load(Ordering::Acquire) {
            return Ok(());
        }
        if !(1..=16384).contains(&width) || !(1..=16384).contains(&height) {
            return Err("DirectShow returned invalid frame dimensions".to_string());
        }
        let width = width as u32;
        let height = height as u32;
        let now = Instant::now();
        let mut state = self
            .state
            .lock()
            .map_err(|_| "Camera frame state poisoned")?;
        state.last_frame = now;
        if !state.pacer.should_publish(now, self.config.target_fps()) {
            self.record_frame_rate(&mut state.telemetry, now, false);
            return Ok(());
        }
        let buffer = state
            .buffer
            .get_or_insert_with(|| I420Buffer::new(width, height));
        if buffer.width() != width || buffer.height() != height {
            *buffer = I420Buffer::new(width, height);
        }
        let (stride_y, stride_u, stride_v) = buffer.strides();
        let (y, u, v) = buffer.data_mut();
        for (index, (destination, destination_stride, columns, rows)) in [
            (y, stride_y, width, height),
            (u, stride_u, width.div_ceil(2), height.div_ceil(2)),
            (v, stride_v, width.div_ceil(2), height.div_ceil(2)),
        ]
        .into_iter()
        .enumerate()
        {
            let stride =
                usize::try_from(strides[index]).map_err(|_| "Invalid camera plane stride")?;
            if planes[index].is_null() || stride < columns as usize {
                return Err("DirectShow returned an invalid I420 plane".to_string());
            }
            let length = plane_length(stride, columns as usize, rows as usize)?;
            // WebRTC owns the plane until the native callback returns. Only the
            // actual pixels of the final row are required, not trailing padding.
            let source = std::slice::from_raw_parts(planes[index], length);
            copy_plane(
                source,
                stride,
                destination,
                destination_stride as usize,
                columns as usize,
                rows as usize,
            )?;
        }
        let (target_width, target_height) = aspect_fit(
            width,
            height,
            self.config.target_width(),
            self.config.target_height(),
        );
        let scaled;
        let output = if target_width != width || target_height != height {
            scaled = buffer.scale(target_width as i32, target_height as i32);
            &scaled
        } else {
            &*buffer
        };
        let mut write_buffer = self
            .buffers
            .write_buffer()
            .lock()
            .map_err(|_| "Camera video buffer poisoned")?;
        write_buffer.copy_from_i420(output, target_width, target_height);
        drop(write_buffer);
        self.buffers.advance_write();
        self.source.capture_frame(&VideoFrame {
            rotation: VideoRotation::VideoRotation0,
            buffer: output,
            timestamp_us,
            frame_metadata: None,
        });
        self.buffers.set_frame_id(state.frame_id);
        if state.frame_id == 0 {
            log::info!(
                "DirectShow camera '{}': first frame {}x{}, published {}x{}",
                self.device_name,
                width,
                height,
                target_width,
                target_height
            );
        }
        state.frame_id = state.frame_id.wrapping_add(1);
        self.record_frame_rate(&mut state.telemetry, now, true);
        self.startup
            .lock()
            .map_err(|_| "Camera startup state poisoned")?
            .record(Ok(()));
        Ok(())
    }

    /// Marks buffers inactive only while this graph still owns their publication.
    fn deactivate(&self) {
        if let Some(_guard) = self.enter() {
            self.buffers.set_inactive(true);
        }
    }
}

/// Contains all unwinding and errors at the native callback boundary.
unsafe extern "C" fn receive_frame(
    userdata: *mut c_void,
    width: i32,
    height: i32,
    y: *const u8,
    stride_y: i32,
    u: *const u8,
    stride_u: i32,
    v: *const u8,
    stride_v: i32,
    timestamp_us: i64,
) {
    let context = &*userdata.cast::<CallbackContext>();
    match catch_unwind(AssertUnwindSafe(|| {
        context.publish(
            width,
            height,
            [y, u, v],
            [stride_y, stride_u, stride_v],
            timestamp_us,
        )
    })) {
        Ok(Ok(())) => {}
        Ok(Err(reason)) => context.fail(reason),
        Err(_) => context.fail("DirectShow frame callback panicked".to_string()),
    }
}

/// Validates an I420 plane extent without requiring final-row padding.
fn plane_length(stride: usize, columns: usize, rows: usize) -> Result<usize, String> {
    if rows == 0 || columns == 0 || stride < columns {
        return Err("Invalid I420 plane shape".to_string());
    }
    (rows - 1)
        .checked_mul(stride)
        .and_then(|size| size.checked_add(columns))
        .filter(|size| *size <= 512 * 1024 * 1024)
        .ok_or_else(|| "I420 plane size overflow".to_string())
}

/// Copies image columns while preserving padding in the destination plane.
fn copy_plane(
    source: &[u8],
    source_stride: usize,
    destination: &mut [u8],
    destination_stride: usize,
    columns: usize,
    rows: usize,
) -> Result<(), String> {
    if source.len() < plane_length(source_stride, columns, rows)?
        || destination.len() < plane_length(destination_stride, columns, rows)?
    {
        return Err("I420 plane is shorter than its stride and dimensions".to_string());
    }
    for row in 0..rows {
        let source_start = row * source_stride;
        let destination_start = row * destination_stride;
        destination[destination_start..destination_start + columns]
            .copy_from_slice(&source[source_start..source_start + columns]);
    }
    Ok(())
}

pub struct DirectShowStream {
    worker: CaptureWorker,
    device_name: String,
    device_id: String,
    failures: Arc<AtomicU32>,
    error_tx: mpsc::Sender<CameraStreamMessage>,
    buffers: Arc<VideoBufferManager>,
    source: NativeVideoSource,
    config: Arc<CameraStreamConfig>,
}

impl DirectShowStream {
    /// Starts a native camera and requires a first frame within a bounded interval.
    pub fn new(
        device_name: &str,
        device_id: &str,
        error_tx: mpsc::Sender<CameraStreamMessage>,
        video_buffer_manager: Arc<VideoBufferManager>,
        buffer_source: NativeVideoSource,
        config: Arc<CameraStreamConfig>,
    ) -> Result<Self, String> {
        let native_id = CString::new(device_id.strip_prefix(DEVICE_PREFIX).unwrap_or(device_id))
            .map_err(|_| "Camera ID contains a null byte")?;
        let (stop_tx, stop_rx) = mpsc::channel();
        let (startup_tx, startup_rx) = mpsc::channel();
        let failures = Arc::new(AtomicU32::new(0));
        let lifetime = Arc::new(CallbackLifetime::default());
        let context = Box::new(CallbackContext {
            device_name: device_name.to_string(),
            generation: config.capture_generation(),
            state: Mutex::new(FrameState {
                last_frame: Instant::now(),
                pacer: FramePacer::default(),
                telemetry: FrameTelemetry::default(),
                frame_id: 0,
                buffer: None,
            }),
            startup: Mutex::new(StartupHandshake::new(startup_tx)),
            failed: AtomicBool::new(false),
            failures: failures.clone(),
            error_tx: error_tx.clone(),
            buffers: video_buffer_manager.clone(),
            source: buffer_source.clone(),
            config: config.clone(),
            lifetime: lifetime.clone(),
        });
        let worker = std::thread::spawn(move || {
            let mut error = [0 as c_char; 512];
            let input_extent = context.config.input_resolution().extent();
            let camera = unsafe {
                hopp_windows_camera_start(
                    native_id.as_ptr(),
                    input_extent.width as i32,
                    input_extent.height as i32,
                    30,
                    receive_frame,
                    (&*context as *const CallbackContext).cast_mut().cast(),
                    error.as_mut_ptr(),
                    error.len(),
                )
            };
            if camera.is_null() {
                context.fail(native_error(&error));
                context.native_start_returned();
                return;
            }
            context.native_start_returned();
            // Native graph creation can take time. Start the no-frame deadline
            // once it has returned, unless delivery has already begun.
            if let Ok(mut state) = context.state.lock() {
                if state.frame_id == 0 {
                    state.last_frame = Instant::now();
                }
            }
            loop {
                match stop_rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if context.failed.load(Ordering::Acquire) {
                    break;
                }
                let stalled = context
                    .state
                    .lock()
                    .map(|state| state.last_frame.elapsed() >= FRAME_TIMEOUT)
                    .unwrap_or(true);
                if stalled {
                    context.fail("Camera stopped delivering video frames".to_string());
                    break;
                }
            }
            // Native stop deregisters and drains callbacks before freeing the
            // handle. The callback context must remain alive until it returns.
            unsafe { hopp_windows_camera_stop(camera) };
            context.deactivate();
        });
        let mut worker = CaptureWorker {
            stop_tx: Some(stop_tx),
            worker: Some(worker),
            lifetime,
        };
        worker.wait_for_startup(startup_rx, START_TIMEOUT)?;
        Ok(Self {
            worker,
            device_name: device_name.to_string(),
            device_id: device_id.to_string(),
            failures,
            error_tx,
            buffers: video_buffer_manager,
            source: buffer_source,
            config,
        })
    }

    /// Stops delivery before joining the owning native COM thread.
    pub fn stop_capture(&mut self) {
        let was_running = self.worker.worker.is_some();
        self.worker.stop(true);
        if was_running {
            self.buffers.set_inactive(true);
        }
    }

    /// Returns the number of distinct failures reported by this graph.
    pub fn get_failures_count(&self) -> u32 {
        self.failures.load(Ordering::Relaxed)
    }

    /// Returns the user-visible name retained for camera selection.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Reopens the selected camera using the latest shared input configuration.
    pub fn copy(&self) -> Result<Self, String> {
        Self::new(
            &self.device_name,
            &self.device_id,
            self.error_tx.clone(),
            self.buffers.clone(),
            self.source.clone(),
            self.config.clone(),
        )
    }
}

impl Drop for DirectShowStream {
    /// Releases the native graph through the same ordered shutdown as explicit stop.
    fn drop(&mut self) {
        self.stop_capture();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Early frame callbacks cannot acknowledge a native start still in progress.
    #[test]
    fn startup_callbacks_wait_for_native_start_to_return() {
        for outcome in [Ok(()), Err("Early callback failure".to_string())] {
            let (startup_tx, startup_rx) = mpsc::channel();
            let mut startup = StartupHandshake::new(startup_tx);
            assert!(startup.record(outcome.clone()));
            assert!(matches!(
                startup_rx.try_recv(),
                Err(mpsc::TryRecvError::Empty)
            ));
            startup.native_start_returned();
            assert_eq!(startup_rx.recv().unwrap(), outcome);
            assert!(!startup.record(Ok(())));
        }
    }

    /// A returned native start still requires an actual frame or capture error.
    #[test]
    fn native_start_completion_waits_for_first_callback_outcome() {
        let (startup_tx, startup_rx) = mpsc::channel();
        let mut startup = StartupHandshake::new(startup_tx);
        startup.native_start_returned();
        assert!(matches!(
            startup_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert!(startup.record(Ok(())));
        assert_eq!(startup_rx.recv().unwrap(), Ok(()));
    }

    /// Failure during graph creation must supersede a previously delivered frame.
    #[test]
    fn startup_failure_supersedes_pending_success() {
        let (startup_tx, startup_rx) = mpsc::channel();
        let mut startup = StartupHandshake::new(startup_tx);
        assert!(startup.record(Ok(())));
        assert!(startup.record(Err("Native graph failed".into())));
        startup.native_start_returned();
        assert_eq!(
            startup_rx.recv().unwrap(),
            Err("Native graph failed".into())
        );
    }

    /// A wedged native start must not turn the constructor's timeout into a join.
    #[test]
    fn blocked_startup_times_out_without_joining_native_worker() {
        let (stop_tx, stop_rx) = mpsc::channel();
        let (startup_tx, startup_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let lifetime = Arc::new(CallbackLifetime::default());
        let worker_lifetime = lifetime.clone();
        let worker = std::thread::spawn(move || {
            // Model a driver blocked inside hopp_windows_camera_start.
            let mut startup = StartupHandshake::new(startup_tx);
            // Some drivers deliver a frame before their blocking start returns.
            assert!(startup.record(Ok(())));
            // A regression that accidentally joins must fail rather than leave
            // the entire test process permanently blocked.
            let _ = release_rx.recv_timeout(Duration::from_secs(2));
            assert_eq!(stop_rx.try_recv(), Ok(()));
            if let Some(_guard) = worker_lifetime.enter() {
                startup.native_start_returned();
            }
            finished_tx.send(()).unwrap();
        });
        let mut capture = CaptureWorker {
            stop_tx: Some(stop_tx),
            worker: Some(worker),
            lifetime: lifetime.clone(),
        };
        let started = Instant::now();
        assert!(capture
            .wait_for_startup(startup_rx, Duration::from_millis(20))
            .unwrap_err()
            .contains("Timed out"));
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(capture.worker.is_none());
        assert!(lifetime.enter().is_none());
        release_tx.send(()).unwrap();
        finished_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    }

    /// Explicit startup failures retain ordered cleanup before trying fallback.
    #[test]
    fn explicit_startup_failure_joins_worker_cleanup() {
        let (stop_tx, stop_rx) = mpsc::channel();
        let (startup_tx, startup_rx) = mpsc::channel();
        let cleaned_up = Arc::new(AtomicBool::new(false));
        let worker_cleaned_up = cleaned_up.clone();
        let worker = std::thread::spawn(move || {
            startup_tx
                .send(Err("Driver rejected format".into()))
                .unwrap();
            stop_rx.recv().unwrap();
            worker_cleaned_up.store(true, Ordering::Release);
        });
        let mut capture = CaptureWorker {
            stop_tx: Some(stop_tx),
            worker: Some(worker),
            lifetime: Arc::new(CallbackLifetime::default()),
        };
        assert_eq!(
            capture.wait_for_startup(startup_rx, Duration::from_secs(1)),
            Err("Driver rejected format".into())
        );
        assert!(cleaned_up.load(Ordering::Acquire));
    }

    /// A detached graph cannot publish, report failure, or deactivate fallback.
    #[tokio::test]
    async fn retired_context_rejects_delayed_callbacks_and_cleanup_in_same_generation() {
        use livekit::webrtc::video_source::VideoResolution;

        let config = Arc::new(CameraStreamConfig::new_high_quality());
        let buffers = Arc::new(VideoBufferManager::default());
        let (error_tx, error_rx) = mpsc::channel();
        let lifetime = Arc::new(CallbackLifetime::default());
        let context = CallbackContext {
            device_name: "Retired camera".into(),
            generation: config.capture_generation(),
            state: Mutex::new(FrameState {
                last_frame: Instant::now(),
                pacer: FramePacer::default(),
                telemetry: FrameTelemetry::default(),
                frame_id: 0,
                buffer: None,
            }),
            startup: Mutex::new(StartupHandshake::default()),
            failed: AtomicBool::new(false),
            failures: Arc::new(AtomicU32::new(0)),
            error_tx,
            buffers: buffers.clone(),
            source: NativeVideoSource::new(
                VideoResolution {
                    width: 2,
                    height: 2,
                },
                false,
            ),
            config,
            lifetime: lifetime.clone(),
        };
        lifetime.cancel();
        // Fallback deliberately shares the generation, as stream_windows does.
        buffers.set_inactive(false);
        buffers.set_frame_id(123);
        let y = [16; 4];
        let chroma = [128];
        unsafe {
            context
                .publish(
                    2,
                    2,
                    [y.as_ptr(), chroma.as_ptr(), chroma.as_ptr()],
                    [2, 1, 1],
                    0,
                )
                .unwrap();
        }
        context.fail("Delayed old graph failure".into());
        context.deactivate();
        assert!(!buffers.is_inactive());
        assert_eq!(buffers.current_frame_id(), 123);
        assert_eq!(buffers.latest_frame().lock().unwrap().width, 0);
        assert_eq!(context.failures.load(Ordering::Relaxed), 0);
        assert!(matches!(
            error_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
    }

    /// Cancellation drains a callback already inside the publication gate.
    #[test]
    fn cancellation_waits_for_in_flight_callback_before_returning() {
        let lifetime = Arc::new(CallbackLifetime::default());
        let in_flight = lifetime.enter().unwrap();
        let (cancelled_tx, cancelled_rx) = mpsc::channel();
        let worker_lifetime = lifetime.clone();
        let worker = std::thread::spawn(move || {
            worker_lifetime.cancel();
            cancelled_tx.send(()).unwrap();
        });
        assert!(cancelled_rx
            .recv_timeout(Duration::from_millis(20))
            .is_err());
        drop(in_flight);
        cancelled_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        worker.join().unwrap();
        assert!(lifetime.enter().is_none());
    }

    /// Image copies preserve padding outside visible columns.
    #[test]
    fn padded_plane_copy_preserves_destination_padding() {
        let source = [1, 2, 99, 99, 3, 4];
        let mut destination = [0; 7];
        copy_plane(&source, 4, &mut destination, 5, 2, 2).unwrap();
        assert_eq!(destination, [1, 2, 0, 0, 0, 3, 4]);
    }

    /// Invalid plane extents fail before any destination mutation.
    #[test]
    fn truncated_plane_is_rejected_before_copying() {
        let mut destination = [0; 4];
        assert!(copy_plane(&[1, 2, 3], 2, &mut destination, 2, 2, 2).is_err());
        assert_eq!(destination, [0; 4]);
        assert!(plane_length(1, 2, 2).is_err());
        assert!(plane_length(usize::MAX, 1, 2).is_err());
    }

    /// Publication tracks the requested cadence despite camera callback jitter.
    #[test]
    fn callback_jitter_preserves_thirty_fps_and_decimates_to_fifteen() {
        let start = Instant::now();
        let mut full_rate = FramePacer::default();
        let mut half_rate = FramePacer::default();
        let mut published_full = 0;
        let mut published_half = 0;
        for index in 0..120_u64 {
            let jitter = [1000, 200, 1800, 500][index as usize % 4];
            let now = start + Duration::from_micros(index * 33_333 + jitter);
            published_full += usize::from(full_rate.should_publish(now, 30));
            published_half += usize::from(half_rate.should_publish(now, 15));
        }
        assert_eq!(published_full, 120);
        assert_eq!(published_half, 60);
    }

    /// Rate changes and stalls do not carry an obsolete publication deadline.
    #[test]
    fn quality_changes_and_stalls_reset_the_publication_deadline() {
        let start = Instant::now();
        let mut pacer = FramePacer::default();
        assert!(pacer.should_publish(start, 15));
        assert!(pacer.should_publish(start + Duration::from_millis(33), 30));
        assert!(pacer.should_publish(start + Duration::from_millis(66), 15));
        let after_stall = start + Duration::from_secs(2);
        assert!(pacer.should_publish(after_stall, 15));
        assert!(!pacer.should_publish(after_stall + Duration::from_millis(33), 15));
        assert!(pacer.should_publish(after_stall + Duration::from_millis(67), 15));
    }

    /// Received and published rates use independent counts in each report window.
    #[test]
    fn telemetry_counts_received_and_published_frames_in_separate_windows() {
        let start = Instant::now();
        let mut telemetry = FrameTelemetry::default();
        assert!(telemetry.record(start, false).is_none());
        let first_frame = start + Duration::from_secs(10);
        assert!(telemetry.record(first_frame, true).is_none());
        for index in 1..=100 {
            let rates = telemetry.record(
                first_frame + Duration::from_millis(index * 100),
                index % 2 == 0,
            );
            if index == 50 || index == 100 {
                assert_eq!(rates, Some((10.0, 5.0)));
            } else {
                assert!(rates.is_none());
            }
        }
    }
}
