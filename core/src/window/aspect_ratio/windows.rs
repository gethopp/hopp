use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use winit::window::Window;

use super::{calculate_max_window_size, min_window_size_for_aspect, WindowConstant};
use crate::utils::geometry::{Extent, Position};

struct WindowAspectState {
    content_aspect_bits: AtomicU64,
    scale_factor_bits: AtomicU64,
}

impl WindowAspectState {
    fn set(&self, content_aspect: f64, scale_factor: f64) {
        self.content_aspect_bits
            .store(content_aspect.to_bits(), Ordering::Release);
        self.scale_factor_bits
            .store(scale_factor.to_bits(), Ordering::Release);
    }

    fn content_aspect(&self) -> f64 {
        f64::from_bits(self.content_aspect_bits.load(Ordering::Acquire))
    }

    fn scale_factor(&self) -> f64 {
        f64::from_bits(self.scale_factor_bits.load(Ordering::Acquire))
    }
}

const ASPECT_SUBCLASS_ID: usize = 1;

unsafe extern "system" fn aspect_ratio_subclass_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    _uid_subclass: usize,
    ref_data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::{LRESULT, RECT};
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::*;

    let state = &*(ref_data as *const WindowAspectState);

    match msg {
        WM_SIZING => {
            let rect = &mut *(lparam.0 as *mut RECT);
            let aspect = state.content_aspect();
            let scale = state.scale_factor();

            let skel_w = WindowConstant::SKELETON_W as f64 * scale;
            let skel_h = WindowConstant::SKELETON_H as f64 * scale;
            let min_w = WindowConstant::MIN_WIDTH * scale;

            let mut client_rect = RECT::default();
            let _ = GetClientRect(hwnd, &mut client_rect);
            let mut window_rect = RECT::default();
            let _ = GetWindowRect(hwnd, &mut window_rect);
            let chrome_w = ((window_rect.right - window_rect.left)
                - (client_rect.right - client_rect.left)) as f64;
            let chrome_h = ((window_rect.bottom - window_rect.top)
                - (client_rect.bottom - client_rect.top)) as f64;

            let edge = wparam.0 as u32;
            let width_driven = edge != WMSZ_TOP && edge != WMSZ_BOTTOM;

            if width_driven {
                let outer_w = ((rect.right - rect.left) as f64).max(min_w + chrome_w);
                let content_w = (outer_w - chrome_w - skel_w).max(1.0);
                let content_h = content_w / aspect;
                let target_h = (content_h + skel_h + chrome_h).round() as i32;

                if edge == WMSZ_LEFT || edge == WMSZ_TOPLEFT || edge == WMSZ_BOTTOMLEFT {
                    rect.left = rect.right - (outer_w.round() as i32);
                } else {
                    rect.right = rect.left + (outer_w.round() as i32);
                }

                if edge == WMSZ_TOPLEFT || edge == WMSZ_TOPRIGHT {
                    rect.top = rect.bottom - target_h;
                } else {
                    rect.bottom = rect.top + target_h;
                }
            } else {
                let outer_h = (rect.bottom - rect.top) as f64;
                let content_h = (outer_h - chrome_h - skel_h).max(1.0);
                let content_w = content_h * aspect;
                let target_w = ((content_w + skel_w + chrome_w).round() as i32)
                    .max((min_w + chrome_w).round() as i32);
                rect.right = rect.left + target_w;
            }

            return LRESULT(1);
        }
        WM_NCDESTROY => {
            let _ =
                RemoveWindowSubclass(hwnd, Some(aspect_ratio_subclass_proc), ASPECT_SUBCLASS_ID);
            drop(Arc::from_raw(ref_data as *const WindowAspectState));
        }
        _ => {}
    }

    DefSubclassProc(hwnd, msg, wparam, lparam)
}

pub struct AspectRatioEnforcer {
    aspect_state: Option<Arc<WindowAspectState>>,
}

impl AspectRatioEnforcer {
    pub fn new(window: &Window) -> Self {
        let mut enforcer = Self { aspect_state: None };
        enforcer.set_aspect_ratio(window, 16.0 / 9.0);
        enforcer
    }

    pub fn set_aspect_ratio(&mut self, window: &Window, content_aspect: f64) {
        set_windows_window_aspect_ratio(window, content_aspect, &mut self.aspect_state);
    }

    pub fn is_zoomed(&self, _window: &Window) -> bool {
        false
    }

    pub fn correct_aspect_after_resize(
        &self,
        window: &Window,
        logical_width: f64,
        logical_height: f64,
        img_aspect: f64,
        screen_area_extent: Extent,
        screen_area_position: Position,
    ) -> Option<(f64, f64)> {
        if img_aspect <= 0.0 {
            return None;
        }

        let content_w = logical_width - WindowConstant::SKELETON_W;
        let expected_h = content_w / img_aspect + WindowConstant::SKELETON_H;

        if (logical_height - expected_h).abs() <= 2.0 {
            return None;
        }

        let is_maximized = window.is_maximized();

        let (target_w, target_h) = if is_maximized {
            window.set_maximized(false);
            calculate_max_window_size(screen_area_extent, img_aspect)
                .unwrap_or((logical_width, expected_h))
        } else {
            (logical_width, expected_h)
        };

        log::info!(
            "AspectRatioEnforcer: aspect correction {:.1}x{:.1} -> {:.1}x{:.1}{}",
            logical_width,
            logical_height,
            target_w,
            target_h,
            if is_maximized { " (was maximized)" } else { "" }
        );

        if is_maximized {
            window.set_outer_position(winit::dpi::LogicalPosition::new(
                screen_area_position.x,
                screen_area_position.y,
            ));
        }

        Some((target_w, target_h))
    }
}

fn set_windows_window_aspect_ratio(
    window: &Window,
    content_aspect: f64,
    aspect_state: &mut Option<Arc<WindowAspectState>>,
) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::UI::Shell::SetWindowSubclass;

    let scale = window.scale_factor();

    if let Some(state) = aspect_state.as_ref() {
        state.set(content_aspect, scale);
    } else {
        let state = Arc::new(WindowAspectState {
            content_aspect_bits: AtomicU64::new(content_aspect.to_bits()),
            scale_factor_bits: AtomicU64::new(scale.to_bits()),
        });

        let Ok(raw_handle) = window.window_handle() else {
            log::warn!("set_windows_window_aspect_ratio: failed to get window handle");
            return;
        };
        let RawWindowHandle::Win32(handle) = raw_handle.as_raw() else {
            log::warn!("set_windows_window_aspect_ratio: not a Win32 handle");
            return;
        };

        let raw_ptr = Arc::into_raw(Arc::clone(&state));
        unsafe {
            let hwnd = windows::Win32::Foundation::HWND(handle.hwnd.get() as *mut _);
            let _ = SetWindowSubclass(
                hwnd,
                Some(aspect_ratio_subclass_proc),
                ASPECT_SUBCLASS_ID,
                raw_ptr as usize,
            );
        }
        *aspect_state = Some(state);
    }

    let (min_w, min_h) = min_window_size_for_aspect(content_aspect);
    window.set_min_inner_size(Some(winit::dpi::LogicalSize::new(min_w, min_h)));
}
