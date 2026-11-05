use hopp::AppData;
use socket_lib::{CameraStartMessage, Message};
use std::sync::{Mutex, MutexGuard};
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

struct CallShortcuts {
    call_id: socket_lib::CallId,
    mic: String,
    camera: String,
    screenshare: String,
    end_call: String,
}

fn after_unlock<T, R>(data: MutexGuard<'_, T>, action: impl FnOnce() -> R) -> R {
    drop(data);
    action()
}

pub fn refresh_call_shortcuts(app: &tauri::AppHandle, data: MutexGuard<'_, AppData>) {
    let app_on_main = app.clone();
    if let Err(e) = after_unlock(data, || {
        app.run_on_main_thread(move || {
            let state = app_on_main.state::<Mutex<AppData>>();
            let data = state.lock().unwrap();
            let shortcuts = data
                .current_call_id
                .as_ref()
                .filter(|_| data.call_active)
                .map(|call_id| {
                    let mut settings = data.app_state.user_settings();
                    settings.resolve_shortcuts();
                    CallShortcuts {
                        call_id: call_id.clone(),
                        mic: settings.shortcut_toggle_mic.unwrap_or_default(),
                        camera: settings.shortcut_toggle_camera.unwrap_or_default(),
                        screenshare: settings.shortcut_toggle_screenshare.unwrap_or_default(),
                        end_call: settings.shortcut_end_call.unwrap_or_default(),
                    }
                });
            // Shortcut callbacks acquire AppData while holding the plugin's own lock.
            after_unlock(data, || {
                unregister_call_shortcuts(&app_on_main);
                if let Some(shortcuts) = shortcuts {
                    register_call_shortcuts(&app_on_main, shortcuts);
                }
            });
        })
    }) {
        log::error!("refresh_call_shortcuts: failed to schedule main-thread update: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::after_unlock;
    use std::sync::{Arc, Mutex};

    #[test]
    fn shortcut_dispatch_releases_application_lock_before_main_thread_work() {
        let state = Mutex::new(());
        let main_thread_can_lock = after_unlock(state.lock().unwrap(), || state.try_lock().is_ok());
        assert!(
            main_thread_can_lock,
            "shortcut dispatch holds AppData while waiting for main-thread work that needs it"
        );
    }

    #[test]
    fn queued_refresh_reads_state_after_call_end_or_settings_change() {
        for final_state in [None, Some("new shortcut")] {
            let state = Arc::new(Mutex::new(Some("old shortcut")));
            let observed = Arc::new(Mutex::new(None));
            let mut queued = None;
            let state_on_main = state.clone();
            let observed_on_main = observed.clone();
            after_unlock(state.lock().unwrap(), || {
                queued = Some(move || {
                    let data = state_on_main.lock().unwrap();
                    let shortcuts = *data;
                    after_unlock(data, || {
                        assert!(
                            state_on_main.try_lock().is_ok(),
                            "plugin update holds AppData while shortcut callbacks need it"
                        );
                        *observed_on_main.lock().unwrap() = Some(shortcuts);
                    });
                });
            });
            *state.lock().unwrap() = final_state;
            queued.unwrap()();
            assert_eq!(*observed.lock().unwrap(), Some(final_state));
        }
    }
}

fn register_call_shortcuts(app: &tauri::AppHandle, shortcuts: CallShortcuts) {
    if let Err(e) = app
        .global_shortcut()
        .on_shortcut(shortcuts.mic.as_str(), |app, _sc, event| {
            if event.state() != ShortcutState::Pressed {
                return;
            }
            handle_mic(app);
        })
    {
        log::error!(
            "register_call_shortcuts: failed to register mic shortcut '{}': {e}",
            shortcuts.mic
        );
    }

    let camera_call_id = shortcuts.call_id.clone();
    if let Err(e) =
        app.global_shortcut()
            .on_shortcut(shortcuts.camera.as_str(), move |app, _sc, event| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }
                handle_camera(app, &camera_call_id);
            })
    {
        log::error!(
            "register_call_shortcuts: failed to register camera shortcut '{}': {e}",
            shortcuts.camera
        );
    }

    if let Err(e) =
        app.global_shortcut()
            .on_shortcut(shortcuts.screenshare.as_str(), |app, _sc, event| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }
                handle_screenshare(app);
            })
    {
        log::error!(
            "register_call_shortcuts: failed to register screenshare shortcut '{}': {e}",
            shortcuts.screenshare
        );
    }

    if !shortcuts.end_call.is_empty() {
        if let Err(e) = app.global_shortcut().on_shortcut(
            shortcuts.end_call.as_str(),
            move |app, _sc, event| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }
                handle_end_call(app, &shortcuts.call_id);
            },
        ) {
            log::error!(
                "register_call_shortcuts: failed to register end_call shortcut '{}': {e}",
                shortcuts.end_call
            );
        }
    }
}

fn unregister_call_shortcuts(app: &tauri::AppHandle) {
    if let Err(e) = app.global_shortcut().unregister_all() {
        log::error!("unregister_call_shortcuts: {e}");
    }
}

fn handle_mic(app: &tauri::AppHandle) {
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::ToggleMic) {
        log::error!("handle_mic shortcut: {e}");
    }
}

fn handle_camera(app: &tauri::AppHandle, call_id: &str) {
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if data.current_call_id.as_deref() != Some(call_id) {
        return;
    }
    if data.is_camera_on {
        if let Err(e) = data.sender.send(Message::StopCamera(call_id.to_string())) {
            log::error!("handle_camera shortcut (stop): {e}");
        }
    } else {
        let device_name = data.app_state.last_used_camera();
        if let Err(e) = data.sender.send(Message::StartCamera(CameraStartMessage {
            call_id: call_id.to_string(),
            device_name,
        })) {
            log::error!("handle_camera shortcut (start): {e}");
        }
    }
}

fn handle_end_call(app: &tauri::AppHandle, call_id: &str) {
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if data.current_call_id.as_deref() != Some(call_id) {
        return;
    }
    if let Err(e) = data
        .sender
        .send(Message::CallEnd(Some(call_id.to_string())))
    {
        log::error!("handle_end_call shortcut: {e}");
    }
}

fn handle_screenshare(app: &tauri::AppHandle) {
    let (is_screensharing, sender) = {
        let data = app.state::<Mutex<AppData>>();
        let data = data.lock().unwrap();
        (data.is_screensharing, data.sender.clone())
    };

    let message = if is_screensharing {
        Message::StopScreenshare
    } else {
        Message::GetAvailableContent
    };

    if let Err(e) = sender.send(message) {
        log::error!("handle_screenshare shortcut: {e}");
    }
}
