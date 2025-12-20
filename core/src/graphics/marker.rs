//! Marker rendering system for displaying corner markers in an overlay window.
//!
//! This module provides functionality to render visual markers at the four corners
//! of a window or capture area. Markers are typically used to indicate the boundaries
//! of a screen capture region or overlay area.

use std::{fs::File, io::Read};

use super::{create_texture, OverlayError, Texture, Vertex};
use crate::utils::geometry::Extent;
use wgpu::util::DeviceExt;

/// Represents the four possible positions where markers can be placed.
///
/// Markers are positioned at the corners of the window/capture area to provide
/// visual feedback about the boundaries.
enum MarkerPosition {
    /// Top-left corner of the window
    TopLeft,
    /// Top-right corner of the window
    TopRight,
    /// Bottom-left corner of the window
    BottomLeft,
    /// Bottom-right corner of the window
    BottomRight,
}

/// A single marker containing its texture and rendering buffers.
///
/// Each marker is rendered as a textured quad at one of the four corner positions.
#[derive(Debug)]
struct Marker {
    /// The texture containing the marker image
    texture: Texture,
    /// Vertex buffer containing the quad vertices for this marker
    vertex_buffer: wgpu::Buffer,
    /// Index buffer for rendering the quad triangles
    index_buffer: wgpu::Buffer,
}

/// Renderer for displaying corner markers in an overlay window.
///
/// The `MarkerRenderer` manages the rendering of visual markers at all four corners
/// of the window. It loads marker textures from PNG files and renders them as
/// textured quads using a shared render pipeline.
#[derive(Debug)]
pub struct MarkerRenderer {
    /// Collection of all four corner markers
    markers: Vec<Marker>,
    /// Shared render pipeline for all markers
    render_pipeline: wgpu::RenderPipeline,
}

impl MarkerRenderer {
    /// Creates a new marker renderer with markers at all four corners.
    ///
    /// This method sets up the rendering pipeline and loads marker textures from
    /// PNG files. It expects to find the following files in the texture path:
    /// - `marker_top_left.png`
    /// - `marker_top_right.png`
    /// - `marker_bottom_left.png`
    /// - `marker_bottom_right.png`
    ///
    /// # Arguments
    ///
    /// * `device` - The WGPU device for creating GPU resources
    /// * `queue` - The WGPU queue for uploading data
    /// * `texture_format` - The target texture format for rendering
    /// * `texture_path` - Optional base path for loading marker texture files
    /// * `window_size` - The size of the window/overlay area
    /// * `scale` - Display scale
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing the new `MarkerRenderer` on success,
    /// or an `OverlayError` if texture loading or GPU resource creation fails.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture_format: wgpu::TextureFormat,
        texture_path: &String,
        window_size: Extent,
        scale: f64,
    ) -> Result<Self, OverlayError> {
        // Create bind group layout for texture and sampler
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Shared Marker Texture BGL"),
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

        // Load shader and create render pipeline
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));
        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Marker Layout"),
                bind_group_layouts: &[&texture_bind_group_layout],
                immediate_size: 0,
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline Marker"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_lines_main"),
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

        // Define marker image files and their positions
        let marker_imgs = if scale <= 1.0 {
            vec![
                ("marker_top_left.png", MarkerPosition::TopLeft),
                ("marker_top_right.png", MarkerPosition::TopRight),
                ("marker_bottom_left.png", MarkerPosition::BottomLeft),
                ("marker_bottom_right.png", MarkerPosition::BottomRight),
            ]
        } else {
            vec![
                ("marker_top_left_big.png", MarkerPosition::TopLeft),
                ("marker_top_right_big.png", MarkerPosition::TopRight),
                ("marker_bottom_left_big.png", MarkerPosition::BottomLeft),
                ("marker_bottom_right_big.png", MarkerPosition::BottomRight),
            ]
        };

        // Load textures and create markers for each position
        let mut markers = Vec::new();
        for (img, position) in marker_imgs {
            let resource_path = format!("{texture_path}/{img}");
            log::debug!("create_cursor_texture: resource path: {resource_path:?}");

            let mut file = match File::open(&resource_path) {
                Ok(file) => file,
                Err(_) => {
                    log::error!("create_cursor_texture: failed to open file: {img}");
                    return Err(OverlayError::TextureCreationError);
                }
            };
            let mut image_buffer = Vec::new();
            let res = file.read_to_end(&mut image_buffer);
            if res.is_err() {
                log::error!("create_cursor_texture: failed to read file: {img}");
                return Err(OverlayError::TextureCreationError);
            }

            let texture = create_texture(device, queue, &image_buffer, &texture_bind_group_layout)?;
            let (vertex_buffer, index_buffer) =
                Self::create_vertex_buffer(device, window_size, position, texture.extent);
            markers.push(Marker {
                texture,
                vertex_buffer,
                index_buffer,
            });
        }

        Ok(MarkerRenderer {
            markers,
            render_pipeline,
        })
    }

    /// Creates vertex and index buffers for a marker at the specified position.
    ///
    /// This method calculates the appropriate vertex positions for a marker quad
    /// based on the window size, marker position, and texture size. The vertices
    /// are positioned in normalized device coordinates (NDC) where the window
    /// spans from -1 to 1 in both X and Y directions.
    ///
    /// # Arguments
    ///
    /// * `device` - The WGPU device for creating buffers
    /// * `window_size` - The size of the window/overlay area in pixels
    /// * `position` - Which corner to position the marker at
    /// * `texture_size` - The size of the marker texture in pixels
    ///
    /// # Returns
    ///
    /// Returns a tuple containing the vertex buffer and index buffer for the marker quad.
    /// The buffers contain data for a rectangle with appropriate texture coordinates.
    fn create_vertex_buffer(
        device: &wgpu::Device,
        window_size: Extent,
        position: MarkerPosition,
        texture_size: Extent,
    ) -> (wgpu::Buffer, wgpu::Buffer) {
        // Calculate the size of the marker in normalized device coordinates
        let clip_extent = Extent {
            width: texture_size.width / window_size.width,
            height: texture_size.height / window_size.height,
        };

        // Calculate vertex positions based on marker position
        let (x, y, x2, y2) = match position {
            MarkerPosition::TopLeft => (
                -1.0,
                1.0,
                -1.0 + clip_extent.width as f32,
                1.0 - clip_extent.height as f32,
            ),
            MarkerPosition::TopRight => (
                1.0 - clip_extent.width as f32,
                1.0,
                1.0,
                1.0 - clip_extent.height as f32,
            ),
            MarkerPosition::BottomLeft => (
                -1.0,
                -1.0 + clip_extent.height as f32,
                -1.0 + clip_extent.width as f32,
                -1.0,
            ),
            MarkerPosition::BottomRight => (
                1.0 - clip_extent.width as f32,
                -1.0 + clip_extent.height as f32,
                1.0,
                -1.0,
            ),
        };

        // Create vertices for a quad with texture coordinates
        let vertices = vec![
            Vertex {
                position: [x, y],
                texture_coords: [0.0, 0.0],
            },
            Vertex {
                position: [x, y2],
                texture_coords: [0.0, 1.0],
            },
            Vertex {
                position: [x2, y2],
                texture_coords: [1.0, 1.0],
            },
            Vertex {
                position: [x2, y],
                texture_coords: [1.0, 0.0],
            },
        ];

        // Define indices for two triangles forming a quad
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

    /// Renders all four corner markers to the current render pass.
    ///
    /// This method draws all markers using the shared render pipeline.
    /// Each marker is rendered as a textured quad at its designated corner position.
    ///
    /// # Arguments
    ///
    /// * `render_pass` - The active render pass to draw the markers into
    pub fn draw(&self, render_pass: &mut wgpu::RenderPass) {
        render_pass.set_pipeline(&self.render_pipeline);
        for marker in &self.markers {
            render_pass.set_bind_group(0, &marker.texture.bind_group, &[]);
            render_pass.set_vertex_buffer(0, marker.vertex_buffer.slice(..));
            render_pass.set_index_buffer(marker.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            render_pass.draw_indexed(0..6, 0, 0..1);
        }
    }
}
