use livekit::webrtc::desktop_capturer::{
    DesktopCaptureSourceType, DesktopCapturer, DesktopCapturerOptions,
};
use socket_lib::{Content, ContentType};
use winit::monitor::MonitorHandle;

use super::{display_title, full_display_frame, ListedWindows, ShareableSource};
use crate::capture::capturer::{ScreenshareExt, ScreenshareFunctions};
use crate::utils::geometry::{Extent, Frame};

pub fn windows_supported() -> bool {
    true
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

pub fn list_windows(monitors: &[MonitorHandle]) -> ListedWindows {
    let options = DesktopCapturerOptions::new(DesktopCaptureSourceType::Window);
    let Some(capturer) = DesktopCapturer::new(options) else {
        log::error!("list_windows: failed to create DesktopCapturer");
        return ListedWindows {
            windows: Vec::new(),
            error: Some("Could not enumerate windows for sharing.".to_string()),
        };
    };

    let windows = capturer
        .get_source_list()
        .into_iter()
        .filter_map(|source| {
            let title = source.title().trim().to_string();
            if title.is_empty() {
                return None;
            }
            if title.to_ascii_lowercase().contains("hopp") {
                return None;
            }

            let monitor_content_id = resolve_monitor_id(source.display_id(), monitors);
            Some(ShareableSource {
                content: Content {
                    content_type: ContentType::Window,
                    id: source.id(),
                },
                title,
                app_name: None,
                // Bounds are refined when capture starts / overlay is created.
                frame: Frame {
                    origin_x: 0.,
                    origin_y: 0.,
                    extent: Extent {
                        width: 0.,
                        height: 0.,
                    },
                },
                monitor_content_id,
                thumbnail: None,
            })
        })
        .collect();

    ListedWindows {
        windows,
        error: None,
    }
}

fn resolve_monitor_id(display_id: i64, monitors: &[MonitorHandle]) -> u64 {
    if display_id >= 0 {
        let as_u64 = display_id as u64;
        if monitors.iter().any(|monitor| {
            ScreenshareFunctions::capture_content_id_for_monitor(monitor)
                .map(|id| id == as_u64)
                .unwrap_or(false)
        }) {
            return as_u64;
        }
    }

    monitors
        .first()
        .and_then(ScreenshareFunctions::capture_content_id_for_monitor)
        .unwrap_or(0)
}
