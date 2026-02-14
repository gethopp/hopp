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

/// Generate a synthetic SMPTE-style color bar test pattern in YUV I420 format.
///
/// The pattern cycles through hues over time using `frame_num` to provide
/// visual feedback that the GPU pipeline is working without any external
/// video dependencies.
///
/// Color bars: White, Yellow, Cyan, Green, Magenta, Red, Blue, Black
/// with a hue shift based on frame_num for animation.
pub fn generate_test_frame(buf: &mut YuvBuffer, frame_num: u64) {
    let w = buf.width;
    let h = buf.height;

    // SMPTE color bars in RGB, we'll convert to YUV
    // Each bar: (R, G, B)
    let bars: [(u8, u8, u8); 8] = [
        (235, 235, 235), // White
        (235, 235, 16),  // Yellow
        (16, 235, 235),  // Cyan
        (16, 235, 16),   // Green
        (235, 16, 235),  // Magenta
        (235, 16, 16),   // Red
        (16, 16, 235),   // Blue
        (16, 16, 16),    // Black
    ];

    // Animate: shift which bar appears first
    let shift = (frame_num / 1) as usize; // shift every (1 now) 15 frames (~0.5s at 30fps)

    let bar_width = w / 8;

    // Fill Y plane
    for row in 0..h {
        for col in 0..w {
            let bar_idx = ((col / bar_width.max(1)) as usize + shift) % 8;
            let (r, g, b) = bars[bar_idx];

            // BT.601 RGB to Y
            let y_val = ((66 * r as u32 + 129 * g as u32 + 25 * b as u32 + 128) >> 8) as u8 + 16;
            buf.y[(row * buf.stride_y + col) as usize] = y_val;
        }
    }

    // Fill U and V planes (subsampled 2x2)
    let h_uv = h / 2;
    let w_uv = w / 2;
    for row in 0..h_uv {
        for col in 0..w_uv {
            // Map UV coord back to full-res to determine which bar
            let full_col = col * 2;
            let bar_idx = ((full_col / bar_width.max(1)) as usize + shift) % 8;
            let (r, g, b) = bars[bar_idx];

            // BT.601 RGB to U (Cb) and V (Cr)
            let u_val = ((-38i32 * r as i32 - 74 * g as i32 + 112 * b as i32 + 128) >> 8) + 128;
            let v_val = ((112i32 * r as i32 - 94 * g as i32 - 18 * b as i32 + 128) >> 8) + 128;

            buf.u[(row * buf.stride_u + col) as usize] = u_val.clamp(0, 255) as u8;
            buf.v[(row * buf.stride_v + col) as usize] = v_val.clamp(0, 255) as u8;
        }
    }

    buf.dirty = true;
}
