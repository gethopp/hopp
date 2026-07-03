use crate::capture::capturer::{MonitorId, ScreenshareExt};

pub struct ScreenshareFunctions {}

impl ScreenshareExt for ScreenshareFunctions {
    fn get_selected_monitor(
        monitors: &[winit::monitor::MonitorHandle],
        _input_id: u32,
    ) -> winit::monitor::MonitorHandle {
        monitors[0].clone()
    }

    fn get_monitor_id(monitor: &winit::monitor::MonitorHandle) -> MonitorId {
        MonitorId::Position(monitor.position())
    }

    fn capture_content_id_for_monitor(_monitor: &winit::monitor::MonitorHandle) -> Option<u32> {
        Some(0)
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
