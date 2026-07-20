use super::{calculate_max_window_size, min_window_size_for_aspect, WindowConstant};
use crate::utils::geometry::{Extent, Position};
use winit::window::Window;

use objc2::MainThreadOnly;
use objc2_app_kit::{NSWindow, NSWindowDelegate};
use objc2_foundation::{NSObject, NSObjectProtocol};

objc2::define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = objc2::MainThreadOnly]
    struct MacosZoomDelegate;

    unsafe impl NSObjectProtocol for MacosZoomDelegate {}

    unsafe impl NSWindowDelegate for MacosZoomDelegate {
        #[unsafe(method(windowWillUseStandardFrame:defaultFrame:))]
        fn window_will_use_standard_frame_default_frame(
            &self,
            window: &NSWindow,
            default_frame: objc2_foundation::NSRect,
        ) -> objc2_foundation::NSRect {
            use objc2_foundation::{NSPoint, NSRect, NSSize};

            let frame = window.frame();
            let content_w = (frame.size.width - WindowConstant::SKELETON_W).max(1.0);
            let content_h = (frame.size.height - WindowConstant::SKELETON_H).max(1.0);
            let aspect = content_w / content_h;

            let visible_frame = if let Some(screen) = window.screen() {
                unsafe { objc2::msg_send![&*screen, visibleFrame] }
            } else {
                default_frame
            };

            let available = Extent {
                width: visible_frame.size.width,
                height: visible_frame.size.height,
            };

            let (width, height) =
                calculate_max_window_size(available, aspect).unwrap_or_else(|| {
                    let content_w = WindowConstant::DEFAULT_WIDTH - WindowConstant::SKELETON_W;
                    let content_h = content_w / aspect;
                    (
                        WindowConstant::DEFAULT_WIDTH,
                        content_h + WindowConstant::SKELETON_H,
                    )
                });

            NSRect::new(
                NSPoint::new(visible_frame.origin.x, visible_frame.origin.y),
                NSSize::new(width, height),
            )
        }
    }
);

impl MacosZoomDelegate {
    fn new() -> objc2::rc::Retained<Self> {
        let mtm =
            objc2::MainThreadMarker::new().expect("macOS zoom delegate must be on main thread");
        let this = Self::alloc(mtm);
        unsafe { objc2::msg_send![this, init] }
    }
}

pub struct AspectRatioEnforcer {
    _zoom_delegate: objc2::rc::Retained<MacosZoomDelegate>,
}

impl AspectRatioEnforcer {
    pub fn new(window: &Window) -> Self {
        let zoom_delegate = configure_macos_zoom_behavior(window);
        set_macos_window_aspect_ratio(window, 16.0 / 9.0);
        Self {
            _zoom_delegate: zoom_delegate,
        }
    }

    pub fn set_aspect_ratio(&mut self, window: &Window, content_aspect: f64) {
        set_macos_window_aspect_ratio(window, content_aspect);
    }

    pub fn is_zoomed(&self, window: &Window) -> bool {
        is_macos_window_zoomed(window)
    }

    pub fn correct_aspect_after_resize(
        &self,
        _window: &Window,
        _logical_width: f64,
        _logical_height: f64,
        _img_aspect: f64,
        _screen_area_extent: Extent,
        _screen_area_position: Position,
    ) -> Option<(f64, f64)> {
        None
    }
}

fn configure_macos_zoom_behavior(window: &Window) -> objc2::rc::Retained<MacosZoomDelegate> {
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSView, NSWindowCollectionBehavior};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let zoom_delegate = MacosZoomDelegate::new();

    let Ok(raw_handle) = window.window_handle() else {
        log::warn!("configure_macos_zoom_behavior: failed to get window handle");
        return zoom_delegate;
    };

    let RawWindowHandle::AppKit(handle) = raw_handle.as_raw() else {
        log::warn!("configure_macos_zoom_behavior: not an AppKit handle");
        return zoom_delegate;
    };

    unsafe {
        let ns_view: Option<Retained<NSView>> = Retained::retain(handle.ns_view.as_ptr().cast());
        let Some(ns_view) = ns_view else {
            return zoom_delegate;
        };
        let Some(ns_window) = ns_view.window() else {
            return zoom_delegate;
        };

        ns_window.setDelegate(Some(ProtocolObject::from_ref(&*zoom_delegate)));
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::FullScreenNone
                | NSWindowCollectionBehavior::FullScreenDisallowsTiling,
        );
        log::info!("configure_macos_zoom_behavior: installed custom zoom delegate");
    }

    zoom_delegate
}

fn is_macos_window_zoomed(window: &Window) -> bool {
    use objc2::rc::Retained;
    use objc2_app_kit::NSView;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(raw_handle) = window.window_handle() else {
        return false;
    };
    let RawWindowHandle::AppKit(handle) = raw_handle.as_raw() else {
        return false;
    };

    unsafe {
        let ns_view: Option<Retained<NSView>> = Retained::retain(handle.ns_view.as_ptr().cast());
        let Some(ns_view) = ns_view else {
            return false;
        };
        let Some(ns_window) = ns_view.window() else {
            return false;
        };
        let zoomed: bool = objc2::msg_send![&*ns_window, isZoomed];
        zoomed
    }
}

fn set_macos_window_aspect_ratio(window: &Window, content_aspect: f64) {
    use objc2::rc::Retained;
    use objc2_app_kit::NSView;
    use objc2_foundation::NSSize;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(raw_handle) = window.window_handle() else {
        log::warn!("set_macos_window_aspect_ratio: failed to get window handle");
        return;
    };

    let RawWindowHandle::AppKit(handle) = raw_handle.as_raw() else {
        log::warn!("set_macos_window_aspect_ratio: not an AppKit handle");
        return;
    };

    unsafe {
        let ns_view: Option<Retained<NSView>> = Retained::retain(handle.ns_view.as_ptr().cast());
        let Some(ns_view) = ns_view else {
            log::warn!("set_macos_window_aspect_ratio: failed to retain NSView");
            return;
        };
        let Some(ns_window) = ns_view.window() else {
            log::warn!("set_macos_window_aspect_ratio: NSView has no window");
            return;
        };

        let frame = ns_window.frame();
        let frame_w = frame.size.width;
        let content_w = frame_w - WindowConstant::SKELETON_W;
        let content_h = content_w / content_aspect;
        let frame_h = content_h + WindowConstant::SKELETON_H;

        ns_window.setAspectRatio(NSSize::new(frame_w, frame_h));

        let (min_w, min_h) = min_window_size_for_aspect(content_aspect);
        ns_window.setMinSize(NSSize::new(min_w, min_h));
    }
}
