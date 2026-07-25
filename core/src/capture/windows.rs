use winit::platform::windows::MonitorHandleExtWindows;

use crate::capture::capturer::{MonitorId, ScreenshareExt};

use windows::core::PCWSTR;
use windows::Win32::Graphics::Gdi::{EnumDisplayDevicesW, DISPLAY_DEVICEW};

pub struct ScreenshareFunctions {}

impl ScreenshareExt for ScreenshareFunctions {
    fn get_selected_monitor(
        monitors: &[winit::monitor::MonitorHandle],
        input_id: u64,
    ) -> winit::monitor::MonitorHandle {
        let mut selected_monitor = monitors[0].clone();
        let input_monitor_name = get_display_index(input_id as u32);
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

    fn capture_content_id_for_monitor(monitor: &winit::monitor::MonitorHandle) -> Option<u64> {
        let monitor_name = monitor.native_id();
        let mut index = 0u32;

        loop {
            let display_name = get_display_index(index);
            if display_name.is_empty() {
                return None;
            }

            if display_name == monitor_name {
                return Some(index as u64);
            }

            index += 1;
        }
    }
}

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
