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
}
