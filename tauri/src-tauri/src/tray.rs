// Tray icon management with platform-specific implementations.
//
// macOS: Uses template images for automatic light/dark adaptation per display,
// with a CALayer overlay for the colored notification dot during calls.
//
// Other platforms: No-op implementation (tray features not yet implemented).

use tauri::image::Image;
use tauri::path::BaseDirectory;
use tauri::tray::TrayIcon;
use tauri::{AppHandle, Manager, Wry};

// Platform-specific type alias
#[cfg(target_os = "macos")]
type PlatformTrayState = macos::MacOSTrayState;
#[cfg(not(target_os = "macos"))]
type PlatformTrayState = default::DefaultTrayState;

/// Platform-agnostic tray state manager.
pub struct TrayState {
    inner: PlatformTrayState,
}

impl TrayState {
    pub fn new(tray_icon: TrayIcon<Wry>) -> Self {
        Self {
            inner: PlatformTrayState::new(tray_icon),
        }
    }

    pub fn set_notification_enabled(&mut self, enabled: bool) {
        self.inner.set_notification_enabled(enabled);
    }

    pub fn is_notification_enabled(&self) -> bool {
        self.inner.is_notification_enabled()
    }
}

// =============================================================================
// Default (no-op) implementation for non-macOS platforms
// =============================================================================

#[cfg(not(target_os = "macos"))]
mod default {
    use super::*;

    pub struct DefaultTrayState {
        #[allow(dead_code)]
        tray_icon: TrayIcon<Wry>,
        notification_enabled: bool,
    }

    impl DefaultTrayState {
        pub fn new(tray_icon: TrayIcon<Wry>) -> Self {
            Self {
                tray_icon,
                notification_enabled: false,
            }
        }

        pub fn set_notification_enabled(&mut self, enabled: bool) {
            self.notification_enabled = enabled;
            // No-op: platform-specific tray features not implemented
        }

        pub fn is_notification_enabled(&self) -> bool {
            self.notification_enabled
        }
    }
}

// =============================================================================
// macOS implementation using CALayer for notification dot overlay
// =============================================================================

#[cfg(target_os = "macos")]
mod macos {
    use super::*;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2::{msg_send, MainThreadMarker};
    use objc2_app_kit::NSStatusBar;
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use objc2_quartz_core::CALayer;

    pub struct MacOSTrayState {
        #[allow(dead_code)]
        tray_icon: TrayIcon<Wry>,
        notification_enabled: bool,
    }

    impl MacOSTrayState {
        pub fn new(tray_icon: TrayIcon<Wry>) -> Self {
            Self {
                tray_icon,
                notification_enabled: false,
            }
        }

        pub fn set_notification_enabled(&mut self, enabled: bool) {
            self.notification_enabled = enabled;
            update_notification_dot(enabled);
        }

        pub fn is_notification_enabled(&self) -> bool {
            self.notification_enabled
        }
    }

    /// Add or remove a colored dot overlay on the tray icon button using CALayer.
    /// This preserves the template behavior of the base icon while adding color.
    fn update_notification_dot(show: bool) {
        unsafe {
            let Some(_mtm) = MainThreadMarker::new() else {
                log::warn!("[TRAY] update_notification_dot: not on main thread");
                return;
            };

            let status_bar = NSStatusBar::systemStatusBar();

            // Access status items via private API (NSPointerArray)
            let items: *const AnyObject = msg_send![&*status_bar, valueForKey: objc2_foundation::ns_string!("_statusItems")];
            if items.is_null() {
                return;
            }

            let count: usize = msg_send![items, count];

            // We might get >1 item for our app, but its filtered from the image selector.
            for i in 0..count {
                let item: *const AnyObject = msg_send![items, pointerAtIndex: i];
                if item.is_null() {
                    continue;
                }

                let button: *const AnyObject = msg_send![item, button];
                if button.is_null() {
                    continue;
                }

                // Only process items with a template image (our tray icon)
                let image: *const AnyObject = msg_send![button, image];
                if image.is_null() {
                    continue;
                }

                // Log if it's a template image
                let is_template: bool = msg_send![image, isTemplate];
                if !is_template {
                    continue;
                }

                let _: () = msg_send![button, setWantsLayer: true];
                let layer: *const AnyObject = msg_send![button, layer];
                if layer.is_null() {
                    continue;
                }

                let bounds: NSRect = msg_send![button, bounds];

                // Look for existing dot layer by name
                let dot_layer_name = objc2_foundation::ns_string!("notificationDot");
                let sublayers: *const AnyObject = msg_send![layer, sublayers];

                let mut existing_dot: *const AnyObject = std::ptr::null();
                if !sublayers.is_null() {
                    let sublayer_count: usize = msg_send![sublayers, count];
                    for j in 0..sublayer_count {
                        let sublayer: *const AnyObject = msg_send![sublayers, objectAtIndex: j];
                        let name: *const AnyObject = msg_send![sublayer, name];
                        if !name.is_null() {
                            let is_equal: bool =
                                msg_send![name, isEqualToString: &*dot_layer_name];
                            if is_equal {
                                existing_dot = sublayer;
                                break;
                            }
                        }
                    }
                }

                if show {
                    if existing_dot.is_null() {
                        let dot = CALayer::new();
                        let _: () = msg_send![&*dot, setName: &*dot_layer_name];

                        // Dot size and position (coordinates from top-left of button)
                        let dot_size: f64 = 4.0;
                        let dot_x: f64 = 10.5;
                        let dot_y: f64 = bounds.size.height - dot_size - 13.5;

                        let dot_frame = NSRect::new(
                            NSPoint::new(dot_x, dot_y),
                            NSSize::new(dot_size, dot_size),
                        );
                        dot.setFrame(dot_frame);

                        // Green color: #05df72
                        let ns_color_class = objc2::runtime::AnyClass::get(c"NSColor").unwrap();
                        let green_color: Retained<AnyObject> = msg_send![
                            ns_color_class,
                            colorWithSRGBRed: 0.02_f64,
                            green: 0.875_f64,
                            blue: 0.447_f64,
                            alpha: 1.0_f64
                        ];
                        let cg_color: *const AnyObject = msg_send![&*green_color, CGColor];

                        let _: () = msg_send![&*dot, setBackgroundColor: cg_color];
                        dot.setCornerRadius(dot_size / 2.0);

                        let _: () = msg_send![layer, addSublayer: &*dot];
                    } else {
                        let _: () = msg_send![existing_dot, setHidden: false];
                    }
                } else if !existing_dot.is_null() {
                    let _: () = msg_send![existing_dot, setHidden: true];
                }
            }
        }
    }
}

// =============================================================================
// Shared utilities
// =============================================================================

/// Load a tray icon from bundled resources.
/// Only used during initial setup in `setup_tray_icon()`.
pub fn load_tray_icon(app_handle: &AppHandle, filename: &str) -> Option<Image<'static>> {
    let icon_path = app_handle
        .path()
        .resolve(
            format!("resources/tray-icons/{}", filename),
            BaseDirectory::Resource,
        )
        .ok()?;

    let icon_bytes = std::fs::read(&icon_path).ok()?;
    Image::from_bytes(&icon_bytes).ok()
}
