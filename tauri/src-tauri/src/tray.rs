// Tray icon management for macOS.
//
// To alternate between light and dark icons, its pretty much impossible in MacOS.
// The menubar icons are not based on theme, but based on the wallpaper color,
// and there is no API to grab this, especially not an API call per monitor.
// Another fun fact is that the menubar icons can be different colors on different monitors.
//
// To solve this, we use template images for the base icon (auto adapts to light/dark per display),
// and a CALayer overlay for the colored notification dot when we are on a call.

#[cfg(target_os = "macos")]
pub mod macos {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use tauri::image::Image;
    use tauri::path::BaseDirectory;
    use tauri::tray::TrayIcon;
    use tauri::{AppHandle, Manager, Wry};

    /// State for the tray icon
    pub struct TrayState {
        pub tray_icon: TrayIcon<Wry>,
        pub app_handle: AppHandle,
        pub notification_enabled: AtomicBool,
    }

    static TRAY_STATE: OnceLock<Arc<Mutex<Option<TrayState>>>> = OnceLock::new();

    pub fn get_tray_state() -> &'static Arc<Mutex<Option<TrayState>>> {
        TRAY_STATE.get_or_init(|| Arc::new(Mutex::new(None)))
    }

    pub fn set_tray_state(state: TrayState) {
        let global = get_tray_state();
        let mut guard = global.lock().unwrap();
        *guard = Some(state);
    }

    /// Load a tray icon from bundled resources
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

    /// Add or remove a colored dot overlay on the tray icon button using CALayer.
    /// This preserves the template behavior of the base icon while adding color.
    fn update_notification_dot(show: bool) {
        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;
        use objc2::{msg_send, MainThreadMarker};
        use objc2_app_kit::NSStatusBar;
        use objc2_foundation::{NSPoint, NSRect, NSSize};
        use objc2_quartz_core::CALayer;

        unsafe {
            let Some(_mtm) = MainThreadMarker::new() else {
                log::warn!("[TRAY] update_notification_dot: not on main thread");
                return;
            };

            let status_bar = NSStatusBar::systemStatusBar();

            // Access status items via private API (NSPointerArray)
            let items: *const AnyObject =
                msg_send![&*status_bar, valueForKey: objc2_foundation::ns_string!("_statusItems")];
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

    /// Update the tray icon - always uses template for base icon
    pub fn update_tray_icon() {
        let state_arc = get_tray_state().clone();
        let guard = state_arc.lock().unwrap();

        if let Some(state) = guard.as_ref() {
            let notification_enabled = state.notification_enabled.load(Ordering::SeqCst);

            if let Some(icon) = load_tray_icon(&state.app_handle, "tray-dark-default.png") {
                let _ = state.tray_icon.set_icon(Some(icon));
                let _ = state.tray_icon.set_icon_as_template(true);
                update_notification_dot(notification_enabled);
            }
        }
    }

    /// Set the notification state and update the icon
    pub fn set_notification_enabled(enabled: bool) {
        {
            let state_guard = get_tray_state().lock().unwrap();
            if let Some(state) = state_guard.as_ref() {
                state.notification_enabled.store(enabled, Ordering::SeqCst);
            }
        }
        update_tray_icon();
    }

    pub fn is_notification_enabled() -> bool {
        let state_guard = get_tray_state().lock().unwrap();
        state_guard
            .as_ref()
            .map(|s| s.notification_enabled.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    /// No-op - template images handle appearance automatically per-display
    pub fn start_appearance_monitoring(_app_handle: AppHandle) {}

    pub fn stop_appearance_monitoring() {}
}

#[cfg(not(target_os = "macos"))]
pub mod fallback {
    use tauri::AppHandle;

    pub fn start_appearance_monitoring(_app_handle: AppHandle) {}
    pub fn stop_appearance_monitoring() {}
    pub fn set_notification_enabled(_enabled: bool) {}
    pub fn is_notification_enabled() -> bool {
        false
    }
    pub fn update_tray_icon() {}
}

#[cfg(target_os = "macos")]
pub use macos::*;

#[cfg(not(target_os = "macos"))]
pub use fallback::*;
