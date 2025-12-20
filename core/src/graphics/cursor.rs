//! Cursor rendering system for overlay graphics.
//!
//! This module provides a GPU-accelerated cursor rendering system using wgpu.
//! It supports multiple cursors with individual textures, transforms, and positions.
//! The system uses a shared transform buffer with dynamic offsets for efficient
//! rendering of multiple cursors.

use crate::utils::geometry::Extent;
use wgpu::util::DeviceExt;

use super::{
    create_texture,
    point::{Point, TransformMatrix},
    GraphicsContext, OverlayError, Texture, Vertex,
};

/// Maximum number of cursors that can be rendered simultaneously
const MAX_CURSORS: u32 = 100;
/// Base horizontal offset for cursor positioning (as a fraction of screen space)
const BASE_OFFSET_X: f32 = 0.001;
/// Base vertical offset for cursor positioning (as a fraction of screen space)
const BASE_OFFSET_Y: f32 = 0.002;

/// Represents a single cursor with its texture, geometry, and position data.
///
/// Each cursor maintains its own vertex and index buffers for geometry,
/// a texture for appearance, and position information for rendering.
/// The cursor uses a dynamic offset into a shared transform buffer.
#[derive(Debug)]
pub struct Cursor {
    /// The cursor's texture (image)
    texture: Texture,
    /// GPU buffer containing vertex data for the cursor quad
    vertex_buffer: wgpu::Buffer,
    /// GPU buffer containing index data for the cursor quad
    index_buffer: wgpu::Buffer,
    /// Dynamic offset into the shared transform buffer
    transform_offset: wgpu::DynamicOffset,
    /// Position and transformation data
    position: Point,
}

impl Cursor {
    /// Updates the cursor's position.
    ///
    /// # Arguments
    /// * `x` - New X coordinate (0.0 to 1.0, representing screen space)
    /// * `y` - New Y coordinate (0.0 to 1.0, representing screen space)
    pub fn set_position(&mut self, x: f64, y: f64) {
        self.position.set_position(x as f32, y as f32);
    }

    /// Returns the current transformation matrix for this cursor.
    ///
    /// This matrix can be used to position the cursor in 3D space or
    /// for other transformation calculations.
    pub fn get_translation_matrix(&self) -> TransformMatrix {
        self.position.get_transform_matrix()
    }

    /// Updates the GPU transform buffer with this cursor's current position.
    ///
    /// # Arguments
    /// * `gfx` - Graphics context containing the shared transform buffer
    ///
    /// This method uploads the cursor's transformation matrix to the GPU
    /// at the appropriate offset in the shared buffer.
    pub fn update_transform_buffer(&self, gfx: &GraphicsContext) {
        gfx.queue.write_buffer(
            &gfx.cursor_renderer.transforms_buffer,
            self.transform_offset as wgpu::BufferAddress,
            bytemuck::cast_slice(&[self.position.get_transform_matrix()]),
        );
    }

    /// Renders this cursor using the provided render pass.
    ///
    /// # Arguments
    /// * `render_pass` - Active wgpu render pass for drawing
    /// * `gfx` - Graphics context containing shared rendering resources
    ///
    /// This method sets up the necessary bind groups, buffers, and draw call
    /// to render the cursor to the current render target.
    pub fn draw(&self, render_pass: &mut wgpu::RenderPass, gfx: &GraphicsContext) {
        render_pass.set_bind_group(0, &self.texture.bind_group, &[]);
        render_pass.set_bind_group(
            1,
            &gfx.cursor_renderer.transforms_bind_group,
            &[self.transform_offset],
        );
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.draw_indexed(0..6, 0, 0..1);
    }
}

/// Main cursor rendering system that manages multiple cursors.
///
/// This renderer creates and manages the GPU resources needed for cursor rendering,
/// including shaders, pipelines, and shared buffers. It uses a single transform
/// buffer with dynamic offsets to efficiently handle multiple cursors.
///
/// # Design Notes
///
/// Due to compatibility issues with development Windows VMs, this implementation
/// uses a shared transform buffer with dynamic offsets rather than separate
/// buffers for each cursor.
#[derive(Debug)]
pub struct CursorsRenderer {
    /// GPU render pipeline for cursor rendering
    pub render_pipeline: wgpu::RenderPipeline,
    /// Bind group layout for cursor textures
    pub texture_bind_group_layout: wgpu::BindGroupLayout,
    /// Bind group layout for transformation matrices
    pub transform_bind_group_layout: wgpu::BindGroupLayout,
    /// Shared buffer containing all cursor transform matrices
    pub transforms_buffer: wgpu::Buffer,
    /// Size of each entry in the transform buffer (including alignment)
    pub transforms_buffer_entry_offset: wgpu::BufferAddress,
    /// Bind group for accessing the transform buffer
    pub transforms_bind_group: wgpu::BindGroup,
    /// Number of cursors that have been created
    pub cursors_created: u32,
}

impl CursorsRenderer {
    /// Creates a new cursor renderer with all necessary GPU resources.
    ///
    /// # Arguments
    /// * `device` - wgpu device for creating GPU resources
    /// * `texture_format` - Format of the render target texture
    ///
    /// # Returns
    /// A fully initialized cursor renderer ready to create and render cursors.
    ///
    /// This method sets up:
    /// - Bind group layouts for textures and transforms
    /// - A shared transform buffer with proper alignment
    /// - Render pipeline with vertex and fragment shaders
    /// - All necessary GPU state for cursor rendering
    pub fn create(device: &wgpu::Device, texture_format: wgpu::TextureFormat) -> Self {
        // Create bind group layout for cursor textures
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Shared Cursor Texture BGL"),
                entries: &[
                    // Texture
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    // Sampler
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        /*
         * Because of an issue in our dev windows vm when using a separate transform
         * buffer for each cursor, we are using a single transform buffer for all cursors
         * with dynamic offsets.
         */

        // Calculate proper buffer alignment for transform matrices
        let device_limits = device.limits();
        let buffer_uniform_alignment =
            device_limits.min_uniform_buffer_offset_alignment as wgpu::BufferAddress;
        let transform_buffer_size = std::mem::size_of::<TransformMatrix>() as wgpu::BufferAddress;
        let aligned_buffer_size = (transform_buffer_size + buffer_uniform_alignment - 1)
            & !(buffer_uniform_alignment - 1);

        // Create bind group layout for transformation matrices
        let transform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Transform BGL"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: std::num::NonZero::new(transform_buffer_size),
                    },
                    count: None,
                }],
            });

        // Create shared transform buffer for all cursors
        let transforms_buffer_size = aligned_buffer_size * MAX_CURSORS as wgpu::BufferAddress;
        let transforms_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Transforms Buffer"),
            size: transforms_buffer_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Create bind group for the transform buffer
        let transform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Transforms Buffer Bind Group"),
            layout: &transform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &transforms_buffer,
                    offset: 0,
                    size: std::num::NonZero::new(transform_buffer_size),
                }),
            }],
        });

        // Load shader and create render pipeline
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));
        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline"),
                bind_group_layouts: &[&texture_bind_group_layout, &transform_bind_group_layout],
                immediate_size: 0,
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            offset: 0,
                            shader_location: 0,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                        wgpu::VertexAttribute {
                            offset: std::mem::size_of::<[f32; 2]>() as wgpu::BufferAddress,
                            shader_location: 1,
                            format: wgpu::VertexFormat::Float32x2,
                        },
                    ],
                }],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: texture_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: Some(wgpu::Face::Back),
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState {
                count: 1,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        Self {
            render_pipeline,
            texture_bind_group_layout,
            transform_bind_group_layout,
            transforms_buffer,
            transforms_buffer_entry_offset: aligned_buffer_size,
            transforms_bind_group: transform_bind_group,
            cursors_created: 0,
        }
    }

    /// Creates a new cursor with the specified image and properties.
    ///
    /// # Arguments
    /// * `image_data` - Loaded image data
    /// * `scale` - Display scale
    /// * `device` - wgpu device for creating GPU resources
    /// * `queue` - wgpu queue for uploading data to GPU
    /// * `texture_path` - texture path
    /// * `window_size` - Size of the rendering window for proper scaling
    ///
    /// # Returns
    /// A new `Cursor` instance ready for rendering, or an error if creation fails.
    ///
    /// # Errors
    /// Returns `OverlayError::TextureCreationError` if:
    /// - The maximum number of cursors has been reached
    /// - Texture creation fails
    ///
    /// The cursor is automatically positioned at (0,0) and its transform matrix
    /// is uploaded to the GPU.
    pub fn create_cursor(
        &mut self,
        image_data: &[u8],
        scale: f64,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        window_size: Extent,
    ) -> Result<Cursor, OverlayError> {
        if self.cursors_created >= MAX_CURSORS {
            log::error!("create_cursor: maximum number of cursors reached");
            return Err(OverlayError::TextureCreationError);
        }

        // Create texture from image file
        let texture = create_texture(device, queue, image_data, &self.texture_bind_group_layout)?;

        // Create vertex and index buffers for cursor geometry
        let (vertex_buffer, index_buffer) =
            Self::create_cursor_vertex_buffer(device, &texture, scale, window_size);

        // Calculate offset into shared transform buffer
        let transform_offset =
            (self.cursors_created as wgpu::BufferAddress) * self.transforms_buffer_entry_offset;
        self.cursors_created += 1;

        // Initialize cursor position with base offsets
        let point = Point::new(
            0.0,
            0.0,
            BASE_OFFSET_X * (scale as f32),
            BASE_OFFSET_Y * (scale as f32),
        );

        // Upload initial transform matrix to GPU
        queue.write_buffer(
            &self.transforms_buffer,
            transform_offset,
            bytemuck::cast_slice(&[point.get_transform_matrix()]),
        );

        Ok(Cursor {
            texture,
            vertex_buffer,
            index_buffer,
            transform_offset: transform_offset as wgpu::DynamicOffset,
            position: point,
        })
    }

    /// Creates vertex and index buffers for a cursor quad.
    ///
    /// # Arguments
    /// * `device` - wgpu device for creating buffers
    /// * `texture` - Cursor texture containing size information
    /// * `scale` - Scale factor for cursor size
    /// * `window_size` - Window dimensions for proper aspect ratio
    ///
    /// # Returns
    /// A tuple containing (vertex_buffer, index_buffer) for the cursor quad.
    ///
    /// This method creates a quad that maintains the original texture aspect ratio
    /// while scaling appropriately for the target window size. The quad is positioned
    /// at the top-left of normalized device coordinates and sized according to the
    /// texture dimensions and scale factor.
    fn create_cursor_vertex_buffer(
        device: &wgpu::Device,
        texture: &Texture,
        scale: f64,
        window_size: Extent,
    ) -> (wgpu::Buffer, wgpu::Buffer) {
        /*
         * Here we want to make the cursor size in the shader to always
         * be relative to the monitor extents. Also we want to keep the
         * original ratio of the texture.
         */

        // Calculate cursor size in clip space, maintaining aspect ratio
        let clip_extent = Extent {
            width: (texture.extent.width / window_size.width) * 2.0 * scale / 2.5,
            height: (texture.extent.height / window_size.height) * 2.0 * scale / 2.5,
        };

        // Create quad vertices with texture coordinates
        let vertices = vec![
            Vertex {
                position: [-1.0, 1.0],
                texture_coords: [0.0, 0.0],
            },
            Vertex {
                position: [-1.0, 1.0 - clip_extent.height as f32],
                texture_coords: [0.0, 1.0],
            },
            Vertex {
                position: [
                    -1.0 + clip_extent.width as f32,
                    1.0 - clip_extent.height as f32,
                ],
                texture_coords: [1.0, 1.0],
            },
            Vertex {
                position: [-1.0 + clip_extent.width as f32, 1.0],
                texture_coords: [1.0, 0.0],
            },
        ];

        // Define triangle indices for the quad (two triangles)
        let indices = vec![0, 1, 2, 0, 2, 3];

        // Create GPU buffers
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Buffer"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Index Buffer"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        (vertex_buffer, index_buffer)
    }
}
