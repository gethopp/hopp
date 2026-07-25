use winit::monitor::MonitorHandle;

use crate::capture::thumbnail::Thumbnail;
use crate::utils::geometry::Frame;

/// A display or window the user can share.
#[derive(Debug, Clone)]
pub struct ShareableSource {
    pub content: socket_lib::Content,
    pub title: String,
    pub app_name: Option<String>,
    /// Frame relative to the containing monitor in physical pixels.
    /// Zero extent for full-display shares.
    pub frame: Frame,
    /// Capture content id of the monitor used for overlay / remote control.
    pub monitor_content_id: u64,
    /// Optional picker preview image.
    pub thumbnail: Option<Thumbnail>,
}

impl ShareableSource {
    pub fn display_label(&self) -> String {
        if let Some(app) = &self.app_name {
            if !app.is_empty() && app != &self.title {
                return format!("{app} — {}", self.title);
            }
        }
        self.title.clone()
    }
}

/// Result of enumerating shareable application windows.
#[derive(Debug, Clone, Default)]
pub struct ListedWindows {
    pub windows: Vec<ShareableSource>,
    pub error: Option<String>,
}

/// Lists shareable displays for the current platform.
pub fn list_displays(monitors: &[MonitorHandle]) -> Vec<ShareableSource> {
    platform::list_displays(monitors)
}

/// Lists shareable application windows. Empty on Linux (unsupported in v1).
pub fn list_windows(monitors: &[MonitorHandle]) -> ListedWindows {
    platform::list_windows(monitors)
}

pub fn windows_supported() -> bool {
    platform::windows_supported()
}

fn display_title(index: usize, monitor: &MonitorHandle) -> String {
    monitor
        .name()
        .map(|name| {
            if name.is_empty() {
                format!("Screen {}", index + 1)
            } else {
                name
            }
        })
        .unwrap_or_else(|| format!("Screen {}", index + 1))
}

fn full_display_frame() -> Frame {
    Frame::default()
}

#[cfg_attr(target_os = "macos", path = "sources_macos.rs")]
#[cfg_attr(target_os = "windows", path = "sources_windows.rs")]
#[cfg_attr(target_os = "linux", path = "sources_linux.rs")]
mod platform;
