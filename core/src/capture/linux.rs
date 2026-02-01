use crate::{
    capture::capturer::{MonitorId, ScreenshareExt},
    utils::geometry::Extent,
};

pub struct ScreenshareFunctions {}

impl ScreenshareExt for ScreenshareFunctions {
    fn get_monitor_size(monitors: &[winit::monitor::MonitorHandle], _input_id: u32) -> Extent {
        Extent {
            width: 0.,
            height: 0.,
        }
    }

    fn get_selected_monitor(
        monitors: &[winit::monitor::MonitorHandle],
        _input_id: u32,
    ) -> winit::monitor::MonitorHandle {
        monitors[0].clone()
    }

    fn get_monitor_id(monitor: &winit::monitor::MonitorHandle) -> MonitorId {
        MonitorId::Position(monitor.position())
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
