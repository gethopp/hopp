use livekit::track::RemoteVideoTrack;
use livekit::webrtc::video_frame::I420Buffer;
use livekit::webrtc::video_stream::native::NativeVideoStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

/// Align a value up to the given alignment.
fn align_to(value: u32, alignment: u32) -> u32 {
    (value + alignment - 1) / alignment * alignment
}

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
    pub frame_id: u64,
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
            frame_id: 0,
        }
    }
}

impl VideoBuffer {
    /// Copy I420 frame data into this buffer with GPU-aligned strides (256-byte alignment).
    /// Data arrives GPU-ready, eliminating per-frame scratch buffer padding in the renderer.
    pub fn copy_from_i420(&mut self, i420: &I420Buffer, width: u32, height: u32) {
        let (src_stride_y, src_stride_u, src_stride_v) = i420.strides();
        let (dy, du, dv) = i420.data();

        // Only recompute strides, resize buffers, and fill padding when dimensions change
        let dims_changed = self.width != width || self.height != height;
        if dims_changed {
            self.width = width;
            self.height = height;

            // GPU-aligned strides (wgpu requires bytes_per_row multiple of 256)
            let y_stride = align_to(width, 256);
            let uv_stride = align_to(width / 2, 256);
            self.stride_y = y_stride;
            self.stride_u = uv_stride;
            self.stride_v = uv_stride;

            let ch = (height + 1) / 2;
            let y_size = (y_stride * height) as usize;
            let u_size = (uv_stride * ch) as usize;
            let v_size = (uv_stride * ch) as usize;

            self.y.resize(y_size, 0);
            self.u.resize(u_size, 128);
            self.v.resize(v_size, 128);
        }

        let y_stride = self.stride_y;
        let uv_stride = self.stride_u;
        let chroma_height = (height + 1) / 2;

        // Row-by-row copy — only the pixel data, padding bytes stay from resize/previous fill
        for row in 0..height as usize {
            let src_start = row * src_stride_y as usize;
            let dst_start = row * y_stride as usize;
            self.y[dst_start..dst_start + width as usize]
                .copy_from_slice(&dy[src_start..src_start + width as usize]);
        }

        let uv_w = (width / 2) as usize;
        for row in 0..chroma_height as usize {
            let src_start = row * src_stride_u as usize;
            let dst_start = row * uv_stride as usize;
            self.u[dst_start..dst_start + uv_w].copy_from_slice(&du[src_start..src_start + uv_w]);
        }

        for row in 0..chroma_height as usize {
            let src_start = row * src_stride_v as usize;
            let dst_start = row * uv_stride as usize;
            self.v[dst_start..dst_start + uv_w].copy_from_slice(&dv[src_start..src_start + uv_w]);
        }
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

/// Process a video stream (camera or screen share) frames with a 5-second timeout.
///
/// Receives frames from the video track and writes them to the buffer manager.
/// If no frames are received for 5 seconds, marks the manager as inactive.
/// The stream continues running until:
/// - The stream ends naturally
/// - A stop signal is received via stop_rx
pub async fn process_video_stream(
    video_track: RemoteVideoTrack,
    manager: Arc<VideoBufferManager>,
    mut stop_rx: mpsc::UnboundedReceiver<()>,
    stream_key: String,
    is_camera: bool,
    redraw_tx: Option<std::sync::mpsc::Sender<crate::window::screensharing_window::RedrawCommand>>,
) {
    let stream_type = if is_camera { "camera" } else { "screen share" };
    log::info!(
        "process_video_stream: Starting {} stream processing for participant: {}",
        stream_type,
        stream_key
    );

    let mut sink = NativeVideoStream::new(video_track.rtc_track());
    let timeout_duration = std::time::Duration::from_secs(1);
    let mut frame_counter: u64 = 0;

    loop {
        tokio::select! {
            result = tokio::time::timeout(timeout_duration, sink.next()) => {
                match result {
                    Ok(Some(frame)) => {
                        let mut latest_frame = frame;
                        let mut skipped: u64 = 0;

                        // Drain queued frames, keep only the newest
                        while let Ok(Some(newer)) = tokio::time::timeout(
                            std::time::Duration::ZERO,
                            sink.next(),
                        ).await {
                            latest_frame = newer;
                            skipped += 1;
                        }


                        if skipped > 0 {
                            log::warn!(
                                "process_video_stream: skipped {skipped} stale frames for {stream_key} [{stream_type}]"
                            );
                        }

                        let i420 = latest_frame.buffer.to_i420();
                        let width = latest_frame.buffer.width();
                        let height = latest_frame.buffer.height();

                        let buf = manager.write_buffer();
                        {
                            let mut guard = buf.lock().unwrap();

                            guard.copy_from_i420(&i420, width, height);

                            guard.frame_id = frame_counter;

                            frame_counter += 1 + skipped;
                        }
                        manager.advance_write();

                        if let Some(tx) = &redraw_tx {
                            if let Err(e) = tx.send(crate::window::screensharing_window::RedrawCommand::ForceRedraw) {
                                log::error!("process_video_stream: failed to send redraw command: {e:?}");
                                break;
                            }
                        }
                    }
                    Ok(None) => {
                        log::info!(
                            "process_video_stream: {} stream ended for participant: {}",
                            stream_type,
                            stream_key
                        );
                        break;
                    }
                    Err(e) => {
                        log::trace!(
                            "process_video_stream: No frames received for 1 seconds from {} [{}], marking as inactive {:?}",
                            stream_key,
                            stream_type,
                            e,
                        );
                        manager.set_inactive(true);
                    }
                }
            }
            _ = stop_rx.recv() => {
                log::info!(
                    "process_video_stream: Received stop signal for {} stream: {}",
                    stream_type,
                    stream_key
                );
                break;
            }
        }
    }

    manager.set_inactive(true);
    log::info!(
        "process_video_stream: {} stream ended for participant: {}",
        stream_type,
        stream_key
    );
}
