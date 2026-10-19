//! Windows virtual cameras exposed through the bundled WebRTC DirectShow backend.

use livekit::webrtc::{
    prelude::VideoBuffer,
    video_frame::{I420Buffer, VideoFrame, VideoRotation},
    video_source::native::NativeVideoSource,
};
use std::ffi::{c_char, c_void, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
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
    fn hopp_windows_camera_list(
        callback: unsafe extern "C" fn(*const c_char, *const c_char, *mut c_void),
        userdata: *mut c_void,
        error: *mut c_char,
        error_len: usize,
    ) -> i32;
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
    fn hopp_windows_camera_stop(camera: *mut c_void);
}

unsafe extern "C" fn collect_device(name: *const c_char, id: *const c_char, userdata: *mut c_void) {
    // The native enumerator lends valid strings and exclusively borrows the Vec.
    let devices = &mut *userdata.cast::<Vec<socket_lib::CameraDevice>>();
    devices.push(socket_lib::CameraDevice {
        name: CStr::from_ptr(name).to_string_lossy().into_owned(),
        id: format!("{DEVICE_PREFIX}{}", CStr::from_ptr(id).to_string_lossy()),
        default: false,
    });
}

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

struct CallbackContext {
    device_name: String,
    generation: u64,
    state: Mutex<FrameState>,
    startup: Mutex<Option<mpsc::Sender<Result<(), String>>>>,
    failed: AtomicBool,
    failures: Arc<AtomicU32>,
    error_tx: mpsc::Sender<CameraStreamMessage>,
    buffers: Arc<VideoBufferManager>,
    source: NativeVideoSource,
    config: Arc<CameraStreamConfig>,
}

impl CallbackContext {
    fn fail(&self, reason: String) {
        if !self.failed.swap(true, Ordering::AcqRel) {
            self.failures.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut startup) = self.startup.lock() {
                if let Some(startup) = startup.take() {
                    // Let the constructor's caller try another backend before
                    // reporting a failed camera to the UI.
                    let _ = startup.send(Err(reason));
                    return;
                }
            }
            let _ = self.error_tx.send(CameraStreamMessage::Failed {
                reason,
                generation: self.generation,
            });
        }
    }

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

    unsafe fn publish(
        &self,
        width: i32,
        height: i32,
        planes: [*const u8; 3],
        strides: [i32; 3],
        timestamp_us: i64,
    ) -> Result<(), String> {
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
        if let Some(startup) = self
            .startup
            .lock()
            .map_err(|_| "Camera startup state poisoned")?
            .take()
        {
            let _ = startup.send(Ok(()));
        }
        Ok(())
    }
}

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
    stop_tx: Option<mpsc::Sender<()>>,
    worker: Option<JoinHandle<()>>,
    device_name: String,
    device_id: String,
    failures: Arc<AtomicU32>,
    error_tx: mpsc::Sender<CameraStreamMessage>,
    buffers: Arc<VideoBufferManager>,
    source: NativeVideoSource,
    config: Arc<CameraStreamConfig>,
}

impl DirectShowStream {
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
            startup: Mutex::new(Some(startup_tx)),
            failed: AtomicBool::new(false),
            failures: failures.clone(),
            error_tx: error_tx.clone(),
            buffers: video_buffer_manager.clone(),
            source: buffer_source.clone(),
            config: config.clone(),
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
                return;
            }
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
            context.buffers.set_inactive(true);
        });
        let mut stream = Self {
            stop_tx: Some(stop_tx),
            worker: Some(worker),
            device_name: device_name.to_string(),
            device_id: device_id.to_string(),
            failures,
            error_tx,
            buffers: video_buffer_manager,
            source: buffer_source,
            config,
        };
        match startup_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(())) => Ok(stream),
            result => {
                stream.stop_capture();
                Err(match result {
                    Ok(Err(reason)) => reason,
                    Err(_) => "Timed out waiting for the first camera frame".to_string(),
                    Ok(Ok(())) => unreachable!(),
                })
            }
        }
    }

    pub fn stop_capture(&mut self) {
        if let Some(stop_tx) = self.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                log::error!("DirectShow capture worker panicked");
            }
        }
    }

    pub fn get_failures_count(&self) -> u32 {
        self.failures.load(Ordering::Relaxed)
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

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
    fn drop(&mut self) {
        self.stop_capture();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padded_plane_copy_preserves_destination_padding() {
        let source = [1, 2, 99, 99, 3, 4];
        let mut destination = [0; 7];
        copy_plane(&source, 4, &mut destination, 5, 2, 2).unwrap();
        assert_eq!(destination, [1, 2, 0, 0, 0, 3, 4]);
    }

    #[test]
    fn truncated_plane_is_rejected_before_copying() {
        let mut destination = [0; 4];
        assert!(copy_plane(&[1, 2, 3], 2, &mut destination, 2, 2, 2).is_err());
        assert_eq!(destination, [0; 4]);
        assert!(plane_length(1, 2, 2).is_err());
        assert!(plane_length(usize::MAX, 1, 2).is_err());
    }

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
