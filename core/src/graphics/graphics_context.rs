//! Graphics context and rendering system for overlay windows.
//!
//! This module provides the core graphics infrastructure for rendering overlay elements
//! such as cursors and markers on top of shared screen content. It uses wgpu for
//! hardware-accelerated rendering with proper alpha blending and transparent window support.

use crate::utils::clock::Clock;
use crate::utils::geometry::Position;
use crate::utils::svg_renderer::SvgRenderError;
use crate::UserEvent;
use std::sync::{
    mpsc::{Receiver, Sender},
    Arc,
};
use std::thread::JoinHandle;
use std::time::Instant;
use thiserror::Error;
use winit::event_loop::EventLoopProxy;
use winit::window::Window;

#[cfg(target_os = "windows")]
use super::direct_composition::DirectComposition;

#[path = "click_animation.rs"]
pub mod click_animation;
use click_animation::ClickAnimationRenderer;

#[path = "iced_renderer.rs"]
pub mod iced_renderer;
use iced_renderer::IcedRenderer;

#[path = "participant.rs"]
pub mod participant;
use participant::ParticipantsManager;

pub(crate) enum RedrawThreadCommands {
    Activity,
    Stop,
}

fn redraw_thread(
    event_loop_proxy: EventLoopProxy<UserEvent>,
    receiver: Receiver<RedrawThreadCommands>,
) {
    let redraw_interval = std::time::Duration::from_millis(16);
    let inactivity_timeout = std::time::Duration::from_secs(15);
    let mut last_activity_time = Instant::now();

    loop {
        // Check for messages with a timeout equal to the redraw interval
        match receiver.recv_timeout(redraw_interval) {
            Ok(command) => match command {
                RedrawThreadCommands::Stop => break,
                RedrawThreadCommands::Activity => {
                    if last_activity_time.elapsed() < redraw_interval {
                        continue;
                    }
                    last_activity_time = Instant::now();
                }
            },
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                log::error!("redraw_thread: channel disconnected");
                break;
            }
        }

        // Check if we should stop due to inactivity
        if last_activity_time.elapsed() > inactivity_timeout {
            log::debug!("redraw_thread: stopping due to inactivity");
            continue;
        }

        // Send redraw event every 16ms
        if let Err(e) = event_loop_proxy.send_event(UserEvent::RequestRedraw) {
            log::error!("redraw_thread: error sending redraw event: {e:?}");
        }
    }
}

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

    /// Maximum number of participants reached.
    #[error("Maximum number of participants reached")]
    MaxParticipantsReached,
}

/// Type alias for Results in overlay graphics operations.
///
/// This is a convenience type that defaults to `()` for the success type,
/// making error handling more ergonomic throughout the graphics module.
/// Most graphics operations either succeed completely or fail with an `OverlayError`.
pub type OverlayResult<T = ()> = std::result::Result<T, OverlayError>;

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
/// - Click animation rendering
/// - Iced-based participant cursors and drawings
///
/// # Lifetime
///
/// The lifetime parameter `'a` represents the lifetime of the underlying window
/// surface, ensuring memory safety when the window is destroyed.
#[derive(Debug)]
pub struct GraphicsContext<'a> {
    /// wgpu surface for rendering to the window
    surface: wgpu::Surface<'a>,
    /// GPU logical device — kept alive for wgpu resource lifetime
    _device: wgpu::Device,
    /// Command queue — kept alive for wgpu resource lifetime
    _queue: wgpu::Queue,
    /// Reference to the overlay window
    window: Arc<Window>,

    /// Windows-specific DirectComposition integration for transparent overlays
    #[cfg(target_os = "windows")]
    _direct_composition: DirectComposition,

    /// Renderer for click animations
    click_animation_renderer: ClickAnimationRenderer,

    /// Renderer for iced graphics
    iced_renderer: IcedRenderer,

    /// Manager for participant state (drawings and cursors)
    participants_manager: ParticipantsManager,

    /// Thread that controls rendering cadence
    redraw_thread: Option<JoinHandle<()>>,
    /// Sender for triggering redraws and animations
    redraw_thread_sender: Sender<RedrawThreadCommands>,
    /// Clock for time tracking
    clock: Arc<dyn Clock>,
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
    pub fn new(
        window_arc: Arc<Window>,
        texture_path: String,
        scale: f64,
        event_loop_proxy: EventLoopProxy<UserEvent>,
    ) -> OverlayResult<Self> {
        Self::with_clock(
            window_arc,
            texture_path,
            scale,
            event_loop_proxy,
            crate::utils::clock::default_clock(),
        )
    }

    /// Creates a new graphics context with a custom clock (for testing).
    pub fn with_clock(
        window_arc: Arc<Window>,
        texture_path: String,
        scale: f64,
        event_loop_proxy: EventLoopProxy<UserEvent>,
        clock: Arc<dyn Clock>,
    ) -> OverlayResult<Self> {
        log::info!("GraphicsContext::new");
        let size = window_arc.inner_size();
        log::info!("GraphicsContext::new: window size: {size:?}, scale: {scale}");
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
        });

        #[cfg(target_os = "windows")]
        let direct_composition =
            DirectComposition::new(window_arc.clone()).ok_or(OverlayError::SurfaceCreationError)?;

        let surface = {
            #[cfg(target_os = "windows")]
            {
                direct_composition.create_surface(&instance)?
            }
            #[cfg(target_os = "macos")]
            {
                instance.create_surface(window_arc.clone()).map_err(|e| {
                    log::error!("GraphicsContext::new: {e:?}");
                    OverlayError::SurfaceCreationError
                })?
            }
            // Add other OS targets here if needed
            #[cfg(not(any(target_os = "windows", target_os = "macos")))]
            {
                // Default or error for unsupported OS
                instance.create_surface(window_arc.clone()).map_err(|e| {
                    log::error!("GraphicsContext::new: {:?}", e);
                    OverlayError::SurfaceCreationError
                })?
            }
        };

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
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
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

        #[cfg(target_os = "windows")]
        direct_composition.commit()?;

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

        let click_animation_renderer = ClickAnimationRenderer::new(clock.clone());

        let iced_renderer = IcedRenderer::new(
            &device,
            &queue,
            surface_config.format,
            &adapter,
            &window_arc,
            &texture_path,
        );

        let (sender, receiver) = std::sync::mpsc::channel();
        let redraw_thread = Some(std::thread::spawn(move || {
            redraw_thread(event_loop_proxy, receiver);
        }));

        Ok(Self {
            surface,
            _device: device,
            _queue: queue,
            window: window_arc,
            #[cfg(target_os = "windows")]
            _direct_composition: direct_composition,
            click_animation_renderer,
            iced_renderer,
            participants_manager: ParticipantsManager::default(),
            redraw_thread,
            redraw_thread_sender: sender,
            clock,
        })
    }

    /// Returns a clone of the redraw thread sender for use by subsystems.
    ///
    /// This allows other components (like CursorController and CursorWrapper)
    /// to trigger redraws by sending commands to the redraw thread.
    pub(crate) fn redraw_sender(&self) -> Sender<RedrawThreadCommands> {
        self.redraw_thread_sender.clone()
    }

    /// Returns a clone of the clock for use by subsystems.
    ///
    /// This allows other components (like CursorController) to use the same
    /// clock for time-dependent logic.
    pub fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    /// Triggers rendering activity.
    ///
    /// Signals the redraw thread to continue rendering and resets the inactivity timer.
    pub fn trigger_render(&self) {
        if let Err(e) = self
            .redraw_thread_sender
            .send(RedrawThreadCommands::Activity)
        {
            log::error!("GraphicsContext::trigger_render: error sending activity event: {e:?}");
        }
    }

    /// Triggers a click animation at the given position.
    ///
    /// Enables the click animation renderer state and signals rendering activity.
    pub fn trigger_click_animation(&mut self, position: Position) {
        log::debug!("GraphicsContext::trigger_click_animation: {position:?}");
        self.click_animation_renderer
            .enable_click_animation(position);
        if let Err(e) = self
            .redraw_thread_sender
            .send(RedrawThreadCommands::Activity)
        {
            log::error!("GraphicsContext::trigger_click_animation: error: {e:?}");
        }
    }

    /// Renders the current frame with all overlay elements.
    ///
    /// This method performs a complete render pass for the overlay, drawing all
    /// active cursors and corner markers to the window surface.
    ///
    /// # Arguments
    ///
    /// # Rendering Pipeline
    ///
    /// The draw operation follows this sequence:
    /// 1. Acquire the current frame buffer from the surface
    /// 2. Clear the frame buffer with transparent black (0,0,0,0)
    /// 3. Render click animations
    /// 4. Render iced elements (participant cursors and drawings)
    /// 5. Submit commands to GPU and present the frame
    ///
    /// # Error Handling
    ///
    /// If frame acquisition fails (e.g., surface lost), the method logs the error
    /// and returns early without crashing. This provides resilience against
    /// temporary graphics driver issues or window state changes.
    pub fn draw(&mut self) {
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

        self.click_animation_renderer.update();

        self.iced_renderer.draw(
            &output,
            &view,
            &self.participants_manager,
            &self.click_animation_renderer,
        );

        self.window.pre_present_notify();

        output.present();
    }

    /// Returns a mutable reference to the participants manager for cursor updates.
    pub fn participants_manager_mut(&mut self) -> &mut ParticipantsManager {
        &mut self.participants_manager
    }

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

    /// Adds a new participant with automatic color assignment.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant
    /// * `name` - Full name of the participant (will be made unique)
    /// * `auto_clear` - Whether to automatically clear paths after 3 seconds (for local participant)
    ///
    /// # Returns
    /// * `Ok(())` - Participant added successfully
    /// * `Err(OverlayError)` - Failed to add participant (e.g., no colors available)
    pub fn add_participant(
        &mut self,
        sid: String,
        name: &str,
        auto_clear: bool,
    ) -> Result<(), SvgRenderError> {
        self.participants_manager
            .add_participant(sid, name, auto_clear)
    }

    /// Removes a participant.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant to remove
    pub fn remove_participant(&mut self, sid: &str) {
        self.participants_manager.remove_participant(sid);
    }

    /// Sets the drawing mode for a specific participant.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant
    /// * `mode` - The drawing mode to set
    pub fn set_drawing_mode(&mut self, sid: &str, mode: crate::room_service::DrawingMode) {
        self.participants_manager.set_drawing_mode(sid, mode);
    }

    /// Starts a new drawing path for a participant.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant
    /// * `point` - Starting point of the path
    /// * `path_id` - Unique identifier for the drawing path
    pub fn draw_start(&mut self, sid: &str, point: Position, path_id: u64) {
        self.participants_manager.draw_start(sid, point, path_id);
    }

    /// Adds a point to the current drawing path for a participant.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant
    /// * `point` - Point to add to the current path
    pub fn draw_add_point(&mut self, sid: &str, point: Position) {
        self.participants_manager.draw_add_point(sid, point);
    }

    /// Ends the current drawing path for a participant.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant
    /// * `point` - Final point of the path
    pub fn draw_end(&mut self, sid: &str, point: Position) {
        self.participants_manager.draw_end(sid, point);
    }

    /// Clears a specific drawing path for a participant.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant
    /// * `path_id` - Unique identifier for the drawing path to clear
    pub fn draw_clear_path(&mut self, sid: &str, path_id: u64) {
        self.participants_manager.draw_clear_path(sid, path_id);
    }

    /// Clears all drawing paths for a participant.
    ///
    /// # Arguments
    /// * `sid` - Session ID identifying the participant
    pub fn draw_clear_all_paths(&mut self, sid: &str) {
        self.participants_manager.draw_clear_all_paths(sid);
    }

    /// Updates auto-clear for all participants and returns removed path IDs.
    ///
    /// # Returns
    /// A vector of removed path IDs
    pub fn update_auto_clear(&mut self) -> Vec<u64> {
        self.participants_manager.update_auto_clear()
    }
}

impl Drop for GraphicsContext<'_> {
    fn drop(&mut self) {
        // Stop the redraw thread
        if let Some(handle) = self.redraw_thread.take() {
            let _ = self.redraw_thread_sender.send(RedrawThreadCommands::Stop);
            let _ = handle.join();
        }
        // This is needed for windows, because otherwise the title bar becomes
        // visible when a new overlay surface is created.
        self.window.set_minimized(true);
    }
}
