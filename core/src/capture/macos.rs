use std::collections::HashMap;

use core_graphics::{
    event::CGEvent,
    event_source::{CGEventSource, CGEventSourceStateID},
    window::{
        create_window_list, kCGNullWindowID, kCGWindowListExcludeDesktopElements,
        kCGWindowListOptionOnScreenOnly,
    },
};
use screencapturekit::{
    cg::{CGPoint, CGRect},
    shareable_content::SCShareableContent,
};
#[cfg(target_os = "macos")]
use winit::platform::macos::MonitorHandleExtMacOS;

use crate::capture::capturer::{MonitorId, ScreenshareExt};

pub struct ScreenshareFunctions {}

impl ScreenshareExt for ScreenshareFunctions {
    fn get_selected_monitor(
        monitors: &[winit::monitor::MonitorHandle],
        input_id: u32,
    ) -> winit::monitor::MonitorHandle {
        let mut selected_monitor = monitors[0].clone();
        for monitor in monitors {
            if monitor.native_id() == input_id {
                selected_monitor = monitor.clone();
            }
        }
        selected_monitor
    }

    fn get_monitor_id(monitor: &winit::monitor::MonitorHandle) -> MonitorId {
        MonitorId::Numeric(monitor.native_id())
    }

    fn capture_content_id_for_monitor(monitor: &winit::monitor::MonitorHandle) -> Option<u32> {
        Some(monitor.native_id())
    }
}

impl Default for ScreenshareFunctions {
    fn default() -> Self {
        Self::new()
    }
}

impl ScreenshareFunctions {
    pub fn new() -> Self {
        Self {}
    }

    pub fn window_under_cursor(overlay_display_id: u32) -> Option<u32> {
        let event_source = match CGEventSource::new(CGEventSourceStateID::CombinedSessionState) {
            Ok(event_source) => event_source,
            Err(()) => {
                log::error!("window_under_cursor: failed to create CoreGraphics event source");
                return None;
            }
        };
        let cursor = match CGEvent::new(event_source) {
            Ok(event) => event.location(),
            Err(()) => {
                log::error!("window_under_cursor: failed to read cursor position");
                return None;
            }
        };
        let cursor = CGPoint::new(cursor.x, cursor.y);
        log::info!(
            "window_under_cursor: cursor=({:.1}, {:.1}) overlay_display_id={}",
            cursor.x,
            cursor.y,
            overlay_display_id
        );

        let content = match SCShareableContent::create()
            .with_exclude_desktop_windows(true)
            .with_on_screen_windows_only(true)
            .get()
        {
            Ok(content) => content,
            Err(error) => {
                log::error!("window_under_cursor: ScreenCaptureKit enumeration failed: {error:?}");
                return None;
            }
        };
        let displays = content.displays();
        let Some(overlay_frame) = displays
            .iter()
            .find(|display| display.display_id() == overlay_display_id)
            .map(|display| display.frame())
        else {
            log::warn!(
                "window_under_cursor: display {} missing from {} shareable displays",
                overlay_display_id,
                displays.len()
            );
            return None;
        };
        let windows: HashMap<u32, CGRect> = content
            .windows()
            .into_iter()
            .map(|window| (window.window_id(), window.frame()))
            .collect();
        log::info!(
            "window_under_cursor: enumerated {} displays and {} on-screen shareable windows",
            displays.len(),
            windows.len()
        );

        let Some(window_ids) = create_window_list(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
            kCGNullWindowID,
        ) else {
            log::error!("window_under_cursor: CoreGraphics returned no window list");
            return None;
        };
        log::info!(
            "window_under_cursor: CoreGraphics returned {} windows in z-order",
            window_ids.len()
        );

        let selected = frontmost_window(
            &windows,
            window_ids.iter().map(|window_id| *window_id),
            cursor,
            overlay_frame,
        );
        log::info!("window_under_cursor: selected window={selected:?}");
        selected
    }
}

fn frontmost_window(
    windows: &HashMap<u32, CGRect>,
    window_ids: impl IntoIterator<Item = u32>,
    cursor: CGPoint,
    overlay_frame: CGRect,
) -> Option<u32> {
    let mut skipped_overlay = false;

    for window_id in window_ids {
        let Some(frame) = windows.get(&window_id) else {
            continue;
        };
        if !frame.contains_point(cursor) {
            continue;
        }

        log::info!(
            "window_under_cursor: hit id={} frame={:?}",
            window_id,
            frame,
        );
        // ponytail: the first display-sized hit is the selection overlay; pass
        // its native window ID instead if ScreenCaptureKit ever omits or reorders it.
        if !skipped_overlay && *frame == overlay_frame {
            skipped_overlay = true;
            log::info!("window_under_cursor: skipping selection overlay id={window_id}");
            continue;
        }

        log::info!("window_under_cursor: accepting id={window_id}");
        return Some(window_id);
    }

    log::info!("window_under_cursor: no shareable window contains the cursor");
    None
}
