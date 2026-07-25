use screencapturekit::prelude::*;
use socket_lib::{Content, ContentType};
use winit::monitor::MonitorHandle;

use super::{display_title, full_display_frame, ListedWindows, ShareableSource};
use crate::capture::capturer::{ScreenshareExt, ScreenshareFunctions};
use crate::capture::thumbnail::{self, Thumbnail};
use crate::capture::window_bounds_macos;
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
            let thumbnail = thumbnail::display_thumbnail(id as u32);
            Some(ShareableSource {
                content: Content {
                    content_type: ContentType::Display,
                    id,
                },
                title: display_title(index, monitor),
                app_name: None,
                frame: full_display_frame(),
                monitor_content_id: id,
                thumbnail,
            })
        })
        .collect()
}

pub fn list_windows(monitors: &[MonitorHandle]) -> ListedWindows {
    let content = match SCShareableContent::get() {
        Ok(content) => content,
        Err(error) => {
            log::error!("list_windows: SCShareableContent::get failed: {error}");
            let message = error.to_string();
            let hint = if message.contains("declined")
                || message.contains("TCC")
                || message.contains("Content unavailable")
            {
                "Screen Recording permission is required to share windows. Enable Hopp in System Settings → Privacy & Security → Screen Recording, then restart the app.".to_string()
            } else {
                format!("Could not list windows: {error}")
            };
            return ListedWindows {
                windows: Vec::new(),
                error: Some(hint),
            };
        }
    };

    let displays = content.displays();
    let windows: Vec<ShareableSource> = content
        .windows()
        .into_iter()
        .filter_map(|window| {
            if !window.is_on_screen() || window.window_layer() != 0 {
                return None;
            }
            let title = window.title()?.trim().to_string();
            if title.is_empty() {
                return None;
            }

            let app_name = window
                .owning_application()
                .map(|app| app.application_name())
                .filter(|name| !name.is_empty());

            if is_hopp_window(&title, app_name.as_deref()) {
                return None;
            }

            let window_id = window.window_id();
            let cg_frame = window.frame();
            if cg_frame.size.width <= 1.0 || cg_frame.size.height <= 1.0 {
                return None;
            }

            // Prefer Quartz bounds for overlay alignment; fall back to SCWindow.frame so a
            // failed CG lookup never empties the picker.
            let (monitor_content_id, frame) = window_bounds_macos::display_local_frame_for_window(
                window_id, monitors,
            )
            .or_else(|| resolve_sc_window_monitor(&cg_frame, &displays, monitors))?;

            let thumb: Option<Thumbnail> = thumbnail::window_thumbnail(window_id);

            Some(ShareableSource {
                content: Content {
                    content_type: ContentType::Window,
                    id: window_id as u64,
                },
                title,
                app_name,
                frame,
                monitor_content_id,
                thumbnail: thumb,
            })
        })
        .collect();

    if windows.is_empty() {
        log::warn!(
            "list_windows: no shareable windows after filtering (SC windows may lack titles or bounds)"
        );
    } else {
        log::info!("list_windows: listed {} windows", windows.len());
    }

    ListedWindows {
        windows,
        error: None,
    }
}

fn is_hopp_window(title: &str, app_name: Option<&str>) -> bool {
    let title_lower = title.to_ascii_lowercase();
    if title_lower.contains("hopp") {
        return true;
    }
    app_name
        .map(|name| name.to_ascii_lowercase().contains("hopp"))
        .unwrap_or(false)
}

/// Resolve monitor + display-local frame from SCWindow/SCDisplay coordinates.
/// SC shareable frames share one coordinate space; we map into winit physical pixels.
fn resolve_sc_window_monitor(
    cg_frame: &screencapturekit::cg::CGRect,
    displays: &[SCDisplay],
    monitors: &[MonitorHandle],
) -> Option<(u64, Frame)> {
    let center_x = cg_frame.origin.x + cg_frame.size.width / 2.0;
    let center_y = cg_frame.origin.y + cg_frame.size.height / 2.0;

    let display = displays.iter().find(|display| {
        let frame = display.frame();
        center_x >= frame.origin.x
            && center_x < frame.origin.x + frame.size.width
            && center_y >= frame.origin.y
            && center_y < frame.origin.y + frame.size.height
    })?;

    let display_id = display.display_id() as u64;
    let display_frame = display.frame();

    let scale = monitors
        .iter()
        .find(|monitor| {
            ScreenshareFunctions::capture_content_id_for_monitor(monitor)
                .map(|id| id == display_id)
                .unwrap_or(false)
        })
        .map(|monitor| monitor.scale_factor())
        .or_else(|| monitors.first().map(|m| m.scale_factor()))
        .unwrap_or(1.0);

    // SCWindow.frame / SCDisplay.frame use the same space; treat as top-left relative to the
    // display (matches Quartz / CGWindowList used for live marker updates).
    let frame = Frame {
        origin_x: (cg_frame.origin.x - display_frame.origin.x) * scale,
        origin_y: (cg_frame.origin.y - display_frame.origin.y) * scale,
        extent: Extent {
            width: cg_frame.size.width * scale,
            height: cg_frame.size.height * scale,
        },
    };

    Some((display_id, frame))
}
