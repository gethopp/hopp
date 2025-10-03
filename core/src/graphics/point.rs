/// A 4x4 transformation matrix for GPU vertex transformations.
///
/// This matrix is used to transform cursor vertices in the shader,
/// primarily for positioning cursors at specific screen coordinates.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TransformMatrix {
    pub matrix: [[f32; 4]; 4],
}

/// Uniform buffer data structure containing a transformation matrix.
///
/// This struct is uploaded to the GPU as a uniform buffer to provide
/// transformation data to the vertex shader for cursor positioning.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TranslationUniform {
    transform: TransformMatrix,
}

impl TranslationUniform {
    /// Creates a new translation uniform with an identity transformation matrix.
    ///
    /// The identity matrix means no transformation is applied initially.
    fn new() -> Self {
        Self {
            transform: TransformMatrix {
                matrix: [
                    [1.0, 0.0, 0.0, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [0.0, 0.0, 1.0, 0.0],
                    [0.0, 0.0, 0.0, 1.0],
                ],
            },
        }
    }

    /// Sets the translation component of the transformation matrix.
    ///
    /// # Arguments
    /// * `x` - Horizontal translation in normalized device coordinates (-1.0 to 1.0)
    /// * `y` - Vertical translation in normalized device coordinates (-1.0 to 1.0)
    ///
    /// # Note
    /// The coordinates are multiplied by 2.0 because the input is expected to be
    /// in the range 0.0-1.0, but NDC space ranges from -1.0 to 1.0.
    /// Y is negated to match screen coordinate conventions.
    fn set_translation(&mut self, x: f32, y: f32) {
        // We need to multiply by 2.0 because the cursor position is in the range of -1.0 to 1.0
        self.transform.matrix[3][0] = x * 2.0;
        self.transform.matrix[3][1] = -y * 2.0;
    }
}

/// Represents a point in 2D space with position and offset information.
///
/// This struct manages cursor positioning with both absolute coordinates
/// and rendering offsets. The transform matrix is automatically updated
/// when the position changes.
#[derive(Debug)]
pub struct Point {
    /// Absolute X coordinate
    x: f32,
    /// Absolute Y coordinate
    y: f32,
    /// Horizontal rendering offset
    offset_x: f32,
    /// Vertical rendering offset
    offset_y: f32,
    /// GPU transformation matrix for this point
    transform_matrix: TranslationUniform,
}

impl Point {
    /// Creates a new point with the specified position and offsets.
    ///
    /// # Arguments
    /// * `x` - Initial X coordinate
    /// * `y` - Initial Y coordinate
    /// * `offset_x` - Horizontal rendering offset
    /// * `offset_y` - Vertical rendering offset
    pub fn new(x: f32, y: f32, offset_x: f32, offset_y: f32) -> Self {
        Self {
            x,
            y,
            offset_x,
            offset_y,
            transform_matrix: TranslationUniform::new(),
        }
    }

    /// Returns the current transformation matrix for GPU upload.
    pub fn get_transform_matrix(&self) -> TransformMatrix {
        self.transform_matrix.transform
    }

    /// Updates the point's position and recalculates the transformation matrix.
    ///
    /// # Arguments
    /// * `x` - New X coordinate
    /// * `y` - New Y coordinate
    ///
    /// The transformation matrix is updated to position the cursor at the
    /// specified coordinates, accounting for the configured offsets.
    pub fn set_position(&mut self, x: f32, y: f32) {
        self.x = x;
        self.y = y;
        self.transform_matrix
            .set_translation(x - self.offset_x, y - self.offset_y);
    }
}
