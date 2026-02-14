//! GPU-accelerated YUV video renderer using iced's shader widget.
//!
//! Implements `iced::widget::shader::Program` and the associated `Primitive`/`Pipeline`
//! traits to render YUV I420 frames via a custom WGSL shader. Each participant tile
//! uses a `YuvVideoProgram` that reads from a shared `YuvBuffer` and renders through
//! the GPU with center-crop aspect-ratio correction.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use iced::mouse;
use iced::widget::shader;
use iced::Rectangle;
use iced_wgpu::primitive;
use wgpu;

use super::yuv_buffer::YuvBuffer;

// ── Params uniform matching the WGSL struct ─────────────────────────────────

/// GPU uniform buffer matching the `Params` struct in `yuv_shader.wgsl`.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    src_w: u32,
    src_h: u32,
    y_tex_w: u32,
    uv_tex_w: u32,
    tile_aspect_num: u32,
    tile_aspect_den: u32,
    tile_w: f32,
    tile_h: f32,
    corner_radius: f32,
    // Pad to 48 bytes (wgpu requires uniform buffer sizes to be multiples of 16)
    _pad: [u32; 3],
}

// ── Per-participant GPU state ────────────────────────────────────────────────

/// GPU textures and bind group for a single participant's YUV frame.
struct ParticipantGpuState {
    y_tex: wgpu::Texture,
    u_tex: wgpu::Texture,
    v_tex: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    params_buf: wgpu::Buffer,
    dims: (u32, u32),
}

// ── Pipeline (shared GPU state) ─────────────────────────────────────────────

/// Shared GPU pipeline state for all YUV video widgets.
///
/// Created once by iced when the first `YuvVideoPrimitive` is encountered.
/// Contains the render pipeline, sampler, bind group layout, and per-participant
/// texture state.
pub struct YuvPipeline {
    render_pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    bind_group_layout: wgpu::BindGroupLayout,
    /// Per-participant GPU textures, keyed by participant ID.
    participants: HashMap<u64, ParticipantGpuState>,
}

impl std::fmt::Debug for YuvPipeline {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YuvPipeline")
            .field("participants_count", &self.participants.len())
            .finish()
    }
}

/// Align a value up to the given alignment.
fn align_to(value: u32, alignment: u32) -> u32 {
    (value + alignment - 1) / alignment * alignment
}

impl YuvPipeline {
    /// Ensure GPU textures exist for the given participant with the correct dimensions.
    /// Returns a mutable reference to the participant's GPU state.
    fn ensure_textures(
        &mut self,
        participant_id: u64,
        device: &wgpu::Device,
        width: u32,
        height: u32,
    ) -> &mut ParticipantGpuState {
        let needs_create = match self.participants.get(&participant_id) {
            Some(state) => state.dims != (width, height),
            None => true,
        };

        if needs_create {
            // wgpu requires bytes_per_row to be a multiple of 256 for texture uploads.
            // For R8Unorm textures, bytes_per_row = texture_width (1 byte per pixel).
            // So we align the texture width to 256.
            let y_tex_w = align_to(width, 256);
            let uv_tex_w = align_to(width / 2, 256);

            let y_tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("YUV Y texture"),
                size: wgpu::Extent3d {
                    width: y_tex_w,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            let u_tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("YUV U texture"),
                size: wgpu::Extent3d {
                    width: uv_tex_w,
                    height: height / 2,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            let v_tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("YUV V texture"),
                size: wgpu::Extent3d {
                    width: uv_tex_w,
                    height: height / 2,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("YUV params uniform"),
                size: std::mem::size_of::<Params>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            let y_view = y_tex.create_view(&wgpu::TextureViewDescriptor::default());
            let u_view = u_tex.create_view(&wgpu::TextureViewDescriptor::default());
            let v_view = v_tex.create_view(&wgpu::TextureViewDescriptor::default());

            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("YUV bind group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&y_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(&u_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&v_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: params_buf.as_entire_binding(),
                    },
                ],
            });

            self.participants.insert(
                participant_id,
                ParticipantGpuState {
                    y_tex,
                    u_tex,
                    v_tex,
                    bind_group,
                    params_buf,
                    dims: (width, height),
                },
            );
        }

        self.participants.get_mut(&participant_id).unwrap()
    }

    /// Upload YUV plane data to GPU textures for the given participant.
    fn upload_yuv(
        &mut self,
        participant_id: u64,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        buf: &YuvBuffer,
        tile_width: u32,
        tile_height: u32,
        corner_radius: f32,
    ) {
        let state = self.ensure_textures(participant_id, device, buf.width, buf.height);

        let y_tex_w = align_to(buf.width, 256);
        let uv_tex_w = align_to(buf.width / 2, 256);

        // Upload Y plane (pad rows to aligned width)
        {
            let mut padded = vec![0u8; (y_tex_w * buf.height) as usize];
            for row in 0..buf.height {
                let src_start = (row * buf.stride_y) as usize;
                let src_end = src_start + buf.width as usize;
                let dst_start = (row * y_tex_w) as usize;
                let dst_end = dst_start + buf.width as usize;
                if src_end <= buf.y.len() {
                    padded[dst_start..dst_end].copy_from_slice(&buf.y[src_start..src_end]);
                }
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &state.y_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &padded,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(y_tex_w),
                    rows_per_image: Some(buf.height),
                },
                wgpu::Extent3d {
                    width: y_tex_w,
                    height: buf.height,
                    depth_or_array_layers: 1,
                },
            );
        }

        // Upload U plane
        {
            let uv_h = buf.height / 2;
            let uv_w = buf.width / 2;
            let mut padded = vec![128u8; (uv_tex_w * uv_h) as usize];
            for row in 0..uv_h {
                let src_start = (row * buf.stride_u) as usize;
                let src_end = src_start + uv_w as usize;
                let dst_start = (row * uv_tex_w) as usize;
                let dst_end = dst_start + uv_w as usize;
                if src_end <= buf.u.len() {
                    padded[dst_start..dst_end].copy_from_slice(&buf.u[src_start..src_end]);
                }
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &state.u_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &padded,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(uv_tex_w),
                    rows_per_image: Some(uv_h),
                },
                wgpu::Extent3d {
                    width: uv_tex_w,
                    height: uv_h,
                    depth_or_array_layers: 1,
                },
            );
        }

        // Upload V plane
        {
            let uv_h = buf.height / 2;
            let uv_w = buf.width / 2;
            let mut padded = vec![128u8; (uv_tex_w * uv_h) as usize];
            for row in 0..uv_h {
                let src_start = (row * buf.stride_v) as usize;
                let src_end = src_start + uv_w as usize;
                let dst_start = (row * uv_tex_w) as usize;
                let dst_end = dst_start + uv_w as usize;
                if src_end <= buf.v.len() {
                    padded[dst_start..dst_end].copy_from_slice(&buf.v[src_start..src_end]);
                }
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &state.v_tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &padded,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(uv_tex_w),
                    rows_per_image: Some(uv_h),
                },
                wgpu::Extent3d {
                    width: uv_tex_w,
                    height: uv_h,
                    depth_or_array_layers: 1,
                },
            );
        }

        // Update params uniform
        // Use integer ratio for tile aspect to avoid float in uniform
        // Multiply by 1000 for precision
        let tile_aspect_num = (tile_width as f32 * 1000.0) as u32;
        let tile_aspect_den = (tile_height.max(1) as f32 * 1000.0) as u32;

        let params = Params {
            src_w: buf.width,
            src_h: buf.height,
            y_tex_w,
            uv_tex_w,
            tile_aspect_num,
            tile_aspect_den,
            tile_w: tile_width as f32,
            tile_h: tile_height as f32,
            corner_radius,
            _pad: [0; 3],
        };
        queue.write_buffer(&state.params_buf, 0, bytemuck::bytes_of(&params));
    }
}

impl primitive::Pipeline for YuvPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("YUV shader"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!(
                "../shaders/yuv_shader.wgsl"
            ))),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("YUV bind group layout"),
            entries: &[
                // Sampler
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                // Y texture
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // U texture
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // V texture
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                // Params uniform
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("YUV pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("YUV render pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            multiview: None,
            cache: None,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("YUV sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self {
            render_pipeline,
            sampler,
            bind_group_layout,
            participants: HashMap::new(),
        }
    }
}

// ── Primitive ────────────────────────────────────────────────────────────────

/// A snapshot of YUV frame data ready for GPU upload and rendering.
///
/// Created each frame by `YuvVideoProgram::draw()`. Holds a copy of the
/// YUV plane data (to avoid holding the mutex during GPU operations) and
/// the tile dimensions for center-crop computation.
#[derive(Debug)]
pub struct YuvVideoPrimitive {
    participant_id: u64,
    /// Snapshot of YUV data (cloned from the shared buffer)
    y_data: Vec<u8>,
    u_data: Vec<u8>,
    v_data: Vec<u8>,
    src_width: u32,
    src_height: u32,
    stride_y: u32,
    stride_u: u32,
    stride_v: u32,
    tile_width: u32,
    tile_height: u32,
    corner_radius: f32,
    dirty: bool,
}

impl primitive::Primitive for YuvVideoPrimitive {
    type Pipeline = YuvPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &iced_wgpu::graphics::Viewport,
    ) {
        if !self.dirty && pipeline.participants.contains_key(&self.participant_id) {
            return;
        }

        // Build a temporary YuvBuffer for the upload
        let buf = YuvBuffer {
            width: self.src_width,
            height: self.src_height,
            stride_y: self.stride_y,
            stride_u: self.stride_u,
            stride_v: self.stride_v,
            y: self.y_data.clone(),
            u: self.u_data.clone(),
            v: self.v_data.clone(),
            dirty: self.dirty,
        };

        pipeline.upload_yuv(
            self.participant_id,
            device,
            queue,
            &buf,
            self.tile_width,
            self.tile_height,
            self.corner_radius,
        );
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        let state = match pipeline.participants.get(&self.participant_id) {
            Some(s) => s,
            None => return false,
        };

        // The viewport and scissor rect are already set by iced to the widget bounds.
        render_pass.set_pipeline(&pipeline.render_pipeline);
        render_pass.set_bind_group(0, &state.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
        true
    }
}

// ── Program ──────────────────────────────────────────────────────────────────

/// An iced shader `Program` that renders a participant's YUV video frame.
///
/// Each participant tile in the camera window uses its own `YuvVideoProgram`
/// instance, which reads from the participant's shared `YuvBuffer`.
pub struct YuvVideoProgram {
    pub participant_id: u64,
    pub buffer: Arc<Mutex<YuvBuffer>>,
    pub corner_radius: f32,
}

impl std::fmt::Debug for YuvVideoProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YuvVideoProgram")
            .field("participant_id", &self.participant_id)
            .finish()
    }
}

impl<Message> shader::Program<Message> for YuvVideoProgram {
    type State = ();
    type Primitive = YuvVideoPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        let buf = self.buffer.lock().unwrap();

        YuvVideoPrimitive {
            participant_id: self.participant_id,
            y_data: buf.y.clone(),
            u_data: buf.u.clone(),
            v_data: buf.v.clone(),
            src_width: buf.width,
            src_height: buf.height,
            stride_y: buf.stride_y,
            stride_u: buf.stride_u,
            stride_v: buf.stride_v,
            tile_width: bounds.width.max(1.0) as u32,
            tile_height: bounds.height.max(1.0) as u32,
            corner_radius: self.corner_radius,
            dirty: buf.dirty,
        }
    }
}
