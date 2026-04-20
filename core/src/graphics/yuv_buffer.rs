//! YUV I420 frame buffer for GPU-accelerated video rendering.
//!
//! Stores per-participant video frames in YUV I420 format (three separate planes:
//! Y at full resolution, U and V at half resolution). The buffer is shared between
//! a frame feeder thread (writer) and the GPU renderer (reader) via `Arc<Mutex<>>`.

/// A YUV I420 frame buffer.
///
/// Stores Y, U, V planes separately with their respective strides.
/// The `dirty` flag indicates that new data has been written and the
/// GPU textures need to be re-uploaded.
pub struct YuvBuffer {
    pub width: u32,
    pub height: u32,
    pub stride_y: u32,
    pub stride_u: u32,
    pub stride_v: u32,
    pub y: Vec<u8>,
    pub u: Vec<u8>,
    pub v: Vec<u8>,
    pub dirty: bool,
}

impl YuvBuffer {
    /// Create a new YUV buffer with the given dimensions.
    ///
    /// Allocates planes for I420 format:
    /// - Y plane: width × height
    /// - U plane: (width/2) × (height/2)
    /// - V plane: (width/2) × (height/2)
    pub fn new(width: u32, height: u32) -> Self {
        let stride_y = width;
        let stride_u = width / 2;
        let stride_v = width / 2;
        let y = vec![0u8; (stride_y * height) as usize];
        let u = vec![128u8; (stride_u * (height / 2)) as usize];
        let v = vec![128u8; (stride_v * (height / 2)) as usize];

        Self {
            width,
            height,
            stride_y,
            stride_u,
            stride_v,
            y,
            u,
            v,
            dirty: true,
        }
    }

    /// Write raw I420 plane data into the buffer.
    pub fn write_planes(&mut self, y: &[u8], u: &[u8], v: &[u8]) {
        let y_len = (self.stride_y * self.height) as usize;
        let uv_len = (self.stride_u * (self.height / 2)) as usize;

        if y.len() >= y_len {
            self.y[..y_len].copy_from_slice(&y[..y_len]);
        }
        if u.len() >= uv_len {
            self.u[..uv_len].copy_from_slice(&u[..uv_len]);
        }
        if v.len() >= uv_len {
            self.v[..uv_len].copy_from_slice(&v[..uv_len]);
        }
        self.dirty = true;
    }
}
