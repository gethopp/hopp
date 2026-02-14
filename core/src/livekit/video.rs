use livekit::webrtc::video_frame::I420Buffer;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

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
#[derive(Debug)]
pub struct VideoBufferManager {
    buffers: [Mutex<VideoBuffer>; 2],
    write_index: AtomicUsize,
}

impl VideoBufferManager {
    pub fn new() -> Self {
        Self {
            buffers: [
                Mutex::new(VideoBuffer::default()),
                Mutex::new(VideoBuffer::default()),
            ],
            write_index: AtomicUsize::new(0),
        }
    }

    /// Returns the buffer to write into (the current write slot).
    pub fn write_buffer(&self) -> &Mutex<VideoBuffer> {
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
}
