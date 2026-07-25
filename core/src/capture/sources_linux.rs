use socket_lib::{Content, ContentType};
use winit::monitor::MonitorHandle;

use super::{display_title, full_display_frame, ListedWindows, ShareableSource};
use crate::capture::capturer::{ScreenshareExt, ScreenshareFunctions};

pub fn windows_supported() -> bool {
    false
}

pub fn list_displays(monitors: &[MonitorHandle]) -> Vec<ShareableSource> {
    monitors
        .iter()
        .enumerate()
        .filter_map(|(index, monitor)| {
            let id = ScreenshareFunctions::capture_content_id_for_monitor(monitor)?;
            Some(ShareableSource {
                content: Content {
                    content_type: ContentType::Display,
                    id,
                },
                title: display_title(index, monitor),
                app_name: None,
                frame: full_display_frame(),
                monitor_content_id: id,
                thumbnail: None,
            })
        })
        .collect()
}

pub fn list_windows(_monitors: &[MonitorHandle]) -> ListedWindows {
    ListedWindows {
        windows: Vec::new(),
        error: Some("Window sharing is not available on Linux yet.".to_string()),
    }
}
