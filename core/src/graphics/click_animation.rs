//! Click animation rendering system for overlay graphics.
//!
//! This module provides a GPU-accelerated click animation rendering system using wgpu.
//! It supports multiple click animations with individual textures, transforms, and positions.
//! The system uses a shared transform buffer with dynamic offsets for efficient
//! rendering of multiple click animations.

use crate::utils::geometry::{Extent, Position};
use std::{collections::VecDeque, fs::File, io::Read};
use wgpu::util::DeviceExt;

use super::{
    create_texture,
    point::{Point, TransformMatrix},
    OverlayError, Texture, Vertex,
};

/// Maximum number of click animations that can be rendered simultaneously
const MAX_ANIMATIONS: usize = 30;

/// Base horizontal offset for click animation positioning (as a fraction of screen space)
const BASE_OFFSET_X: f32 = 0.007;
/// Base vertical offset for click animation positioning (as a fraction of screen space)
const BASE_OFFSET_Y: f32 = 0.015;

/// Represents a single click animation with its texture, geometry, and position data.
///
/// Each click animation maintains its own vertex and index buffers for geometry,
/// a texture for appearance, and position information for rendering.
/// The animation uses dynamic offsets into shared transform and radius buffers.
#[derive(Debug)]
pub struct ClickAnimation {
    /// The click animation's texture (image)
    texture: Texture,
    /// GPU buffer containing vertex data for the animation quad
    vertex_buffer: wgpu::Buffer,
    /// GPU buffer containing index data for the animation quad
    index_buffer: wgpu::Buffer,
    /// Dynamic offset into the shared transform buffer
    transform_offset: wgpu::DynamicOffset,
    /// Position and transformation data
    position: Point,
    /// Dynamic offset into the shared radius buffer
    radius_offset: wgpu::DynamicOffset,
    /// Time when the animation was enabled, None if disabled
    enabled_instant: Option<std::time::Instant>,
}

impl ClickAnimation {
    /// Updates the GPU transform buffer with this animation's current position.
    ///
    /// # Arguments
    /// * `queue` - wgpu queue for uploading data to GPU
    /// * `transforms_buffer` - Shared transform buffer
    ///
    /// This method uploads the animation's transformation matrix to the GPU
    /// at the appropriate offset in the shared buffer.
    pub fn update_transform_buffer(&self, queue: &wgpu::Queue, transforms_buffer: &wgpu::Buffer) {
        queue.write_buffer(
            transforms_buffer,
            self.transform_offset as wgpu::BufferAddress,
            bytemuck::cast_slice(&[self.position.get_transform_matrix()]),
        );
    }

    /// Updates the GPU radius buffer for this animation's current radius value.
    ///
    /// # Arguments
    /// * `queue` - wgpu queue for uploading data to GPU
    /// * `radius_buffer` - Shared radius buffer
    /// * `radius` - Current radius value for the animation
    pub fn update_radius(&self, queue: &wgpu::Queue, radius_buffer: &wgpu::Buffer, radius: f32) {
        queue.write_buffer(
            radius_buffer,
            self.radius_offset as wgpu::BufferAddress,
            bytemuck::cast_slice(&[radius]),
        );
    }

    /// Enables the click animation at the specified position.
    ///
    /// # Arguments
    /// * `position` - Screen position where the animation should appear
    /// * `queue` - wgpu queue for uploading data to GPU
    /// * `transforms_buffer` - Shared transform buffer
    /// * `radius_buffer` - Shared radius buffer
    ///
    /// This method initializes the animation with a starting radius and position,
    /// and records the current time for animation timing.
    pub fn enable(
        &mut self,
        position: Position,
        queue: &wgpu::Queue,
        transforms_buffer: &wgpu::Buffer,
        radius_buffer: &wgpu::Buffer,
    ) {
        self.position
            .set_position(position.x as f32, position.y as f32);
        self.update_transform_buffer(queue, transforms_buffer);
        self.update_radius(queue, radius_buffer, 0.1);
        self.enabled_instant = Some(std::time::Instant::now());
    }

    /// Disables the click animation by moving it off-screen.
    ///
    /// # Arguments
    /// * `queue` - wgpu queue for uploading data to GPU
    /// * `transforms_buffer` - Shared transform buffer
    ///
    /// This method hides the animation by positioning it off-screen and
    /// clears the enabled timestamp.
    pub fn disable(&mut self, queue: &wgpu::Queue, transforms_buffer: &wgpu::Buffer) {
        self.position.set_position(-100.0, -100.0);
        self.update_transform_buffer(queue, transforms_buffer);
        self.enabled_instant = None;
    }

    /// Renders this click animation using the provided render pass.
    ///
    /// # Arguments
    /// * `render_pass` - Active wgpu render pass for drawing
    /// * `queue` - wgpu queue for uploading data to GPU
    /// * `radius_buffer` - Shared radius buffer
    /// * `transforms_bind_group` - Bind group for transformation matrices
    /// * `radius_bind_group` - Bind group for radius values
    ///
    /// This method handles the animation timing, updates the radius based on elapsed time,
    /// and renders the animation to the current render target. The animation automatically
    /// disables itself after 1s.
    pub fn draw(
        &mut self,
        render_pass: &mut wgpu::RenderPass,
        queue: &wgpu::Queue,
        radius_buffer: &wgpu::Buffer,
        transforms_bind_group: &wgpu::BindGroup,
        radius_bind_group: &wgpu::BindGroup,
    ) {
        if self.enabled_instant.is_none() {
            return;
        }
        let enabled_instant = self.enabled_instant.unwrap();
        let radius_start = 0.1;
        let elapsed = enabled_instant.elapsed().as_millis();
        let time_offset = 300;
        if elapsed > time_offset {
            let radius = radius_start + (elapsed - time_offset) as f32 / 2333.0;
            self.update_radius(queue, radius_buffer, radius);
        }
        if elapsed > 1000 {
            self.disable(queue, radius_buffer);
        }
        render_pass.set_bind_group(0, &self.texture.bind_group, &[]);
        render_pass.set_bind_group(1, transforms_bind_group, &[self.transform_offset]);
        render_pass.set_bind_group(2, radius_bind_group, &[self.radius_offset]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        render_pass.draw_indexed(0..6, 0, 0..1);
    }
}

/// Main click animation rendering system that manages multiple animations.
///
/// This renderer creates and manages the GPU resources needed for click animation rendering,
/// including shaders, pipelines, and shared buffers. It uses shared transform and radius
/// buffers with dynamic offsets to efficiently handle multiple animations.
///
/// # Design Notes
///
/// Due to compatibility issues with development Windows VMs, this implementation
/// uses shared buffers with dynamic offsets rather than separate buffers for each animation.
/// A channel is used to safely communicate animation enable requests from other threads
/// to the render thread.
#[derive(Debug)]
pub struct ClickAnimationRenderer {
    /// GPU render pipeline for click animation rendering
    pub render_pipeline: wgpu::RenderPipeline,
    /// Bind group layout for animation textures
    pub texture_bind_group_layout: wgpu::BindGroupLayout,
    /// Bind group layout for transformation matrices
    pub transform_bind_group_layout: wgpu::BindGroupLayout,
    /// Shared buffer containing all animation transform matrices
    pub transforms_buffer: wgpu::Buffer,
    /// Size of each entry in the transform buffer (including alignment)
    pub transforms_buffer_entry_offset: wgpu::BufferAddress,
    /// Bind group for accessing the transform buffer
    pub transforms_bind_group: wgpu::BindGroup,
    /// Bind group layout for animation radius values
    pub radius_bind_group_layout: wgpu::BindGroupLayout,
    /// Shared buffer containing all animation radius values
    pub radius_buffer: wgpu::Buffer,
    /// Size of each entry in the radius buffer (including alignment)
    pub radius_buffer_entry_offset: wgpu::BufferAddress,
    /// Bind group for accessing the radius buffer
    pub radius_bind_group: wgpu::BindGroup,
    /// Sender for communicating animation enable requests to the render thread
    pub clik_animation_position_sender: std::sync::mpsc::Sender<Position>,
    /// Receiver for animation enable requests (only accessed from render thread)
    pub clik_animation_position_receiver: std::sync::mpsc::Receiver<Position>,
    /// Array of all click animation instances
    pub click_animations: Vec<ClickAnimation>,
    /// Queue of available (inactive) animation slots
    pub available_slots: VecDeque<usize>,
    /// Queue of currently used (active) animation slots
    pub used_slots: VecDeque<usize>,
}

struct ClickAnimationCreateData<'a> {
    texture_path: String,
    scale: f64,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    window_size: Extent,
    texture_bind_group_layout: &'a wgpu::BindGroupLayout,
    transforms_buffer_entry_offset: wgpu::BufferAddress,
    transforms_buffer: &'a wgpu::Buffer,
    radius_buffer_entry_offset: wgpu::BufferAddress,
    radius_buffer: &'a wgpu::Buffer,
    animations_created: u32,
}

impl ClickAnimationRenderer {
    /// Creates a new click animation renderer with all necessary GPU resources.
    ///
    /// # Arguments
    /// * `device` - wgpu device for creating GPU resources
    /// * `queue` - wgpu queue for uploading initial data
    /// * `texture_format` - Format of the render target texture
    /// * `texture_path` - Path to the texture resource directory
    /// * `window_size` - Size of the rendering window
    /// * `scale` - Display scale factor
    ///
    /// # Returns
    /// A fully initialized animation renderer ready to render click animations,
    /// or an error if initialization fails.
    ///
    /// This method sets up:
    /// - Bind group layouts for textures, transforms, and radius values
    /// - Shared transform and radius buffers with proper alignment
    /// - Render pipeline with vertex and fragment shaders
    /// - Pre-allocated pool of click animation instances
    /// - Channel for thread-safe animation enable requests
    pub fn create(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture_format: wgpu::TextureFormat,
        texture_path: &str,
        window_size: Extent,
        scale: f64,
    ) -> Result<Self, OverlayError> {
        // Create bind group layout for click animation textures
        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Shared Click Animation Texture BGL"),
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
         * buffer for each animation, we are using a single transform buffer for all animations
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

        // Create shared transform buffer for all animations
        let transforms_buffer_size = aligned_buffer_size * MAX_ANIMATIONS as wgpu::BufferAddress;
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

        // Create radius uniform buffer for click animation, the radius will change over time
        // for the animation
        let radius_buffer_size = std::mem::size_of::<f32>() as wgpu::BufferAddress;
        let aligned_radius_buffer_size =
            (radius_buffer_size + buffer_uniform_alignment - 1) & !(buffer_uniform_alignment - 1);

        let radius_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("Radius BGL"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: std::num::NonZero::new(radius_buffer_size),
                    },
                    count: None,
                }],
            });
        log::info!("aligned_radius_buffer_size: {}", aligned_radius_buffer_size);
        let radius_whole_buffer_size =
            aligned_radius_buffer_size * MAX_ANIMATIONS as wgpu::BufferAddress;
        let radius_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Radius Buffer"),
            size: radius_whole_buffer_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let radius_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Radius Buffer Bind Group"),
            layout: &radius_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &radius_buffer,
                    offset: 0,
                    size: std::num::NonZero::new(radius_buffer_size),
                }),
            }],
        });

        // Load shader and create render pipeline
        let shader = device.create_shader_module(wgpu::include_wgsl!("shader.wgsl"));
        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Click Animation"),
                bind_group_layouts: &[
                    &texture_bind_group_layout,
                    &transform_bind_group_layout,
                    &radius_bind_group_layout,
                ],
                push_constant_ranges: &[],
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Render Pipeline Click Animation"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_click_animation_main"),
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
                entry_point: Some("fs_click_animation_main"),
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
            multiview: None,
            cache: None,
        });

        // Create the click animations
        let mut click_animations = Vec::new();
        let mut available_slots = VecDeque::new();
        for i in 0..MAX_ANIMATIONS {
            let click_animation = Self::create_click_animation(ClickAnimationCreateData {
                texture_path: texture_path.to_owned(),
                scale,
                device,
                queue,
                window_size,
                texture_bind_group_layout: &texture_bind_group_layout,
                transforms_buffer_entry_offset: aligned_buffer_size,
                transforms_buffer: &transforms_buffer,
                radius_buffer_entry_offset: aligned_radius_buffer_size,
                radius_buffer: &radius_buffer,
                animations_created: i as u32,
            })?;

            click_animations.push(click_animation);
            available_slots.push_back(i);
        }

        let (sender, receiver) = std::sync::mpsc::channel();
        Ok(Self {
            render_pipeline,
            texture_bind_group_layout,
            transform_bind_group_layout,
            transforms_buffer,
            transforms_buffer_entry_offset: aligned_buffer_size,
            transforms_bind_group: transform_bind_group,
            radius_bind_group_layout,
            radius_buffer,
            radius_buffer_entry_offset: aligned_radius_buffer_size,
            radius_bind_group,
            clik_animation_position_sender: sender,
            clik_animation_position_receiver: receiver,
            click_animations,
            available_slots,
            used_slots: VecDeque::new(),
        })
    }

    /// Creates a new click animation instance with the specified properties.
    ///
    /// # Arguments
    /// * `data` - Configuration data containing all necessary parameters for animation creation
    ///
    /// # Returns
    /// A new `ClickAnimation` instance ready for rendering, or an error if creation fails.
    ///
    /// # Errors
    /// Returns `OverlayError::TextureCreationError` if:
    /// - The texture file cannot be opened or read
    /// - Texture creation fails
    ///
    /// The animation is automatically positioned off-screen and disabled by default.
    /// Its transform matrix and radius are uploaded to the GPU at the appropriate offsets.
    fn create_click_animation(
        data: ClickAnimationCreateData,
    ) -> Result<ClickAnimation, OverlayError> {
        let resource_path = format!("{}/click_texture.png", data.texture_path);
        log::debug!("create_click_animation: resource path: {resource_path:?}");

        let mut file = match File::open(&resource_path) {
            Ok(file) => file,
            Err(_) => {
                log::error!("create_click_animation: failed to open file: click_texture.png");
                return Err(OverlayError::TextureCreationError);
            }
        };
        let mut image_data = Vec::new();
        let res = file.read_to_end(&mut image_data);
        if res.is_err() {
            log::error!("create_click_animation: failed to read file: click_texture.png");
            return Err(OverlayError::TextureCreationError);
        }

        // Create texture from image file
        let texture = create_texture(
            data.device,
            data.queue,
            &image_data,
            data.texture_bind_group_layout,
        )?;

        // Create vertex and index buffers for animation geometry
        let (vertex_buffer, index_buffer) = Self::create_animation_vertex_buffer(
            data.device,
            &texture,
            data.scale,
            data.window_size,
        );

        // Calculate offset into shared transform buffer
        let transform_offset =
            (data.animations_created as wgpu::BufferAddress) * data.transforms_buffer_entry_offset;

        // Initialize animation position with base offsets
        let point = Point::new(
            0.0,
            0.0,
            BASE_OFFSET_X * (data.scale as f32),
            BASE_OFFSET_Y * (data.scale as f32),
        );

        // Upload initial transform matrix to GPU
        data.queue.write_buffer(
            data.transforms_buffer,
            transform_offset,
            bytemuck::cast_slice(&[point.get_transform_matrix()]),
        );

        let radius_offset =
            (data.animations_created as wgpu::BufferAddress) * data.radius_buffer_entry_offset;
        data.queue.write_buffer(
            data.radius_buffer,
            radius_offset,
            bytemuck::cast_slice(&[0.0f32]),
        );

        Ok(ClickAnimation {
            texture,
            vertex_buffer,
            index_buffer,
            transform_offset: transform_offset as wgpu::DynamicOffset,
            position: point,
            radius_offset: radius_offset as wgpu::DynamicOffset,
            enabled_instant: None,
        })
    }

    /// Creates vertex and index buffers for a click animation quad.
    ///
    /// # Arguments
    /// * `device` - wgpu device for creating buffers
    /// * `texture` - Animation texture containing size information
    /// * `scale` - Scale factor for animation size
    /// * `window_size` - Window dimensions for proper aspect ratio
    ///
    /// # Returns
    /// A tuple containing (vertex_buffer, index_buffer) for the animation quad.
    ///
    /// This method creates a quad that maintains the original texture aspect ratio
    /// while scaling appropriately for the target window size. The quad is positioned
    /// at the top-left of normalized device coordinates and sized according to the
    /// texture dimensions and scale factor.
    fn create_animation_vertex_buffer(
        device: &wgpu::Device,
        texture: &Texture,
        scale: f64,
        window_size: Extent,
    ) -> (wgpu::Buffer, wgpu::Buffer) {
        // Calculate animation size in clip space, maintaining aspect ratio
        let clip_extent = Extent {
            width: (texture.extent.width / window_size.width) * scale * 1.5,
            height: (texture.extent.height / window_size.height) * scale * 1.5,
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

    /// Requests to enable a click animation at the specified position.
    ///
    /// # Arguments
    /// * `position` - Screen position where the animation should appear
    ///
    /// This method sends the position through a channel to the render thread,
    /// where it will be processed on the next draw call. This allows animations
    /// to be triggered from any thread safely.
    pub fn enable_click_animation(&mut self, position: Position) {
        if let Err(e) = self.clik_animation_position_sender.send(position) {
            log::error!("enable_click_animation: error sending position: {e:?}");
        }
    }

    /// Draws all active click animations to the provided render pass.
    ///
    /// # Arguments
    /// * `render_pass` - Active wgpu render pass for drawing
    /// * `queue` - wgpu queue for uploading data to GPU
    ///
    /// This method:
    /// 1. Processes any pending animation enable requests from the channel
    /// 2. Allocates slots for new animations from the available pool
    /// 3. Renders all active animations
    /// 4. Reclaims slots from completed animations back to the available pool
    ///
    /// The method automatically manages the lifecycle of animations, returning
    /// them to the available pool once they complete.
    pub fn draw(&mut self, render_pass: &mut wgpu::RenderPass, queue: &wgpu::Queue) {
        // Drain click animation enable requests.
        while let Ok(position) = self.clik_animation_position_receiver.try_recv() {
            if self.available_slots.is_empty() {
                log::warn!("enable_click_animation: available_slots is empty");
                break;
            }

            let slot = self.available_slots.pop_front().unwrap();
            self.used_slots.push_back(slot);

            self.click_animations[slot].enable(
                position,
                queue,
                &self.transforms_buffer,
                &self.radius_buffer,
            );
        }

        if self.used_slots.is_empty() {
            return;
        }

        render_pass.set_pipeline(&self.render_pipeline);

        for slot in self.used_slots.iter() {
            self.click_animations[*slot].draw(
                render_pass,
                queue,
                &self.radius_buffer,
                &self.transforms_bind_group,
                &self.radius_bind_group,
            );
        }

        loop {
            let front = self.used_slots.front();
            if front.is_none() {
                break;
            }

            let slot = *front.unwrap();
            if self.click_animations[slot].enabled_instant.is_none() {
                let front = self.used_slots.pop_front().unwrap();
                self.available_slots.push_back(front);
            } else {
                break;
            }
        }
    }
}
