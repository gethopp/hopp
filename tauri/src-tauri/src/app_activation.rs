use crate::AppData;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_app_kit::NSApplicationDidBecomeActiveNotification;
use objc2_foundation::{NSNotification, NSNotificationCenter, NSObjectProtocol};
use socket_lib::Message;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
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
        tray_clicked: Arc<AtomicBool>,
    ) -> Self {
        let observer = unsafe {
            let center = NSNotificationCenter::defaultCenter();

            let bringing_to_front = Arc::new(AtomicBool::new(false));

            let block = block2::RcBlock::new(move |_notification: NonNull<NSNotification>| {
                log::info!("app_activation: received NSApplicationDidBecomeActiveNotification");

                // Guard: skip if activation was triggered by a tray click
                if tray_clicked.load(Ordering::Relaxed) {
                    log::info!("app_activation: tray_clicked flag set, skipping");
                    return;
                }

                // If the user directly clicked the main
                if app_handle
                    .get_webview_window("main")
                    .and_then(|w| w.is_focused().ok())
                    .unwrap_or(false)
                {
                    log::info!("app_activation: main window already focused, skipping");
                    return;
                }

                if let Some(window) = app_handle.get_webview_window("contentPicker") {
                    log::info!("app_activation: contentPicker is open focus there");
                    let _ = window.show();
                    let _ = window.set_focus();
                    return;
                }

                // Regular mode is either when permissions/notification windows are open, or when we are in a call.
                if app_handle
                    .state::<Mutex<AppData>>()
                    .lock()
                    .unwrap()
                    .activation_policy_regular
                {
                    log::info!("app_activation: activation_policy_regular is true, showing permissions window if exists");
                    if let Some(window) = app_handle.get_webview_window("permissions") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    } else if bringing_to_front.load(Ordering::Relaxed) {
                        log::info!(
                            "app_activation: BringWindowsToFront already in flight, skipping"
                        );
                    } else if app_handle
                        .get_webview_window("main")
                        .and_then(|w| w.is_focused().ok())
                        .unwrap_or(false)
                    {
                        log::info!(
                            "app_activation: main window focused, skipping BringWindowsToFront"
                        );
                    } else {
                        bringing_to_front.store(true, Ordering::Relaxed);
                        let data = app_handle.state::<Mutex<AppData>>();
                        let data = data.lock().unwrap();
                        if let Err(e) = data.sender.send(Message::BringWindowsToFront) {
                            log::error!(
                                "app_activation: failed to send BringWindowsToFront: {e:?}"
                            );
                        } else {
                            let focused = crate::recv_expected_response(
                                &data.event_socket,
                                |msg| match msg {
                                    Message::BringWindowsToFrontResult(f) => Ok(f),
                                    other => Err(other),
                                },
                            )
                            .unwrap_or(false);

                            if !focused {
                                log::info!("app_activation: BringWindowsToFront returned false, showing main window");
                                if let Some(window) = app_handle.get_webview_window("main") {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                            }
                        }
                        bringing_to_front.store(false, Ordering::Relaxed);
                    }
                    return;
                } else {
                    log::info!("app_activation: reset policy to accessory");
                    app_handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
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
