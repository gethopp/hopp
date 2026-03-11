/// Shared macOS vibrancy (frosted glass) helper.
///
/// Inserts an `NSVisualEffectView` behind the wgpu Metal layer so the
/// desktop shows through transparent regions rendered by the GPU.

/// Pick the best transparent alpha mode from the surface capabilities.
///
/// On macOS, prefers `PreMultiplied` > `PostMultiplied` so the compositor
/// honours the alpha channel written by wgpu (required for vibrancy).
/// On other platforms returns `Auto`.
pub fn pick_transparent_alpha_mode(caps: &wgpu::SurfaceCapabilities) -> wgpu::CompositeAlphaMode {
    if cfg!(target_os = "macos") {
        if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
        {
            wgpu::CompositeAlphaMode::PreMultiplied
        } else if caps
            .alpha_modes
            .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
        {
            wgpu::CompositeAlphaMode::PostMultiplied
        } else {
            wgpu::CompositeAlphaMode::Auto
        }
    } else {
        wgpu::CompositeAlphaMode::Auto
    }
}

/// Apply macOS vibrancy (dark HUD frosted glass) to a winit window.
///
/// * `corner_radius` – rounded-corner mask radius (should match the window frame).
///
/// Uses the HudWindow material (dark frosted glass) with dark appearance forced.
/// Forces the NSWindow, content view and CAMetalLayer to be non-opaque so the
/// compositor honours the alpha channel written by wgpu.
/// Implementation based on:
/// https://stackoverflow.com/a/72299845
#[cfg(target_os = "macos")]
pub fn apply_macos_vibrancy(window: &winit::window::Window, corner_radius: f64) {
    use objc2::rc::Retained;
    use objc2::{msg_send, AnyThread, ClassType, MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{
        NSAppearance, NSAutoresizingMaskOptions, NSBezierPath, NSColor, NSImage, NSView,
        NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState,
        NSVisualEffectView, NSWindowOrderingMode,
    };
    use objc2_foundation::{NSEdgeInsets, NSPoint, NSRect, NSSize};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Some(mtm) = MainThreadMarker::new() else {
        log::warn!("apply_macos_vibrancy: not on main thread, skipping");
        return;
    };

    let Ok(raw_handle) = window.window_handle() else {
        log::warn!("apply_macos_vibrancy: failed to get window handle");
        return;
    };

    let RawWindowHandle::AppKit(handle) = raw_handle.as_raw() else {
        log::warn!("apply_macos_vibrancy: not an AppKit handle");
        return;
    };

    unsafe {
        let ns_view: Option<Retained<NSView>> = Retained::retain(handle.ns_view.as_ptr().cast());
        let Some(ns_view) = ns_view else {
            log::warn!("apply_macos_vibrancy: failed to retain NSView");
            return;
        };

        let Some(ns_window) = ns_view.window() else {
            log::warn!("apply_macos_vibrancy: NSView has no window");
            return;
        };
        ns_window.setOpaque(false);
        ns_window.setBackgroundColor(Some(&NSColor::clearColor()));

        let dark_name = objc2_foundation::ns_string!("NSAppearanceNameDarkAqua");
        if let Some(dark_appearance) = NSAppearance::appearanceNamed(dark_name) {
            let _: () = msg_send![&*ns_window, setAppearance: &*dark_appearance];
        }

        let layer: *mut objc2::runtime::AnyObject = msg_send![&*ns_view, layer];
        if !layer.is_null() {
            let _: () = msg_send![layer, setOpaque: false];
            log::info!("apply_macos_vibrancy: CAMetalLayer.isOpaque set to false");
        }

        let Some(frame_view) = ns_view.superview() else {
            log::warn!("apply_macos_vibrancy: content view has no superview");
            return;
        };

        let bounds = frame_view.bounds();

        let vibrancy_view: Retained<NSVisualEffectView> =
            msg_send![NSVisualEffectView::alloc(mtm), initWithFrame: bounds];

        vibrancy_view.setMaterial(NSVisualEffectMaterial(13)); // HudWindow
        vibrancy_view.setBlendingMode(NSVisualEffectBlendingMode(0)); // BehindWindow
        vibrancy_view.setState(NSVisualEffectState(1)); // Active
        vibrancy_view.setEmphasized(true);
        vibrancy_view.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );

        // Rounded-corner mask so vibrancy edges match the window frame.
        let mask_size = NSSize::new(corner_radius * 2.0, corner_radius * 2.0);
        let mask_image: Retained<NSImage> = msg_send![NSImage::alloc(), initWithSize: mask_size];

        let _: () = msg_send![&*mask_image, lockFocus];
        let mask_rect = NSRect::new(NSPoint::new(0.0, 0.0), mask_size);
        let path: Retained<NSBezierPath> = msg_send![
            NSBezierPath::class(),
            bezierPathWithRoundedRect: mask_rect,
            xRadius: corner_radius,
            yRadius: corner_radius
        ];
        let _: () = msg_send![&NSColor::blackColor(), set];
        let _: () = msg_send![&*path, fill];
        let _: () = msg_send![&*mask_image, unlockFocus];

        let insets = NSEdgeInsets {
            top: corner_radius,
            left: corner_radius,
            bottom: corner_radius,
            right: corner_radius,
        };
        let _: () = msg_send![&*mask_image, setCapInsets: insets];
        let _: () = msg_send![&*mask_image, setResizingMode: 1_isize]; // .stretch

        let _: () = msg_send![&*vibrancy_view, setMaskImage: &*mask_image];

        frame_view.addSubview_positioned_relativeTo(
            &vibrancy_view,
            NSWindowOrderingMode::Below,
            Some(&ns_view),
        );
    }

    log::info!("apply_macos_vibrancy: vibrancy applied successfully");
}
