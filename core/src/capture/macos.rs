use std::collections::HashMap;

use core_graphics::{
    event::CGEvent,
    event_source::{CGEventSource, CGEventSourceStateID},
    window::{
        create_window_list, kCGNullWindowID, kCGWindowListExcludeDesktopElements,
        kCGWindowListOptionOnScreenOnly,
    },
};
use screencapturekit::shareable_content::SCShareableContent;
#[cfg(target_os = "macos")]
use winit::platform::macos::MonitorHandleExtMacOS;

use crate::{
    capture::capturer::{MonitorId, ScreenshareExt},
    utils::geometry::{Extent, Frame, Position},
    SelectableWindow,
};

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

    pub(crate) fn cursor_position() -> Option<Position> {
        let event_source = match CGEventSource::new(CGEventSourceStateID::CombinedSessionState) {
            Ok(event_source) => event_source,
            Err(()) => {
                log::error!("cursor_position: failed to create CoreGraphics event source");
                return None;
            }
        };
        let cursor = match CGEvent::new(event_source) {
            Ok(event) => event.location(),
            Err(()) => {
                log::error!("cursor_position: failed to read cursor position");
                return None;
            }
        };

        Some(Position {
            x: cursor.x,
            y: cursor.y,
        })
    }

    pub(crate) fn selectable_windows() -> Option<Vec<SelectableWindow>> {
        let content = match SCShareableContent::create()
            .with_exclude_desktop_windows(true)
            .with_on_screen_windows_only(true)
            .get()
        {
            Ok(content) => content,
            Err(error) => {
                log::error!("selectable_windows: ScreenCaptureKit enumeration failed: {error:?}");
                return None;
            }
        };
        let current_process_id = std::process::id() as i32;
        let windows: HashMap<u32, Frame> = content
            .windows()
            .into_iter()
            .filter(|window| {
                let owner_process_id = window
                    .owning_application()
                    .map(|application| application.process_id());
                is_window_selectable(window.window_layer(), owner_process_id, current_process_id)
            })
            .map(|window| {
                let frame = window.frame();
                (
                    window.window_id(),
                    Frame {
                        origin_x: frame.origin.x,
                        origin_y: frame.origin.y,
                        extent: Extent {
                            width: frame.size.width,
                            height: frame.size.height,
                        },
                    },
                )
            })
            .collect();

        let Some(window_ids) = create_window_list(
            kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
            kCGNullWindowID,
        ) else {
            log::error!("selectable_windows: CoreGraphics returned no window list");
            return None;
        };

        Some(windows_in_front_order(
            &windows,
            window_ids.iter().map(|window_id| *window_id),
        ))
    }
}

fn is_window_selectable(
    layer: i32,
    owner_process_id: Option<i32>,
    current_process_id: i32,
) -> bool {
    layer == 0 && owner_process_id.is_some_and(|process_id| process_id != current_process_id)
}

fn windows_in_front_order(
    windows: &HashMap<u32, Frame>,
    window_ids: impl IntoIterator<Item = u32>,
) -> Vec<SelectableWindow> {
    window_ids
        .into_iter()
        .filter_map(|id| {
            windows
                .get(&id)
                .copied()
                .map(|frame| SelectableWindow { id, frame })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_shareable_windows_in_front_to_back_order() {
        let frame = Frame {
            origin_x: 0.0,
            origin_y: 0.0,
            extent: Extent {
                width: 100.0,
                height: 100.0,
            },
        };
        let windows = HashMap::from([(1, frame), (2, frame), (3, frame)]);

        let ordered = windows_in_front_order(&windows, [3, 9, 1]);

        assert_eq!(
            ordered
                .into_iter()
                .map(|window| window.id)
                .collect::<Vec<_>>(),
            vec![3, 1]
        );
    }
}
