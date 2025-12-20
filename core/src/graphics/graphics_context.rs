//! Graphics context and rendering system for overlay windows.
//!
//! This module provides the core graphics infrastructure for rendering overlay elements
//! such as cursors and markers on top of shared screen content. It uses wgpu for
//! hardware-accelerated rendering with proper alpha blending and transparent window support.

use crate::utils::geometry::Extent;
use crate::{input::mouse::CursorController, utils::geometry::Position};
use image::GenericImageView;
use log::error;
use std::sync::Arc;
use thiserror::Error;
use wgpu::ExperimentalFeatures;
use winit::window::Window;

#[path = "marker.rs"]
mod marker;
use marker::MarkerRenderer;

#[path = "cursor.rs"]
pub mod cursor;
use cursor::{Cursor, CursorsRenderer};

#[path = "click_animation.rs"]
pub mod click_animation;
use click_animation::ClickAnimationRenderer;

#[path = "point.rs"]
pub mod point;

/// Errors that can occur during overlay graphics operations.
#[derive(Error, Debug)]
pub enum OverlayError {
    /// Failed to create the overlay window.
    #[error("Failed to create overlay window")]
    WindowCreationError,

    /// Failed to create a graphics surface for rendering.
    #[error("Failed to create graphics surface for rendering")]
    SurfaceCreationError,

    /// Failed to request a graphics adapter from the system.
    #[error("Failed to request graphics adapter")]
    AdapterRequestError,

    /// Failed to request a graphics device from the adapter.
    #[error("Failed to request graphics device")]
    DeviceRequestError,

    /// Failed to create or load a texture resource.
    #[error("Failed to create or load texture resource")]
    TextureCreationError,
}

/// Type alias for Results in overlay graphics operations.
///
/// This is a convenience type that defaults to `()` for the success type,
/// making error handling more ergonomic throughout the graphics module.
/// Most graphics operations either succeed completely or fail with an `OverlayError`.
pub type OverlayResult<T = ()> = std::result::Result<T, OverlayError>;

/// Internal texture representation for overlay graphics.
///
/// This struct encapsulates a GPU texture resource along with its metadata
/// and binding information. It stores both the texture's dimensions and the
/// wgpu bind group needed for shader access during rendering.
#[derive(Debug)]
struct Texture {
    /// Dimensions of the texture in pixels (width, height)
    extent: Extent,
    /// wgpu bind group containing texture and sampler resources for shader access
    bind_group: wgpu::BindGroup,
}

/// Vertex data structure for overlay geometry rendering.
///
/// This struct represents a single vertex in the graphics pipeline, containing
/// both position and texture coordinate information. It's designed to be
/// directly uploaded to GPU vertex buffers for efficient rendering.
///
/// # Memory Layout
///
/// The struct uses `#[repr(C)]` to ensure consistent memory layout across
/// platforms, making it safe for direct GPU buffer uploads via bytemuck.
#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    /// 2D position in clip space coordinates (range: -1.0 to 1.0)
    position: [f32; 2],
    /// 2D texture coordinates for sampling (range: 0.0 to 1.0)
    texture_coords: [f32; 2],
}

/// Core graphics context for overlay rendering operations.
///
/// `GraphicsContext` encapsulates all the necessary GPU resources and state required
/// for rendering overlay graphics, including cursors and markers. It manages the
/// wgpu rendering pipeline, surface configuration, and coordinate transformations
/// for overlay windows.
///
/// # Platform Support
///
/// The context supports multiple platforms with platform-specific optimizations:
/// - **Windows**: Uses DirectComposition for transparent overlay rendering
/// - **macOS**: Uses standard Core Graphics surface creation
///
/// # Rendering Pipeline
///
/// The graphics context maintains separate renderers for different overlay elements:
/// - Cursor rendering via `CursorsRenderer` for multiple simultaneous cursors
/// - Marker rendering via `MarkerRenderer` for corner boundary indicators
///
/// # Lifetime
///
/// The lifetime parameter `'a` represents the lifetime of the underlying window
/// surface, ensuring memory safety when the window is destroyed.
#[derive(Debug)]
pub struct GraphicsContext<'a> {
    /// wgpu surface for rendering to the window
    surface: wgpu::Surface<'a>,
    /// GPU logical device for creating resources and submitting commands
    device: wgpu::Device,
    /// Command queue for submitting GPU operations
    queue: wgpu::Queue,
    /// Reference to the overlay window
    window: Arc<Window>,
    /// Renderer for cursor graphics with multi-cursor support
    cursor_renderer: CursorsRenderer,

    /// Renderer for corner markers indicating overlay boundaries
    marker_renderer: MarkerRenderer,

    /// Renderer for click animations
    click_animation_renderer: ClickAnimationRenderer,
}

impl<'a> GraphicsContext<'a> {
    /// Creates a new graphics context for overlay rendering.
    ///
    /// This method initializes all necessary GPU resources for overlay rendering,
    /// including surface creation, adapter/device initialization, and render pipeline setup.
    /// The process varies by platform to ensure optimal transparent overlay rendering.
    ///
    /// # Arguments
    ///
    /// * `window` - The overlay window to render to
    /// * `texture_path` - Base directory path for loading texture resources
    /// * `scale` - Display scale
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing the initialized `GraphicsContext` on success,
    /// or an `OverlayError` if any initialization step fails.
    ///
    /// # Errors
    ///
    /// This method can return several types of errors:
    /// - `OverlayError::SurfaceCreationError` - Failed to create rendering surface
    /// - `OverlayError::AdapterRequestError` - No suitable GPU adapter found
    /// - `OverlayError::DeviceRequestError` - Failed to create logical GPU device
    /// - `OverlayError::TextureCreationError` - Failed to initialize marker textures
    ///
    /// # Platform-Specific Behavior
    ///
    /// - **Windows**: Initializes DirectComposition for transparent overlay rendering
    pub fn new(window: Window, texture_path: String, scale: f64) -> OverlayResult<Self> {
        log::info!("GraphicsContext::new");
        let size = window.inner_size();
        let window_arc = Arc::new(window);

        let backend_options = wgpu::BackendOptions {
            dx12: wgpu::Dx12BackendOptions {
                presentation_system: wgpu::Dx12SwapchainKind::DxgiFromVisual,
                ..Default::default()
            },
            ..Default::default()
        };
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            backend_options: backend_options,
            ..Default::default()
        });

        let surface = instance.create_surface(window_arc.clone()).map_err(|e| {
            log::error!("GraphicsContext::new: {e:?}");
            OverlayError::SurfaceCreationError
        })?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }));
        if let Err(e) = adapter {
            log::error!("GraphicsContext::new request_adapter: {e:?}");
            return Err(OverlayError::AdapterRequestError);
        }
        let adapter = adapter.unwrap();

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            label: None,
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::default(),
            experimental_features: ExperimentalFeatures::disabled(),
        }))
        .map_err(|_| OverlayError::DeviceRequestError)?;

        let surface_capabilities = surface.get_capabilities(&adapter);

        let alpha_modes = surface_capabilities.alpha_modes;
        let surface_formats = surface_capabilities.formats;

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_formats[0],
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::AutoVsync, // This is using fifo or fifo_relaxed
            alpha_mode: alpha_modes
                .iter()
                .find(|mode| {
                    /*
                     * This is a workaround for windows, where we observed
                     * crashes with post multiplied alpha.
                     */
                    #[allow(unused_variables)]
                    let post_multiplied = mode == &&wgpu::CompositeAlphaMode::PostMultiplied;
                    #[cfg(target_os = "windows")]
                    let post_multiplied = false;
                    (mode != &&wgpu::CompositeAlphaMode::Opaque)
                        && ((mode == &&wgpu::CompositeAlphaMode::PreMultiplied) || post_multiplied)
                })
                .copied()
                .unwrap_or(alpha_modes[0]),
            view_formats: vec![],
            desired_maximum_frame_latency: 0,
        };
        surface.configure(&device, &surface_config);

        /*
         * Workaround for resetting the default white background
         * on transparent windows on windows.
         */
        #[cfg(target_os = "windows")]
        {
            window_arc.set_minimized(true);
            std::thread::sleep(std::time::Duration::from_millis(100));
            window_arc.set_minimized(false);
        }

        let cursor_renderer = CursorsRenderer::create(&device, surface_config.format);

        let marker_renderer = MarkerRenderer::new(
            &device,
            &queue,
            surface_config.format,
            &texture_path,
            Extent {
                width: size.width as f64,
                height: size.height as f64,
            },
            scale,
        )?;

        let click_animation_renderer = ClickAnimationRenderer::create(
            &device,
            &queue,
            surface_config.format,
            &texture_path,
            Extent {
                width: size.width as f64,
                height: size.height as f64,
            },
            scale,
        )?;

        Ok(Self {
            surface,
            device,
            queue,
            window: window_arc,
            cursor_renderer,
            marker_renderer,
            click_animation_renderer,
        })
    }

    /// Creates a new cursor with the specified image and scale factor.
    ///
    /// This method loads a cursor image from disk and creates all necessary GPU
    /// resources for rendering it as part of the overlay. The cursor maintains
    /// its original aspect ratio while being scaled appropriately for the target
    /// window size.
    ///
    /// # Arguments
    ///
    /// * `image_data` - Loaded image data
    /// * `display_scale` - Display scale
    ///
    /// # Returns
    ///
    /// Returns a `Result` containing the new `Cursor` instance on success,
    /// or an `OverlayError` if cursor creation fails.
    pub fn create_cursor(
        &mut self,
        image_data: &[u8],
        display_scale: f64,
    ) -> std::result::Result<Cursor, OverlayError> {
        let window_size = self.window.inner_size();
        self.cursor_renderer.create_cursor(
            image_data,
            display_scale,
            &self.device,
            &self.queue,
            Extent {
                width: window_size.width as f64,
                height: window_size.height as f64,
            },
        )
    }

    /// Renders the current frame with all overlay elements.
    ///
    /// This method performs a complete render pass for the overlay, drawing all
    /// active cursors and corner markers to the window surface.
    ///
    /// # Arguments
    ///
    /// * `cursor_controller` - Controller managing cursor state and rendering
    ///
    /// # Rendering Pipeline
    ///
    /// The draw operation follows this sequence:
    /// 1. Acquire the current frame buffer from the surface
    /// 2. Clear the frame buffer with transparent black (0,0,0,0)
    /// 3. Set up the cursor rendering pipeline
    /// 4. Render all active cursors via the cursor controller
    /// 5. Render corner markers for overlay boundaries
    /// 6. Submit commands to GPU and present the frame
    ///
    /// # Error Handling
    ///
    /// If frame acquisition fails (e.g., surface lost), the method logs the error
    /// and returns early without crashing. This provides resilience against
    /// temporary graphics driver issues or window state changes.
    pub fn draw(&mut self, cursor_controller: &CursorController) {
        let output = match self.surface.get_current_texture() {
            Ok(output) => output,
            Err(e) => {
                log::error!("GraphicsContext::draw: failed to get current texture: {e:?}");
                return;
            }
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cursor encoder"),
            });
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("cursor render pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
            multiview_mask: None,
        });
        render_pass.set_pipeline(&self.cursor_renderer.render_pipeline);

        cursor_controller.draw(&mut render_pass, self);

        self.marker_renderer.draw(&mut render_pass);
        self.click_animation_renderer
            .draw(&mut render_pass, &self.queue);

        drop(render_pass);

        self.queue.submit(std::iter::once(encoder.finish()));

        self.window.pre_present_notify();

        output.present();
    }

    /// Returns a reference to the underlying overlay window.
    ///
    /// # Returns
    ///
    /// A reference to the `Window` instance used for overlay rendering.
    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Requests to enable a click animation at the specified position.
    ///
    /// # Arguments
    /// * `position` - Screen position where the animation should appear
    pub fn enable_click_animation(&mut self, position: Position) {
        log::debug!("GraphicsContext::enable_click_animation: {position:?}");
        self.click_animation_renderer
            .enable_click_animation(position);
    }
}

impl Drop for GraphicsContext<'_> {
    fn drop(&mut self) {
        // This is needed for windows, because otherwise the title bar becomes
        // visible when a new overlay surface is created.
        self.window.set_minimized(true);
    }
}

/// Creates a GPU texture from an image file for overlay rendering.
///
/// This function loads an image from disk, uploads it to GPU memory, and creates
/// all necessary wgpu resources for texture rendering including samplers and
/// bind groups. The resulting texture is ready for use in overlay rendering pipelines.
///
/// # Arguments
///
/// * `device` - wgpu device for creating GPU resources
/// * `queue` - wgpu queue for uploading texture data to GPU
/// * `image_data` - Loaded image data
/// * `bind_group_layout` - wgpu bind group layout for the texture resources
///
/// # Returns
///
/// Returns a `Result` containing the created `Texture` on success, or an
/// `OverlayError::TextureCreationError` if any step of texture creation fails.
fn create_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    image_data: &[u8],
    bind_group_layout: &wgpu::BindGroupLayout,
) -> Result<Texture, OverlayError> {
    let diffuse_image = match image::load_from_memory(image_data) {
        Ok(image) => image,
        Err(_) => {
            error!("create_cursor_texture: failed to load image");
            return Err(OverlayError::TextureCreationError);
        }
    };

    let diffuse_rgba = diffuse_image.to_rgba8();

    let dimensions = diffuse_image.dimensions();
    let texture_size = wgpu::Extent3d {
        width: dimensions.0,
        height: dimensions.1,
        depth_or_array_layers: 1,
    };

    let diffuse_texture = device.create_texture(&wgpu::TextureDescriptor {
        size: texture_size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        label: Some("texture"),
        view_formats: &[],
    });

    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &diffuse_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &diffuse_rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4 * dimensions.0),
            rows_per_image: Some(dimensions.1),
        },
        texture_size,
    );

    let diffuse_texture_view = diffuse_texture.create_view(&wgpu::TextureViewDescriptor::default());
    let diffuse_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    });

    let diffuse_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("diffuse_bind_group"),
        layout: bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&diffuse_texture_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&diffuse_sampler),
            },
        ],
    });

    Ok(Texture {
        extent: Extent {
            width: dimensions.0 as f64,
            height: dimensions.1 as f64,
        },
        bind_group: diffuse_bind_group,
    })
}
