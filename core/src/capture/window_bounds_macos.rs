//! Window bounds from CGWindowList — global top-left points (Quartz space).

use core_foundation::base::TCFType;
use core_foundation::dictionary::CFDictionaryRef;
use core_foundation::number::CFNumber;
use core_graphics::display::CGDisplay;
use core_graphics::geometry::{CGPoint, CGRect, CGSize};
use core_graphics::window::{
    kCGWindowBounds, kCGWindowListOptionIncludingWindow, kCGWindowListOptionOnScreenOnly,
    kCGWindowNumber,
};
use std::ffi::c_void;
use winit::dpi::PhysicalPosition;
use winit::monitor::MonitorHandle;

use crate::capture::capturer::{ScreenshareExt, ScreenshareFunctions};
use crate::utils::geometry::{Extent, Frame};

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDictionaryGetValue(theDict: CFDictionaryRef, key: *const c_void) -> *const c_void;
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGRectMakeWithDictionaryRepresentation(
        dict: CFDictionaryRef,
        rect: *mut CGRect,
    ) -> bool;
}

/// Global top-left window bounds in points.
pub fn window_bounds_points(window_id: u32) -> Option<CGRect> {
    if let Some(rect) =
        bounds_from_window_list(kCGWindowListOptionIncludingWindow, Some(window_id), window_id)
    {
        return Some(rect);
    }

    bounds_from_window_list(kCGWindowListOptionOnScreenOnly, None, window_id)
}

fn bounds_from_window_list(
    option: u32,
    relative_to: Option<u32>,
    window_id: u32,
) -> Option<CGRect> {
    let infos = CGDisplay::window_list_info(option, relative_to)?;
    if infos.is_empty() {
        return None;
    }

    for dict_ptr in infos.get_all_values() {
        if dict_ptr.is_null() {
            continue;
        }
        unsafe {
            if relative_to.is_none() {
                let number_ptr = CFDictionaryGetValue(
                    dict_ptr as CFDictionaryRef,
                    kCGWindowNumber as *const c_void,
                );
                if number_ptr.is_null() {
                    continue;
                }
                let number = CFNumber::wrap_under_get_rule(number_ptr as _);
                let Some(id) = number.to_i64() else {
                    continue;
                };
                if id as u32 != window_id {
                    continue;
                }
            }

            let bounds_ptr = CFDictionaryGetValue(
                dict_ptr as CFDictionaryRef,
                kCGWindowBounds as *const c_void,
            );
            if bounds_ptr.is_null() {
                continue;
            }

            let mut rect = CGRect::new(&CGPoint::new(0., 0.), &CGSize::new(0., 0.));
            let ok =
                CGRectMakeWithDictionaryRepresentation(bounds_ptr as CFDictionaryRef, &mut rect);
            if ok && rect.size.width > 1.0 && rect.size.height > 1.0 {
                return Some(rect);
            }
        }
    }
    None
}

/// Display-local physical-pixel frame (top-left) for a window, plus monitor capture id.
pub fn display_local_frame_for_window(
    window_id: u32,
    monitors: &[MonitorHandle],
) -> Option<(u64, Frame)> {
    let bounds = window_bounds_points(window_id)?;
    let monitor = monitor_containing_point(
        bounds.origin.x + bounds.size.width / 2.0,
        bounds.origin.y + bounds.size.height / 2.0,
        monitors,
    )
    .or_else(|| monitors.first())?;
    let scale = monitor.scale_factor();
    let pos = monitor.position();
    let monitor_content_id = ScreenshareFunctions::capture_content_id_for_monitor(monitor)
        .or_else(|| {
            monitors
                .first()
                .and_then(ScreenshareFunctions::capture_content_id_for_monitor)
        })?;

    Some((
        monitor_content_id,
        frame_from_global_points(&bounds, scale, pos),
    ))
}

/// Display-local physical frame given the capture monitor's scale and position.
pub fn display_local_frame_on_monitor(
    window_id: u32,
    scale: f64,
    display_position: PhysicalPosition<i32>,
) -> Option<Frame> {
    let bounds = window_bounds_points(window_id)?;
    Some(frame_from_global_points(&bounds, scale, display_position))
}

fn frame_from_global_points(
    bounds: &CGRect,
    scale: f64,
    display_position: PhysicalPosition<i32>,
) -> Frame {
    Frame {
        origin_x: bounds.origin.x * scale - display_position.x as f64,
        origin_y: bounds.origin.y * scale - display_position.y as f64,
        extent: Extent {
            width: bounds.size.width * scale,
            height: bounds.size.height * scale,
        },
    }
}

fn monitor_containing_point<'a>(
    point_x: f64,
    point_y: f64,
    monitors: &'a [MonitorHandle],
) -> Option<&'a MonitorHandle> {
    monitors.iter().find(|monitor| {
        let scale = monitor.scale_factor();
        let pos = monitor.position();
        let size = monitor.size();
        let left = pos.x as f64 / scale;
        let top = pos.y as f64 / scale;
        let right = left + size.width as f64 / scale;
        let bottom = top + size.height as f64 / scale;
        point_x >= left && point_x < right && point_y >= top && point_y < bottom
    })
}
