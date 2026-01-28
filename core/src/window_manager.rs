use std::sync::Arc;
use thiserror::Error;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event_loop::ActiveEventLoop;
use winit::monitor::MonitorHandle;
use winit::window::{Window, WindowAttributes, WindowLevel};

#[cfg(target_os = "macos")]
use winit::platform::macos::WindowExtMacOS;

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
    monitor_position: PhysicalPosition<i32>,
}

pub struct WindowManager {
    windows: Vec<WindowEntry>,
    active_window_index: Option<usize>,
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
            active_window_index: None,
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
            use winit::platform::windows::WindowExtWindows;
            window.set_skip_taskbar(true);
        }

        #[cfg(target_os = "macos")]
        {
            window.set_has_shadow(false);
        }

        let monitor_position = monitor.position();
        let logical_position = monitor_position.to_logical::<f64>(monitor.scale_factor());

        let final_position =
            LogicalPosition::new(logical_position.x + 30., logical_position.y + 30.);

        window.set_outer_position(final_position);
        window.set_visible(false);

        Ok(WindowEntry {
            window,
            monitor_position,
        })
    }

    pub fn show_window(
        &mut self,
        monitor: &MonitorHandle,
    ) -> Result<Arc<Window>, WindowManagerError> {
        let monitor_position = monitor.position();
        log::info!(
            "WindowManager::show_window: looking for window at {:?}",
            monitor_position
        );

        for entry in &self.windows {
            log::info!(
                "WindowManager::show_window: display {:?} window {:?}",
                entry.monitor_position,
                entry.window.outer_position()
            );
        }
        let index = self
            .windows
            .iter()
            .position(|entry| entry.monitor_position == monitor_position)
            .ok_or(WindowManagerError::MonitorNotFound)?;

        let window = &self.windows[index].window;

        log::info!("window fullscreen {:?}", window.fullscreen());
        if let Err(e) = set_fullscreen(window, monitor.clone()) {
            log::error!(
                "WindowManager::show_window: error setting fullscreen: {:?}",
                e
            );
            return Err(WindowManagerError::FullscreenError(e.to_string()));
        }

        window.set_visible(true);
        self.active_window_index = Some(index);

        Ok(window.clone())
    }

    pub fn hide_active_window(&mut self) {
        if let Some(index) = self.active_window_index.take() {
            log::info!(
                "WindowManager::hide_active_window: hiding window at index {}",
                index
            );
            self.windows[index].window.set_visible(false);
        }
    }

    pub fn update(&mut self, event_loop: &ActiveEventLoop) -> Result<(), WindowManagerError> {
        let monitors: Vec<MonitorHandle> = event_loop.available_monitors().collect();
        let mut monitor_positions: Vec<PhysicalPosition<i32>> =
            monitors.iter().map(|m| m.position()).collect();

        log::info!(
            "WindowManager::update: checking {} monitors",
            monitors.len()
        );

        self.windows.retain(|entry| {
            if let Some(pos_index) = monitor_positions
                .iter()
                .position(|&pos| pos == entry.monitor_position)
            {
                monitor_positions.remove(pos_index);
                true
            } else {
                log::info!(
                    "WindowManager::update: removing window at outdated position {:?}",
                    entry.monitor_position
                );
                false
            }
        });

        for position in monitor_positions {
            log::info!(
                "WindowManager::update: adding new window for position {:?}",
                position
            );

            let monitor = monitors
                .iter()
                .find(|m| m.position() == position)
                .ok_or(WindowManagerError::MonitorNotFound)?;

            let window_entry = Self::create_window_entry(event_loop, monitor)?;
            self.windows.push(window_entry);
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
