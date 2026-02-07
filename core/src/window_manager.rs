use std::sync::Arc;
use thiserror::Error;
use winit::dpi::{LogicalPosition, LogicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::monitor::MonitorHandle;
use winit::window::{Window, WindowAttributes, WindowLevel};

#[cfg(target_os = "macos")]
use winit::platform::macos::WindowExtMacOS;

#[cfg(target_os = "windows")]
use winit::platform::windows::WindowExtWindows;

use crate::capture::capturer::{MonitorId, ScreenshareExt, ScreenshareFunctions};
use crate::ServerError;

// Constants for magic numbers
/// Initial size for the overlay window (width and height in logical pixels)
const OVERLAY_WINDOW_INITIAL_SIZE: f64 = 1.0;

fn get_window_attributes() -> WindowAttributes {
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

/// Returns the logical position where a window should be placed for the given monitor.
fn get_window_position_for_monitor(monitor: &MonitorHandle) -> LogicalPosition<f64> {
    let monitor_position = monitor.position();
    monitor_position.to_logical::<f64>(monitor.scale_factor())
}

#[derive(Error, Debug)]
pub enum WindowManagerError {
    #[error("Failed to create window")]
    WindowCreationError,
    #[error("Monitor not found")]
    MonitorNotFound,
    #[error("Fullscreen error: {0}")]
    FullscreenError(String),
}

impl From<WindowManagerError> for ServerError {
    fn from(err: WindowManagerError) -> Self {
        match err {
            WindowManagerError::WindowCreationError => ServerError::WindowCreationError,
            WindowManagerError::MonitorNotFound => ServerError::WindowCreationError, // Map to WindowCreationError for now
            WindowManagerError::FullscreenError(_) => ServerError::FullscreenError,
        }
    }
}

struct WindowEntry {
    window: Arc<Window>,
    monitor_id: MonitorId,
    position: LogicalPosition<f64>,
}

pub struct WindowManager {
    windows: Vec<WindowEntry>,
    active_monitor_id: Option<MonitorId>,
}

impl WindowManager {
    pub fn new(event_loop: &ActiveEventLoop) -> Result<Self, WindowManagerError> {
        log::info!("WindowManager::new: creating windows for available monitors");
        let mut windows = Vec::new();

        for monitor in event_loop.available_monitors() {
            let window_entry = Self::create_window_entry(event_loop, &monitor)?;
            windows.push(window_entry);
        }

        Ok(Self {
            windows,
            active_monitor_id: None,
        })
    }

    fn create_window_entry(
        event_loop: &ActiveEventLoop,
        monitor: &MonitorHandle,
    ) -> Result<WindowEntry, WindowManagerError> {
        let attributes = get_window_attributes();
        let window = event_loop
            .create_window(attributes)
            .map_err(|_| WindowManagerError::WindowCreationError)?;

        let window = Arc::new(window);

        #[cfg(target_os = "linux")]
        {
            /* This is needed for getting the system picker for screen sharing. */
            let _ = window.request_inner_size(monitor.size().clone());
        }

        let _ = window.set_cursor_hittest(false);

        #[cfg(target_os = "windows")]
        {
            window.set_skip_taskbar(true);
        }

        #[cfg(target_os = "macos")]
        {
            window.set_has_shadow(false);
        }

        let position = get_window_position_for_monitor(monitor);
        window.set_outer_position(position);
        window.set_visible(false);

        let monitor_id = ScreenshareFunctions::get_monitor_id(monitor);

        Ok(WindowEntry {
            window,
            monitor_id,
            position,
        })
    }

    pub fn show_window(
        &mut self,
        monitor: &MonitorHandle,
    ) -> Result<Arc<Window>, WindowManagerError> {
        let target_id = ScreenshareFunctions::get_monitor_id(monitor);
        log::info!(
            "WindowManager::show_window: looking for window with id {:?}",
            target_id
        );

        let entry = self
            .windows
            .iter()
            .find(|entry| entry.monitor_id == target_id)
            .ok_or(WindowManagerError::MonitorNotFound)?;

        if let Err(e) = set_fullscreen(&entry.window, monitor.clone()) {
            log::error!(
                "WindowManager::show_window: error setting fullscreen: {:?}",
                e
            );
            return Err(WindowManagerError::FullscreenError(e.to_string()));
        }

        entry.window.set_visible(true);
        self.active_monitor_id = Some(target_id);

        Ok(entry.window.clone())
    }

    pub fn hide_active_window(&mut self) {
        if let Some(active_id) = self.active_monitor_id.take() {
            log::info!(
                "WindowManager::hide_active_window: hiding window for monitor {:?}",
                active_id
            );
            if let Some(entry) = self
                .windows
                .iter()
                .find(|entry| entry.monitor_id == active_id)
            {
                entry.window.set_visible(false);
            }
        }
    }

    pub fn is_active_window(&self, window_id: winit::window::WindowId) -> bool {
        self.active_monitor_id.as_ref().is_some_and(|active_id| {
            self.windows
                .iter()
                .find(|entry| &entry.monitor_id == active_id)
                .is_some_and(|entry| entry.window.id() == window_id)
        })
    }

    pub fn update(&mut self, event_loop: &ActiveEventLoop) -> Result<(), WindowManagerError> {
        let monitors: Vec<MonitorHandle> = event_loop.available_monitors().collect();

        log::info!(
            "WindowManager::update: checking {} monitors",
            monitors.len()
        );

        for monitor in &monitors {
            log::info!("WindowManager::update: monitor {:?}", monitor);
        }
        for window in &self.windows {
            log::info!(
                "WindowManager::update: window for monitor id {:?} with position {:?}",
                window.monitor_id,
                window.window.outer_position(),
            );
        }

        let mut matched_monitor_indices: Vec<usize> = Vec::new();
        let mut active_monitor_id_found = false;

        self.windows.retain_mut(|entry| {
            if let Some(monitor_idx) = monitors
                .iter()
                .position(|m| ScreenshareFunctions::get_monitor_id(m) == entry.monitor_id)
            {
                matched_monitor_indices.push(monitor_idx);
                let monitor = &monitors[monitor_idx];

                // Reposition only if the monitor position has changed
                let new_position = get_window_position_for_monitor(monitor);
                if entry.position != new_position {
                    log::info!(
                        "WindowManager::update: repositioning window for monitor {:?} from {:?} to {:?}",
                        entry.monitor_id,
                        entry.position,
                        new_position
                    );
                    entry.window.set_outer_position(new_position);
                    entry.position = new_position;

                    // Re-apply fullscreen if this is the active window and position changed
                    if self.active_monitor_id.as_ref() == Some(&entry.monitor_id) {
                        active_monitor_id_found = true;
                        log::info!(
                            "WindowManager::update: re-applying fullscreen for active window on monitor {:?}",
                            entry.monitor_id
                        );
                        if let Err(e) = set_fullscreen(&entry.window, monitor.clone()) {
                            log::error!(
                                "WindowManager::update: error setting fullscreen: {:?}",
                                e
                            );
                        }
                    }
                } else if self.active_monitor_id.as_ref() == Some(&entry.monitor_id) {
                    active_monitor_id_found = true;
                }

                true
            } else {
                log::info!(
                    "WindowManager::update: removing window for disconnected monitor {:?}",
                    entry.monitor_id
                );
                false
            }
        });

        // Clear active monitor if it was disconnected
        if !active_monitor_id_found && self.active_monitor_id.is_some() {
            log::info!(
                "WindowManager::update: active monitor {:?} disconnected, clearing active monitor",
                self.active_monitor_id
            );
            self.active_monitor_id = None;
        }

        // Add windows for new monitors
        for (idx, monitor) in monitors.iter().enumerate() {
            if !matched_monitor_indices.contains(&idx) {
                log::info!(
                    "WindowManager::update: adding new window for monitor {:?}",
                    ScreenshareFunctions::get_monitor_id(monitor)
                );
                self.windows
                    .push(Self::create_window_entry(event_loop, monitor)?);
            }
        }

        log::info!(
            "WindowManager::update: windows list length: {:?}",
            self.windows.len()
        );

        Ok(())
    }
}

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
    #[error("Failed to match fullscreen size within timeout")]
    FailedToMatchFullscreenSize,
}

fn set_fullscreen(
    window: &winit::window::Window,
    selected_monitor: MonitorHandle,
) -> Result<(), FullscreenError> {
    log::info!("set_fullscreen: {selected_monitor:?}");
    #[cfg(target_os = "macos")]
    {
        /* WA for putting the window in the right place. */
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
        } else {
            return Err(FullscreenError::FailedToGetRawWindowHandle);
        }
    }
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    {
        use winit::window::Fullscreen;

        window.set_fullscreen(Some(Fullscreen::Borderless(Some(selected_monitor.clone()))));
    }
    // Wait for the window to reach fullscreen size before creating the graphics context to avoid scissor rect
    // validation errors in wgpu.
    let expected_size = selected_monitor.size();
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(1);

    loop {
        let current_size = window.inner_size();
        if current_size.width == expected_size.width && current_size.height == expected_size.height
        {
            log::info!(
                "set_fullscreen: window reached fullscreen size {:?}",
                current_size
            );
            break;
        }

        if start.elapsed() > timeout {
            log::error!(
                "set_fullscreen: timeout waiting for fullscreen. Current: {:?}, Expected: {:?}",
                current_size,
                expected_size
            );
            return Err(FullscreenError::FailedToMatchFullscreenSize);
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    Ok(())
}
