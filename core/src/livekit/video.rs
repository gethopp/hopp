use livekit::track::RemoteVideoTrack;
use livekit::webrtc::video_frame::I420Buffer;
use livekit::webrtc::video_stream::native::NativeVideoStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

/// A buffer holding YUV420 planar video data
#[derive(Debug)]
pub struct VideoBuffer {
    pub width: u32,
    pub height: u32,
    pub stride_y: u32,
    pub stride_u: u32,
    pub stride_v: u32,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
}

impl Default for VideoBuffer {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            stride_y: 0,
            stride_u: 0,
            stride_v: 0,
            y: Vec::new(),
            u: Vec::new(),
            v: Vec::new(),
        }
    }
}

impl VideoBuffer {
    /// Copy I420 frame data into this buffer. Reuses existing allocations when dimensions match.
    pub fn copy_from_i420(&mut self, i420: &I420Buffer, width: u32, height: u32) {
        self.width = width;
        self.height = height;

        let (sy, su, sv) = i420.strides();
        self.stride_y = sy as u32;
        self.stride_u = su as u32;
        self.stride_v = sv as u32;

        let (dy, du, dv) = i420.data();

        let ch = (height + 1) / 2;
        let y_size = (sy as u32 * height) as usize;
        let u_size = (su as u32 * ch) as usize;
        let v_size = (sv as u32 * ch) as usize;

        // Resize only when necessary to avoid reallocations
        if self.y.len() != y_size {
            self.y.resize(y_size, 0);
        }
        if self.u.len() != u_size {
            self.u.resize(u_size, 0);
        }
        if self.v.len() != v_size {
            self.v.resize(v_size, 0);
        }

        self.y.copy_from_slice(dy);
        self.u.copy_from_slice(du);
        self.v.copy_from_slice(dv);
    }
}

/// Double-buffered video frame manager.
///
/// Writer writes to `buffers[write_index]`, then swaps `write_index`.
/// Reader reads from `buffers[1 - write_index]` (the last completed frame).
/// No contention: writer and reader always access different slots.
///
#[derive(Debug)]
pub struct VideoBufferManager {
    buffers: [Mutex<VideoBuffer>; 2],
    write_index: AtomicUsize,
    inactive: std::sync::atomic::AtomicBool,
}

impl VideoBufferManager {
    pub fn new() -> Self {
        Self {
            buffers: [
                Mutex::new(VideoBuffer::default()),
                Mutex::new(VideoBuffer::default()),
            ],
            write_index: AtomicUsize::new(0),
            inactive: std::sync::atomic::AtomicBool::new(true),
        }
    }

    /// Returns the buffer to write into (the current write slot).
    pub fn write_buffer(&self) -> &Mutex<VideoBuffer> {
        self.inactive.store(false, Ordering::Release);
        let idx = self.write_index.load(Ordering::Acquire);
        &self.buffers[idx]
    }

    /// Swaps the write index so the just-written buffer becomes the read buffer.
    pub fn advance_write(&self) {
        let current = self.write_index.load(Ordering::Acquire);
        self.write_index.store(1 - current, Ordering::Release);
    }

    /// Returns the latest completed frame (the slot the writer is NOT writing to).
    pub fn latest_frame(&self) -> &Mutex<VideoBuffer> {
        let write_idx = self.write_index.load(Ordering::Acquire);
        &self.buffers[1 - write_idx]
    }

    /// Returns true if the stream is inactive (no frames received recently).
    pub fn is_inactive(&self) -> bool {
        self.inactive.load(Ordering::Acquire)
    }

    /// Mark the stream as inactive.
    pub fn set_inactive(&self, inactive: bool) {
        self.inactive.store(inactive, Ordering::Release);
    }
}

/// Process camera stream frames with a 5-second timeout.
///
/// Receives frames from the video track and writes them to the buffer manager.
/// If no frames are received for 5 seconds, marks the manager as inactive.
/// The stream continues running until:
/// - The stream ends naturally
/// - A stop signal is received via stop_rx
pub async fn process_camera_stream(
    video_track: RemoteVideoTrack,
    manager: Arc<VideoBufferManager>,
    mut stop_rx: mpsc::UnboundedReceiver<()>,
    stream_key: String,
) {
    log::info!(
        "process_camera_stream: Starting camera stream processing for participant: {}",
        stream_key
    );

    let mut sink = NativeVideoStream::new(video_track.rtc_track());
    let mut frames = 0u64;
    let mut fps_frames = 0u32;
    let mut fps_last = std::time::Instant::now();
    let timeout_duration = std::time::Duration::from_secs(1);

    loop {
        tokio::select! {
            result = tokio::time::timeout(timeout_duration, sink.next()) => {
                match result {
                    Ok(Some(frame)) => {
                        let i420 = frame.buffer.to_i420();
                        let width = frame.buffer.width();
                        let height = frame.buffer.height();

                        let buf = manager.write_buffer();
                        {
                            let mut guard = buf.lock().unwrap();
                            guard.copy_from_i420(&i420, width, height);
                        }
                        manager.advance_write();

                        frames += 1;
                        fps_frames += 1;
                        let elapsed = fps_last.elapsed();
                        if elapsed >= std::time::Duration::from_secs(5) {
                            let fps = fps_frames as f32 / elapsed.as_secs_f32();
                            log::info!(
                                "process_camera_stream: {} fps={:.1} total_frames={} ({}x{})",
                                stream_key,
                                fps,
                                frames,
                                width,
                                height
                            );
                            fps_frames = 0;
                            fps_last = std::time::Instant::now();
                        }
                    }
                    Ok(None) => {
                        log::info!(
                            "process_camera_stream: Stream ended for participant: {}",
                            stream_key
                        );
                        break;
                    }
                    Err(_) => {
                        log::warn!(
                            "process_camera_stream: No frames received for 5 seconds from {}, marking as inactive",
                            stream_key
                        );
                        manager.set_inactive(true);
                        // Continue waiting for frames instead of breaking
                    }
                }
            }
            _ = stop_rx.recv() => {
                log::info!(
                    "process_camera_stream: Received stop signal for camera stream: {}",
                    stream_key
                );
                break;
            }
        }
    }

    manager.set_inactive(true);
    log::info!(
        "process_camera_stream: Camera stream ended for participant: {}",
        stream_key
    );
}
