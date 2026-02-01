#[cfg(target_os = "macos")]
use winit::platform::macos::MonitorHandleExtMacOS;

use crate::{
    capture::capturer::{MonitorId, ScreenshareExt},
    utils::geometry::Extent,
};

pub struct ScreenshareFunctions {}

impl ScreenshareExt for ScreenshareFunctions {
    fn get_monitor_size(monitors: &[winit::monitor::MonitorHandle], input_id: u32) -> Extent {
        for monitor in monitors {
            if monitor.native_id() == input_id {
                let monitor_size = monitor.size();
                return Extent {
                    width: monitor_size.width as f64,
                    height: monitor_size.height as f64,
                };
            }
        }

        Extent {
            width: 0.,
            height: 0.,
        }
    }

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
