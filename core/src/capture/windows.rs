use winit::platform::windows::MonitorHandleExtWindows;

use crate::{
    capture::capturer::{MonitorId, ScreenshareExt},
    utils::geometry::Extent,
};

use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW};

pub struct ScreenshareFunctions {}

impl ScreenshareExt for ScreenshareFunctions {
    fn get_monitor_size(monitors: &[winit::monitor::MonitorHandle], input_id: u32) -> Extent {
        let input_monitor_name = get_display_index(input_id);
        log::debug!("get_monitor_size input name: {input_monitor_name:?}");
        for monitor in monitors {
            if monitor.native_id() == input_monitor_name {
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
        let input_monitor_name = get_display_index(input_id);
        for monitor in monitors {
            if monitor.native_id() == input_monitor_name {
                selected_monitor = monitor.clone();
                break;
            }
        }
        selected_monitor
    }

    fn get_monitor_id(monitor: &winit::monitor::MonitorHandle) -> MonitorId {
        MonitorId::Named(monitor.native_id())
    }
}

// TODO: Change name to this.
fn get_display_index(input_id: u32) -> String {
    unsafe {
        let mut display_device = DISPLAY_DEVICEW {
            cb: std::mem::size_of::<DISPLAY_DEVICEW>() as u32,
            ..Default::default()
        };
        let null_ptr = PCWSTR::null();
        let res = EnumDisplayDevicesW(null_ptr, input_id, &mut display_device, 0).as_bool();
        if !res {
            return String::new();
        }
        String::from_utf16_lossy(
            display_device.DeviceName[..]
                .split(|&x| x == 0)
                .next()
                .unwrap_or(&[]),
        )
    }
}
