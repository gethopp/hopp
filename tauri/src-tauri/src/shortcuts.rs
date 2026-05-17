use hopp::AppData;
use socket_lib::{CameraStartMessage, Message};
use std::sync::Mutex;
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub struct CallShortcuts {
    pub mic: String,
    pub camera: String,
    pub screenshare: String,
    pub end_call: String,
}

pub fn register_call_shortcuts(app: &tauri::AppHandle, shortcuts: CallShortcuts) {
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

    if let Err(e) =
        app.global_shortcut()
            .on_shortcut(shortcuts.camera.as_str(), |app, _sc, event| {
                if event.state() != ShortcutState::Pressed {
                    return;
                }
                handle_camera(app);
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
        if let Err(e) =
            app.global_shortcut()
                .on_shortcut(shortcuts.end_call.as_str(), |app, _sc, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    handle_end_call(app);
                })
        {
            log::error!(
                "register_call_shortcuts: failed to register end_call shortcut '{}': {e}",
                shortcuts.end_call
            );
        }
    }
}

pub fn unregister_call_shortcuts(app: &tauri::AppHandle) {
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

fn handle_camera(app: &tauri::AppHandle) {
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if data.is_camera_on {
        if let Err(e) = data.sender.send(Message::StopCamera) {
            log::error!("handle_camera shortcut (stop): {e}");
        }
    } else {
        let device_name = data.app_state.last_used_camera();
        if let Err(e) = data
            .sender
            .send(Message::StartCamera(CameraStartMessage { device_name }))
        {
            log::error!("handle_camera shortcut (start): {e}");
        }
    }
}

fn handle_end_call(app: &tauri::AppHandle) {
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::CallEnd) {
        log::error!("handle_end_call shortcut: {e}");
    }
}

fn handle_screenshare(app: &tauri::AppHandle) {
    let (is_screensharing, sender) = {
        let data = app.state::<Mutex<AppData>>();
        let data = data.lock().unwrap();
        (data.is_screensharing, data.sender.clone())
    };

    if is_screensharing {
        if let Err(e) = sender.send(Message::StopScreenshare) {
            log::error!("handle_screenshare shortcut (stop): {e}");
        }
    } else if let Err(e) = hopp::create_media_window(
        app,
        hopp::MediaWindowConfig {
            label: "contentPicker",
            title: "Content picker",
            url: "contentPicker.html",
            width: 800.0,
            height: 450.0,
            resizable: true,
            always_on_top: true,
            content_protected: false,
            maximizable: false,
            minimizable: true,
            decorations: true,
            transparent: false,
            background_color: None,
        },
    ) {
        log::error!("handle_screenshare shortcut (open picker): {e}");
    }
}
