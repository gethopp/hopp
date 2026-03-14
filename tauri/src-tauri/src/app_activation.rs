use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::NSApplicationDidBecomeActiveNotification;
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};
use tauri::Manager;

// SAFETY: The observer is an ObjC object registered on the main thread notification center.
// We only store it so it stays alive; we never access it from another thread.
struct SendSyncObserver(Retained<ProtocolObject<dyn NSObjectProtocol>>);
unsafe impl Send for SendSyncObserver {}
unsafe impl Sync for SendSyncObserver {}

pub struct AppActivationObserver {
    _observer: SendSyncObserver,
}

impl AppActivationObserver {
    pub fn new(
        app_handle: tauri::AppHandle,
        location_set: Arc<Mutex<bool>>,
        reopen_requested: Arc<Mutex<bool>>,
    ) -> Self {
        let observer = unsafe {
            let center = NSNotificationCenter::defaultCenter();

            let block = block2::RcBlock::new(move |_notification: NonNull<NSNotification>| {
                log::info!("app_activation: received NSApplicationDidBecomeActiveNotification");

                // If the user directly clicked the main
                if app_handle
                    .get_webview_window("main")
                    .and_then(|w| w.is_focused().ok())
                    .unwrap_or(false)
                {
                    log::info!("app_activation: {label} window already focused, skipping");
                    return;
                }

                // Guard: location must be set
                {
                    let is_location_set = location_set.lock().unwrap();
                    if !*is_location_set {
                        log::info!("app_activation: location not set, ignoring");
                        return;
                    }
                }

                // Guard: skip if reopen already in progress
                {
                    let is_reopen_in_progress = reopen_requested.lock().unwrap();
                    if *is_reopen_in_progress {
                        return;
                    }
                }

                // Set reopen flag
                {
                    let mut is_reopen_in_progress = reopen_requested.lock().unwrap();
                    *is_reopen_in_progress = true;
                }

                // Show screenshare window if exists, otherwise show main
                if let Some(window) = app_handle.get_webview_window("screenshare") {
                    let _ = window.show();
                    let _ = window.set_focus();
                } else if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                } else {
                    log::error!("app_activation: no window found to show");
                }

                Self::reset_reopen_requested_after_delay(reopen_requested.clone());
            });

            center.addObserverForName_object_queue_usingBlock(
                Some(NSApplicationDidBecomeActiveNotification),
                None,
                None,
                &block,
            )
        };

        Self {
            _observer: SendSyncObserver(observer),
        }
    }

    fn reset_reopen_requested_after_delay(reopen_in_progress: Arc<Mutex<bool>>) {
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            *reopen_in_progress.lock().unwrap() = false;
        });
    }
}
