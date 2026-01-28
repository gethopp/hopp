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
use winit::monitor::MonitorHandle;
use winit::window::Window;

#[path = "overlay_context.rs"]
pub mod overlay_context;
use overlay_context::OverlayContext;

#[path = "cursor.rs"]
pub mod cursor;
use cursor::{Cursor, CursorsRenderer};

#[path = "click_animation.rs"]
pub mod click_animation;
use click_animation::ClickAnimationRenderer;

#[path = "point.rs"]
pub mod point;

#[path = "iced_renderer.rs"]
pub mod iced_renderer;
use iced_renderer::IcedRenderer;

#[path = "draw.rs"]
pub mod draw;

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
/// - Click animation rendering via `ClickAnimationRenderer`
/// - Drawing/iced rendering via `IcedRenderer`
///
/// # Lifetime
///
/// The lifetime parameter `'a` represents the lifetime of the underlying window
/// surfaces, ensuring memory safety when windows are destroyed.
#[derive(Debug)]
pub struct GraphicsContext<'a> {
    /// wgpu instance for creating surfaces
    instance: wgpu::Instance,
    /// wgpu adapter for device creation
    adapter: wgpu::Adapter,
    /// GPU logical device for creating resources and submitting commands
    device: wgpu::Device,
    /// Command queue for submitting GPU operations
    queue: wgpu::Queue,
    /// Surface format used for all overlays
    surface_format: wgpu::TextureFormat,
    /// Path to texture resources
    texture_path: String,
    /// Pre-created overlay windows for all displays
    overlay_contexts: Vec<OverlayContext<'a>>,
    /// Index of currently active overlay (if any)
    active_overlay_index: Option<usize>,
    /// Renderer for cursor graphics with multi-cursor support (created at init)
    cursor_renderer: CursorsRenderer,
    /// Renderer for click animations (created when show_overlay is called)
    click_animation_renderer: Option<ClickAnimationRenderer>,
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
    /// * `windows` - Pre-created and positioned overlay windows
    /// * `texture_path` - Base directory path for loading texture resources
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
    ///
    /// # Platform-Specific Behavior
    ///
    /// - **Windows**: Initializes DirectComposition for transparent overlay rendering
    pub fn new(windows: Vec<Arc<Window>>, texture_path: String) -> OverlayResult<Self> {
        log::info!("GraphicsContext::new with {} windows", windows.len());

        if windows.is_empty() {
            log::error!("GraphicsContext::new: No windows provided");
            return Err(OverlayError::WindowCreationError);
        }

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        let mut overlay_surfaces = Vec::new();

        for window in windows {
            match overlay_context::create_surface(window, &instance) {
                Ok(overlay_surface) => {
                    overlay_surfaces.push(overlay_surface);
                }
                Err(e) => {
                    log::error!("GraphicsContext::new: Failed to create surface: {}", e);
                }
            }
        }

        if overlay_surfaces.is_empty() {
            log::error!("GraphicsContext::new: Failed to create any surfaces");
            return Err(OverlayError::SurfaceCreationError);
        }

        // Find an adapter that supports the first surface
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&overlay_surfaces[0].surface),
            force_fallback_adapter: false,
        }))
        .map_err(|e| {
            log::error!("GraphicsContext::new: request_adapter failed: {e:?}");
            OverlayError::AdapterRequestError
        })?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            label: None,
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::default(),
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
        }))
        .map_err(|_| OverlayError::DeviceRequestError)?;

        // Use a common surface format (Bgra8UnormSrgb is widely supported)
        // TODO: get this from the surfces
        let surface_format = wgpu::TextureFormat::Bgra8UnormSrgb;

        // Create OverlayContexts
        let mut overlay_contexts = Vec::new();
        for (index, overlay_surface) in overlay_surfaces.into_iter().enumerate() {
            match OverlayContext::new(
                overlay_surface,
                &device,
                &queue,
                surface_format,
                &adapter,
                &texture_path,
            ) {
                Ok(context) => {
                    overlay_contexts.push(context);
                }
                Err(e) => {
                    log::error!(
                        "GraphicsContext::new: Failed to create overlay context {}: {}",
                        index,
                        e
                    );
                }
            }
        }

        if overlay_contexts.is_empty() {
            log::error!("GraphicsContext::new: No overlay contexts created");
            return Err(OverlayError::SurfaceCreationError);
        }

        // Configure each surface
        for overlay_context in &overlay_contexts {
            overlay_context
                .configure_surface(&device, &adapter, surface_format)
                .map_err(|e| {
                    log::error!("GraphicsContext::new: Failed to configure surface: {}", e);
                    OverlayError::SurfaceCreationError
                })?;
        }

        // Initialize cursor renderer only (others are deferred until show_overlay)
        let cursor_renderer = CursorsRenderer::create(&device, surface_format);

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            surface_format,
            texture_path,
            overlay_contexts,
            active_overlay_index: None,
            cursor_renderer,
            click_animation_renderer: None,
        })
    }

    /// Finds an adapter that is compatible with all surfaces.
    fn find_compatible_adapter(
        instance: &wgpu::Instance,
        overlay_contexts: &[OverlayContext],
    ) -> OverlayResult<wgpu::Adapter> {
        log::info!(
            "find_compatible_adapter: checking {} surfaces",
            overlay_contexts.len()
        );

        // Try to find an adapter compatible with the first surface
        let first_surface = &overlay_contexts[0].surface;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(first_surface),
            force_fallback_adapter: false,
        }));

        match adapter {
            Ok(adapter) => {
                // Verify this adapter supports all other surfaces
                for (i, ctx) in overlay_contexts.iter().enumerate().skip(1) {
                    if !adapter.is_surface_supported(&ctx.surface) {
                        log::warn!(
                            "find_compatible_adapter: adapter not compatible with surface {}",
                            i
                        );
                        // TODO For now, we'll still use this adapter and hope for the best
                        // A more robust solution would try other adapters
                    }
                }
                log::info!("find_compatible_adapter: found compatible adapter");
                Ok(adapter)
            }
            Err(e) => {
                log::error!("find_compatible_adapter: request_adapter failed: {e:?}");
                Err(OverlayError::AdapterRequestError)
            }
        }
    }

    /// Stub method for adding a new overlay window.
    ///
    /// This will be called by get_available_content() when a new display is detected.
    /// Currently empty - implementation to be added later.
    pub fn add_overlay_window(&mut self) {
        // Empty stub for now
        log::debug!("add_overlay_window: stub called");
    }

    /// Stub method for removing an outdated overlay window.
    ///
    /// This will be called by get_available_content() when a display is removed.
    /// Currently empty - implementation to be added later.
    pub fn remove_overlay_window(&mut self) {
        // Empty stub for now
        log::debug!("remove_overlay_window: stub called");
    }

    /// Shows the overlay that matches the monitor's position and sets it to fullscreen.
    ///
    /// This method finds the overlay whose window position matches the monitor's position,
    /// shows it, sets it to fullscreen, and creates the display-specific renderers.
    ///
    /// # Arguments
    ///
    /// * `monitor` - The monitor to match and fullscreen on
    ///
    /// # Returns
    ///
    /// Returns Ok(()) on success, or an error if no matching overlay is found or the operation fails.
    pub fn show_overlay_for_monitor(&mut self, monitor: &MonitorHandle) -> OverlayResult<()> {
        let index = self
            .find_overlay_by_monitor_position(&monitor)
            .ok_or_else(|| {
                log::error!("show_overlay_for_monitor: no matching overlay found");
                OverlayError::WindowCreationError
            })?;

        self.show_overlay(index, monitor)
    }

    /// Finds the overlay index whose window position matches the monitor's position.
    ///
    /// # Arguments
    ///
    /// * `monitor` - The monitor to match against
    ///
    /// # Returns
    ///
    /// Returns Some(index) if a matching overlay is found, None otherwise.
    pub fn find_overlay_by_monitor_position(&self, monitor: &MonitorHandle) -> Option<usize> {
        let monitor_pos = monitor.position();
        log::info!(
            "find_overlay_by_monitor_position: looking for monitor at ({}, {})",
            monitor_pos.x,
            monitor_pos.y
        );

        for (i, ctx) in self.overlay_contexts.iter().enumerate() {
            if let Ok(win_pos) = ctx.window.outer_position() {
                log::debug!(
                    "find_overlay_by_monitor_position: overlay {} at ({}, {})",
                    i,
                    win_pos.x,
                    win_pos.y
                );
                // Compare positions (allow small tolerance for scaling differences)
                if (win_pos.x - monitor_pos.x).abs() < 10 && (win_pos.y - monitor_pos.y).abs() < 100
                {
                    log::info!(
                        "find_overlay_by_monitor_position: found match at index {}",
                        i
                    );
                    return Some(i);
                }
            }
        }
        log::warn!("find_overlay_by_monitor_position: no matching overlay found");
        None
    }

    /// Shows the overlay at the given index and sets it to fullscreen.
    ///
    /// This method also creates the `click_animation_renderer` and `iced_renderer`
    /// using the monitor's scale factor.
    ///
    /// # Arguments
    ///
    /// * `index` - The index of the overlay to show
    /// * `monitor` - The monitor to fullscreen on
    ///
    /// # Returns
    ///
    /// Returns Ok(()) on success, or an error if the operation fails.
    pub fn show_overlay(&mut self, index: usize, monitor: &MonitorHandle) -> OverlayResult<()> {
        log::info!("show_overlay: index={}, monitor={:?}", index, monitor);

        if index >= self.overlay_contexts.len() {
            log::error!("show_overlay: index {} out of bounds", index);
            return Err(OverlayError::WindowCreationError);
        }

        let overlay_context = &self.overlay_contexts[index];
        overlay_context
            .show_fullscreen(monitor.clone())
            .map_err(|e| {
                log::error!("show_overlay: show_fullscreen failed: {}", e);
                OverlayError::WindowCreationError
            })?;

        // Reconfigure the surface after fullscreen
        overlay_context
            .configure_surface(&self.device, &self.adapter, self.surface_format)
            .map_err(|e| {
                log::error!("show_overlay: configure_surface failed: {}", e);
                OverlayError::SurfaceCreationError
            })?;

        self.active_overlay_index = Some(index);

        // Create the display-specific renderers
        let window = &overlay_context.window;
        let size = window.inner_size();
        let scale = monitor.scale_factor();

        let click_animation_renderer = ClickAnimationRenderer::create(
            &self.device,
            &self.queue,
            self.surface_format,
            &self.texture_path,
            Extent {
                width: size.width as f64,
                height: size.height as f64,
            },
            scale,
        )?;
        self.click_animation_renderer = Some(click_animation_renderer);

        log::info!("show_overlay: renderers created successfully");
        Ok(())
    }

    /// Hides the currently active overlay and clears the display renderers.
    pub fn hide_active_overlay(&mut self) {
        log::info!("hide_active_overlay");

        if let Some(index) = self.active_overlay_index {
            if let Some(overlay_context) = self.overlay_contexts.get(index) {
                overlay_context.hide();
            }
        }

        self.active_overlay_index = None;
        self.clear_display_renderers();
    }

    /// Clears the display-specific renderers.
    ///
    /// Sets `click_animation_renderer` and `iced_renderer` to None.
    pub fn clear_display_renderers(&mut self) {
        log::info!("clear_display_renderers");
        self.click_animation_renderer = None;
    }

    /// Returns a reference to the currently active window.
    ///
    /// # Returns
    ///
    /// Some(&Arc<Window>) if there's an active overlay, None otherwise.
    pub fn get_active_window(&self) -> Option<&Arc<Window>> {
        self.active_overlay_index
            .and_then(|index| self.overlay_contexts.get(index))
            .map(|ctx| &ctx.window)
    }

    /// Returns a reference to the currently active surface.
    fn get_active_surface(&self) -> Option<&wgpu::Surface<'a>> {
        self.active_overlay_index
            .and_then(|index| self.overlay_contexts.get(index))
            .map(|ctx| &ctx.surface)
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
        let window = self.get_active_window().ok_or_else(|| {
            log::error!("create_cursor: no active window");
            OverlayError::WindowCreationError
        })?;
        let window_size = window.inner_size();
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
        let surface = match self.get_active_surface() {
            Some(s) => s,
            None => {
                log::warn!("GraphicsContext::draw: no active surface");
                return;
            }
        };

        let output = match surface.get_current_texture() {
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
        });
        render_pass.set_pipeline(&self.cursor_renderer.render_pipeline);

        cursor_controller.draw(&mut render_pass, self);

        if let Some(ref mut click_animation_renderer) = self.click_animation_renderer {
            click_animation_renderer.draw(&mut render_pass, &self.queue);
        }
        drop(render_pass);

        self.queue.submit(std::iter::once(encoder.finish()));

        if let Some(index) = self.active_overlay_index {
            if let Some(overlay_context) = self.overlay_contexts.get_mut(index) {
                overlay_context.iced_renderer.draw(&output, &view);
            }
        }

        if let Some(window) = self.get_active_window() {
            window.pre_present_notify();
        }

        output.present();
    }

    /// Returns a reference to the underlying active overlay window.
    ///
    /// # Returns
    ///
    /// Some(&Window) if there's an active overlay, None otherwise.
    pub fn window(&self) -> Option<&Window> {
        self.get_active_window().map(|w| w.as_ref())
    }

    /// Requests to enable a click animation at the specified position.
    ///
    /// # Arguments
    /// * `position` - Screen position where the animation should appear
    pub fn enable_click_animation(&mut self, position: Position) {
        log::debug!("GraphicsContext::enable_click_animation: {position:?}");
        if let Some(ref mut click_animation_renderer) = self.click_animation_renderer {
            click_animation_renderer.enable_click_animation(position);
        } else {
            log::warn!("enable_click_animation: click_animation_renderer not initialized");
        }
    }

    /// Adds a new participant to the draw manager with their color.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant
    /// * `color` - Hex color string for the participant's drawings
    pub fn add_draw_participant(&mut self, sid: String, color: &str) {
        if let Some(index) = self.active_overlay_index {
            if let Some(overlay_context) = self.overlay_contexts.get_mut(index) {
                overlay_context
                    .iced_renderer
                    .add_draw_participant(sid, color);
            }
        } else {
            log::warn!("add_draw_participant: iced_renderer not initialized");
        }
    }

    /// Removes a participant from the draw manager.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant to remove
    pub fn remove_draw_participant(&mut self, sid: &str) {
        if let Some(index) = self.active_overlay_index {
            if let Some(overlay_context) = self.overlay_contexts.get_mut(index) {
                overlay_context.iced_renderer.remove_draw_participant(sid);
            }
        }
    }

    /// Sets the drawing mode for a specific participant.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant
    /// * `mode` - The drawing mode to set
    pub fn set_drawing_mode(&mut self, sid: &str, mode: crate::room_service::DrawingMode) {
        if let Some(index) = self.active_overlay_index {
            if let Some(overlay_context) = self.overlay_contexts.get_mut(index) {
                overlay_context.iced_renderer.set_drawing_mode(sid, mode);
            }
        }
    }

    /// Starts a new drawing path for a participant.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant
    /// * `point` - Starting point of the path
    /// * `path_id` - Unique identifier for the drawing path
    pub fn draw_start(&mut self, sid: &str, point: Position, path_id: u64) {
        if let Some(index) = self.active_overlay_index {
            if let Some(overlay_context) = self.overlay_contexts.get_mut(index) {
                overlay_context
                    .iced_renderer
                    .draw_start(sid, point, path_id);
            }
        }
    }

    /// Adds a point to the current drawing path for a participant.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant
    /// * `point` - Point to add to the current path
    pub fn draw_add_point(&mut self, sid: &str, point: Position) {
        if let Some(index) = self.active_overlay_index {
            if let Some(overlay_context) = self.overlay_contexts.get_mut(index) {
                overlay_context.iced_renderer.draw_add_point(sid, point);
            }
        }
    }

    /// Ends the current drawing path for a participant.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant
    /// * `point` - Final point of the path
    pub fn draw_end(&mut self, sid: &str, point: Position) {
        if let Some(index) = self.active_overlay_index {
            if let Some(overlay_context) = self.overlay_contexts.get_mut(index) {
                overlay_context.iced_renderer.draw_end(sid, point);
            }
        }
    }

    /// Clears a specific drawing path for a participant.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant
    /// * `path_id` - Unique identifier for the drawing path to clear
    pub fn draw_clear_path(&mut self, sid: &str, path_id: u64) {
        if let Some(index) = self.active_overlay_index {
            if let Some(overlay_context) = self.overlay_contexts.get_mut(index) {
                overlay_context.iced_renderer.draw_clear_path(sid, path_id);
            }
        }
    }

    /// Clears all drawing paths for a participant.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant
    pub fn draw_clear_all_paths(&mut self, sid: &str) {
        if let Some(index) = self.active_overlay_index {
            if let Some(overlay_context) = self.overlay_contexts.get_mut(index) {
                overlay_context.iced_renderer.draw_clear_all_paths(sid);
            }
        }
    }
}

impl Drop for GraphicsContext<'_> {
    fn drop(&mut self) {
        // This is needed for windows, because otherwise the title bar becomes
        // visible when a new overlay surface is created.
        // Minimize all overlay windows
        for ctx in &self.overlay_contexts {
            ctx.window.set_minimized(true);
        }
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
        mipmap_filter: wgpu::FilterMode::Nearest,
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
