//! GPU-accelerated YUV video renderer using iced's shader widget.
//!
//! Implements `iced::widget::shader::Program` and the associated `Primitive`/`Pipeline`
//! traits to render YUV I420 frames via a custom WGSL shader. Each participant tile
//! uses a `YuvVideoProgram` that reads from a `VideoBufferManager` and renders through
//! the GPU with center-crop aspect-ratio correction.

use std::collections::HashMap;
use std::sync::Arc;

use iced::mouse;
use iced::widget::shader;
use iced::Rectangle;
use iced_wgpu::primitive;
use wgpu;

use crate::livekit::video::VideoBufferManager;

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
    /// When non-zero, skip aspect-ratio crop and stretch the source to fill the tile.
    stretch_to_fill: u32,
    /// When non-zero, flip the frame horizontally (for local camera mirror).
    flip_horizontal: u32,
    // Pad to 48 bytes (wgpu requires uniform buffer sizes to be multiples of 16)
    _pad: [u32; 2],
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
    value.div_ceil(alignment) * alignment
}

pub(crate) struct YuvFrameData<'a> {
    pub width: u32,
    pub height: u32,
    pub y: &'a [u8],
    pub u: &'a [u8],
    pub v: &'a [u8],
}

pub(crate) struct YuvTileParams {
    pub tile_width: u32,
    pub tile_height: u32,
    pub corner_radius: f32,
    pub stretch_to_fill: bool,
    pub flip_horizontal: bool,
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
    /// Accepts raw slices and dimensions directly to avoid intermediate copies.
    fn upload_yuv(
        &mut self,
        participant_id: u64,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        frame: &YuvFrameData,
        tile: &YuvTileParams,
    ) {
        if frame.width == 0 || frame.height == 0 {
            return;
        }

        let state = self.ensure_textures(participant_id, device, frame.width, frame.height);

        let y_tex_w = align_to(frame.width, 256);
        let uv_tex_w = align_to(frame.width / 2, 256);
        let uv_h = frame.height / 2;

        // Data is already padded to GPU-aligned strides in VideoBuffer,
        // upload directly — no intermediate copy needed.
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &state.y_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            frame.y,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(y_tex_w),
                rows_per_image: Some(frame.height),
            },
            wgpu::Extent3d {
                width: y_tex_w,
                height: frame.height,
                depth_or_array_layers: 1,
            },
        );

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &state.u_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            frame.u,
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

        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &state.v_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            frame.v,
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

        // Update params uniform.
        // In stretch-to-fill mode, force aspect terms to match the source aspect so
        // crop math becomes a no-op even if the shader flag path is not taken.
        let (tile_aspect_num, tile_aspect_den) = if tile.stretch_to_fill {
            (frame.width.max(1), frame.height.max(1))
        } else {
            (
                (tile.tile_width as f32 * 1000.0) as u32,
                (tile.tile_height.max(1) as f32 * 1000.0) as u32,
            )
        };

        let params = Params {
            src_w: frame.width,
            src_h: frame.height,
            y_tex_w,
            uv_tex_w,
            tile_aspect_num,
            tile_aspect_den,
            tile_w: tile.tile_width as f32,
            tile_h: tile.tile_height as f32,
            corner_radius: tile.corner_radius,
            stretch_to_fill: u32::from(tile.stretch_to_fill),
            flip_horizontal: u32::from(tile.flip_horizontal),
            _pad: [0; 2],
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

/// YUV frame reference ready for GPU upload and rendering.
///
/// Created each frame by `YuvVideoProgram::draw()`. Holds an `Arc` to the
/// double-buffered video manager so `prepare()` can lock the read buffer and
/// upload directly to the GPU — avoiding a ~3 MB clone per frame.
pub struct YuvVideoPrimitive {
    participant_id: u64,
    /// Reference to the double-buffered video manager (zero-copy path).
    buffer: Arc<VideoBufferManager>,
    tile_width: u32,
    tile_height: u32,
    corner_radius: f32,
    /// When true, skip aspect-ratio crop and stretch the source to fill the tile.
    stretch_to_fill: bool,
    /// When true, flip horizontally for local camera mirror preview.
    mirror: bool,
    /// When true, skip GPU texture upload (frame hasn't changed).
    skip_upload: bool,
}

impl std::fmt::Debug for YuvVideoPrimitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("YuvVideoPrimitive")
            .field("participant_id", &self.participant_id)
            .field("skip_upload", &self.skip_upload)
            .finish()
    }
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
        if self.skip_upload && pipeline.participants.contains_key(&self.participant_id) {
            return;
        }

        // Lock the read buffer directly and upload to GPU — no intermediate clone.
        let frame_lock = self.buffer.latest_frame();
        let buf = frame_lock.lock().unwrap();

        if buf.width == 0 || buf.height == 0 {
            return;
        }

        pipeline.upload_yuv(
            self.participant_id,
            device,
            queue,
            &YuvFrameData {
                width: buf.width,
                height: buf.height,
                y: &buf.y,
                u: &buf.u,
                v: &buf.v,
            },
            &YuvTileParams {
                tile_width: self.tile_width,
                tile_height: self.tile_height,
                corner_radius: self.corner_radius,
                stretch_to_fill: self.stretch_to_fill,
                flip_horizontal: self.mirror,
            },
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
/// instance, which reads from the participant's `VideoBufferManager`.
pub struct YuvVideoProgram {
    pub participant_id: u64,
    pub buffer: Arc<VideoBufferManager>,
    pub corner_radius: f32,
    /// When true, skip aspect-ratio crop and stretch the source to fill the tile.
    pub stretch_to_fill: bool,
    /// When true, flip horizontally for local camera mirror preview.
    pub mirror: bool,
    /// When true, skip GPU texture upload (frame hasn't changed).
    pub skip_upload: bool,
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
        YuvVideoPrimitive {
            participant_id: self.participant_id,
            buffer: self.buffer.clone(),
            tile_width: bounds.width.max(1.0) as u32,
            tile_height: bounds.height.max(1.0) as u32,
            corner_radius: self.corner_radius,
            stretch_to_fill: self.stretch_to_fill,
            mirror: self.mirror,
            skip_upload: self.skip_upload,
        }
    }
}
