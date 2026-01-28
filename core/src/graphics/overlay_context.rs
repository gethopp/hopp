//! Per-display overlay window context management.
//!
//! This module provides the OverlayContext struct which encapsulates window and surface
//! resources for a single display. Windows are pre-created and hidden, then shown on-demand
//! when screen sharing starts.

use std::sync::Arc;
use thiserror::Error;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event_loop::ActiveEventLoop;
use winit::monitor::MonitorHandle;
use winit::window::{Window, WindowAttributes, WindowLevel};

use super::iced_renderer::IcedRenderer;

#[cfg(target_os = "windows")]
use winit::platform::windows::WindowExtWindows;

#[cfg(target_os = "macos")]
use winit::platform::macos::WindowExtMacOS;

#[cfg(target_os = "windows")]
use super::direct_composition::DirectComposition;

/// Surface data for an overlay window.
#[derive(Debug)]
pub struct OverlaySurface<'a> {
    /// wgpu surface for rendering to the window
    pub surface: wgpu::Surface<'a>,
    /// Reference to the overlay window
    pub window: Arc<Window>,
    /// Windows-specific DirectComposition integration for transparent overlays
    #[cfg(target_os = "windows")]
    pub direct_composition: DirectComposition,
}

/// Initial size for the overlay window (width and height in logical pixels)
const OVERLAY_WINDOW_INITIAL_SIZE: f64 = 1.0;

/// Errors that can occur during fullscreen operations.
#[derive(Error, Debug)]
enum FullscreenError {
    #[error("Failed to get raw window handle")]
    #[cfg(target_os = "macos")]
    GetRawWindowHandleError,
    #[error("Failed to get NSView")]
    #[cfg(target_os = "macos")]
    GetNSViewError,
    #[error("Failed to get NSWindow")]
    #[cfg(target_os = "macos")]
    GetNSWindowError,
    #[error("Failed to get raw window handle")]
    #[cfg(target_os = "macos")]
    FailedToGetRawWindowHandle,
}

/// Creates window attributes for the overlay window.
///
/// Returns a WindowAttributes instance configured for a transparent, always-on-top
/// overlay window suitable for rendering cursors and visual feedback.
pub fn get_window_attributes() -> WindowAttributes {
    WindowAttributes::default()
        .with_title("Overlay window")
        .with_window_level(WindowLevel::AlwaysOnTop)
        .with_decorations(false)
        .with_transparent(true)
        .with_inner_size(LogicalSize::new(
            OVERLAY_WINDOW_INITIAL_SIZE,
            OVERLAY_WINDOW_INITIAL_SIZE,
        ))
        .with_content_protected(true)
}

/// Sets a window to fullscreen on the specified monitor.
///
/// Platform-specific implementation:
/// - macOS: Uses simple fullscreen with window level above main menu
/// - Windows/Linux: Uses borderless fullscreen
fn set_fullscreen(
    window: &winit::window::Window,
    selected_monitor: MonitorHandle,
) -> Result<(), FullscreenError> {
    log::info!("set_fullscreen: {selected_monitor:?}");
    #[cfg(target_os = "macos")]
    {
        /* WA for putting the window in the right place. */
        window.set_maximized(true);
        window.set_simple_fullscreen(true);

        use objc2::rc::Retained;
        use objc2_app_kit::NSMainMenuWindowLevel;
        use objc2_app_kit::NSView;
        use raw_window_handle::HasWindowHandle;
        use raw_window_handle::RawWindowHandle;

        let raw_handle = window
            .window_handle()
            .map_err(|_| FullscreenError::GetRawWindowHandleError)?;
        if let RawWindowHandle::AppKit(handle) = raw_handle.as_raw() {
            let view = handle.ns_view.as_ptr();
            let ns_view: Option<Retained<NSView>> = unsafe { Retained::retain(view.cast()) };
            if ns_view.is_none() {
                return Err(FullscreenError::GetNSViewError);
            }
            let ns_view = ns_view.unwrap();
            let ns_window = ns_view.window();
            if ns_window.is_none() {
                return Err(FullscreenError::GetNSWindowError);
            }
            let ns_window = ns_window.unwrap();
            /* This is a hack to make the overlay window to appear above the main menu. */
            ns_window.setLevel(NSMainMenuWindowLevel + 1);
            return Ok(());
        }
        Err(FullscreenError::FailedToGetRawWindowHandle)
    }
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        use winit::window::Fullscreen;

        window.set_fullscreen(Some(Fullscreen::Borderless(Some(selected_monitor))));

        Ok(())
    }
}

/// Per-display overlay context containing window and surface resources.
///
/// Each OverlayContext represents a window associated with a specific display.
/// Windows are created hidden and positioned at the display's top-left corner,
/// then shown and set to fullscreen when screen sharing begins.
#[derive(Debug)]
pub struct OverlayContext<'a> {
    /// wgpu surface for rendering to the window
    pub surface: wgpu::Surface<'a>,
    /// Reference to the overlay window
    pub window: Arc<Window>,
    /// Renderer for iced graphics
    pub iced_renderer: IcedRenderer,
}

impl<'a> OverlayContext<'a> {
    /// Creates a new overlay context from an existing window.
    ///
    /// This takes an already-created and positioned window and creates the
    /// necessary surface for rendering.
    ///
    /// # Arguments
    ///
    /// * `window` - The pre-created overlay window
    /// * `surface` - The pre-created wgpu surface
    /// * `device` - The wgpu device
    /// * `queue` - The wgpu queue
    /// * `surface_format` - The surface format
    /// * `adapter` - The wgpu adapter
    /// * `texture_path` - The path to textures
    ///
    /// # Returns
    ///
    /// Returns a Result containing the initialized OverlayContext on success,
    /// or an error message string on failure.
    pub fn new(
        overlay_surface: OverlaySurface<'a>,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_format: wgpu::TextureFormat,
        adapter: &wgpu::Adapter,
        texture_path: &str,
    ) -> Result<Self, String> {
        log::info!(
            "OverlayContext::new for window {:?}",
            overlay_surface.window
        );

        let iced_renderer = IcedRenderer::new(
            device,
            queue,
            surface_format,
            adapter,
            &overlay_surface.window,
            texture_path,
        );

        Ok(Self {
            surface: overlay_surface.surface,
            window: overlay_surface.window,
            iced_renderer,
        })
    }

    /// Shows the window and sets it to fullscreen on the specified monitor.
    ///
    /// # Arguments
    ///
    /// * `monitor` - The monitor to fullscreen on
    ///
    /// # Returns
    ///
    /// Returns Ok(()) on success, or an error message on failure.
    pub fn show_fullscreen(&self, monitor: MonitorHandle) -> Result<(), String> {
        show_window_fullscreen(&self.window, monitor)
    }

    /// Configures the surface with the appropriate settings.
    pub fn configure_surface(
        &self,
        device: &wgpu::Device,
        adapter: &wgpu::Adapter,
        surface_format: wgpu::TextureFormat,
    ) -> Result<(), String> {
        let size = self.window.inner_size();
        let surface_capabilities = self.surface.get_capabilities(adapter);
        let alpha_modes = surface_capabilities.alpha_modes;

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
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
        self.surface.configure(device, &surface_config);

        #[cfg(target_os = "windows")]
        {
            self.surface.direct_composition.commit().map_err(|e| {
                log::error!(
                    "OverlayContext::configure_surface: DirectComposition commit failed: {:?}",
                    e
                );
                format!("DirectComposition commit failed: {:?}", e)
            })?;

            // Windows workaround for resetting the default white background
            self.surface.window.set_minimized(true);
            std::thread::sleep(std::time::Duration::from_millis(100));
            self.surface.window.set_minimized(false);
        }

        Ok(())
    }

    /// Hides the window.
    pub fn hide(&self) {
        log::info!("OverlayContext::hide for window {:?}", self.window);
        self.window.set_visible(false);
    }
}

/// Creates a surface for the given window.
pub fn create_surface<'a>(
    window: Arc<Window>,
    instance: &wgpu::Instance,
) -> Result<OverlaySurface<'a>, String> {
    #[cfg(target_os = "windows")]
    {
        let direct_composition = DirectComposition::new(window.clone())
            .ok_or_else(|| "Failed to create DirectComposition".to_string())?;

        let surface = direct_composition
            .create_surface(instance)
            .map_err(|e| format!("Failed to create surface: {:?}", e))?;

        Ok(OverlaySurface {
            surface,
            window,
            direct_composition,
        })
    }

    #[cfg(not(target_os = "windows"))]
    {
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("Failed to create surface: {:?}", e))?;

        Ok(OverlaySurface { surface, window })
    }
}

/// Creates a single overlay window for a monitor.
///
/// This function creates a hidden, positioned overlay window for the given monitor.
/// The window is created with standard overlay attributes and positioned at the
/// monitor's top-left corner.
///
/// # Arguments
///
/// * `event_loop` - The active event loop for window creation
/// * `monitor` - The monitor to create the window for
///
/// # Returns
///
/// Returns an Arc<Window> on success, or an error message on failure.
fn create_overlay_window_for_monitor(
    event_loop: &ActiveEventLoop,
    monitor: &MonitorHandle,
) -> Result<Arc<Window>, String> {
    // Create window with standard overlay attributes
    let attributes = get_window_attributes();
    let window = event_loop
        .create_window(attributes)
        .map_err(|e| format!("Failed to create window: {}", e))?;

    // Platform-specific configuration
    #[cfg(target_os = "linux")]
    {
        /* This is needed for getting the system picker for screen sharing. */
        let _ = window.request_inner_size(monitor.size());
    }

    // Disable cursor hittest so the window doesn't capture mouse events
    window
        .set_cursor_hittest(false)
        .map_err(|e| format!("Failed to set cursor hittest: {}", e))?;

    #[cfg(target_os = "windows")]
    window.set_skip_taskbar(true);

    #[cfg(target_os = "macos")]
    window.set_has_shadow(false);

    // Position window at display's top-left corner
    let position = monitor.position();
    let scale = monitor.scale_factor();
    let logical_position = LogicalPosition::new(position.x as f64, position.y as f64);
    let physical_position: PhysicalPosition<f64> = logical_position.to_physical(scale);
    window.set_outer_position(physical_position);

    // Start hidden
    window.set_visible(false);

    show_window_fullscreen(&window, monitor.clone())?;

    log::info!(
        "create_overlay_window_for_monitor: window created position {:?}",
        window.outer_position()
    );

    Ok(Arc::new(window))
}

/// Shows the window and sets it to fullscreen on the specified monitor.
pub fn show_window_fullscreen(window: &Window, monitor: MonitorHandle) -> Result<(), String> {
    log::info!("show_window_fullscreen for window {:?}", window);

    window.set_visible(true);

    set_fullscreen(window, monitor).map_err(|e| format!("Failed to set fullscreen: {}", e))?;

    Ok(())
}

/// Creates overlay windows for all available monitors.
///
/// This function queries all available monitors from the event loop and creates
/// a hidden, positioned overlay window for each. The windows are created with
/// standard overlay attributes and positioned at their respective display's top-left corner.
///
/// # Arguments
///
/// * `event_loop` - The active event loop for window creation
///
/// # Returns
///
/// Returns a Vec of Arc<Window>, one for each available monitor.
pub fn create_overlay_windows(event_loop: &ActiveEventLoop) -> Vec<Arc<Window>> {
    log::info!("create_overlay_windows: Creating overlay windows for all displays");

    let monitors: Vec<MonitorHandle> = event_loop.available_monitors().collect();
    log::info!("create_overlay_windows: Found {} monitors", monitors.len());

    let mut windows = Vec::new();

    for (index, monitor) in monitors.iter().enumerate() {
        log::info!(
            "create_overlay_windows: Creating overlay for monitor {}",
            index,
        );

        match create_overlay_window_for_monitor(event_loop, monitor) {
            Ok(window) => {
                windows.push(window);
            }
            Err(e) => {
                log::error!(
                    "create_overlay_windows: Failed to create overlay for monitor {}: {}",
                    index,
                    e
                );
                // Continue with other monitors even if one fails
            }
        }
    }

    log::info!(
        "create_overlay_windows: Created {} overlay windows",
        windows.len()
    );

    windows
}
