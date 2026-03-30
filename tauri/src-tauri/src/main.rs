// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use hopp::sounds::{self, SoundConfig};
use log::LevelFilter;
use socket_lib::{
    AudioCaptureMessage, AudioDevice, CameraDevice, CaptureContent, Content, DrawingEnabled,
    Extent, Message, ScreenShareMessage, SentryMetadata,
};
use std::sync::mpsc as std_mpsc;
use tauri::Manager;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    path::BaseDirectory,
    Emitter,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

use tauri_plugin_log::{Target, TargetKind};

use hopp::{
    app_state::AppState, create_core_process, get_log_level, get_log_path, get_sentry_dsn,
    permissions, ping_frontend, recv_expected_response, setup_start_on_launch, setup_tray_icon,
    AppData,
};
use hopp::{disable_app_nap, set_window_corner_radius_and_decorations, CORNER_RADIUS};
#[cfg(target_os = "macos")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::{env, sync::Arc};

use std::time::Duration;

#[cfg(any(target_os = "windows", target_os = "linux"))]
use tauri::PhysicalPosition;

#[tauri::command(async)]
async fn screenshare(
    app: tauri::AppHandle,
    content: Content,
    resolution: Extent,
    accessibility_permission: bool,
) -> Result<(), String> {
    log::info!("screenshare: content: {content:?}, resolution: {resolution:?}");

    /*
     * If the user was previously a controller, we need to hide the viewing
     * window, to hide the delay from requesting the screen share to
     * screen share starting and the viewing window automatically being closed.
     */
    let window = app.get_webview_window("screenshare");
    if let Some(window) = window {
        log::info!("screenshare: closing window");
        let _ = window.hide();
    }

    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    if let Err(e) = data
        .sender
        .send(Message::StartScreenShare(ScreenShareMessage {
            content,
            resolution,
            accessibility_permission,
        }))
    {
        log::error!("screenshare: failed to send message: {e:?}");
        return Err("Failed to send message to hopp_core".to_string());
    }

    match recv_expected_response(&data.event_socket, |msg| match msg {
        Message::StartScreenShareResult(r) => Ok(r),
        other => Err(other),
    }) {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => {
            log::error!("screenshare: failed to start screenshare");
            Err(e)
        }
        Err(e) => {
            log::error!("screenshare: failed to receive message: {e:?}");
            Err("Failed to receive message from hopp_core".to_string())
        }
    }
}

#[tauri::command(async)]
async fn open_stats_window(app: tauri::AppHandle) {
    log::info!("open_stats_window");
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::OpenStatsWindow) {
        log::error!("open_stats_window: failed to send message: {e:?}");
    }
}

#[tauri::command(async)]
async fn stop_sharing(app: tauri::AppHandle) {
    log::info!("stop_sharing");
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::StopScreenshare) {
        log::error!("stop_sharing: failed to send message: {e:?}");
    }
}

#[tauri::command(async)]
async fn get_available_content(app: tauri::AppHandle) -> Vec<CaptureContent> {
    log::info!("get_available_content");
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::GetAvailableContent) {
        log::error!("get_available_content: failed to send message: {e:?}");
        return vec![];
    }
    match recv_expected_response(&data.event_socket, |msg| match msg {
        Message::AvailableContent(c) => Ok(c),
        other => Err(other),
    }) {
        Ok(content) => {
            for c in &content.content {
                log::info!(
                    "get_available_content: possible content {}, content {:?}",
                    c.title,
                    c.content
                );
            }
            content.content
        }
        Err(e) => {
            log::error!("get_available_content: recv failed: {e:?}");
            vec![]
        }
    }
}

#[tauri::command(async)]
fn play_sound(app: tauri::AppHandle, sound_name: String) {
    log::info!("play_sound");
    let tmp_sound_name = sound_name.split("/").last();
    if let Some(tmp_sound_name) = tmp_sound_name {
        log::info!("Playing sound: {}", tmp_sound_name);
    }
    /*
     * Check if the sound is already playing, if it has finished we
     * remove the entry from the sound_entries vector.
     */
    {
        let data = app.state::<Mutex<AppData>>();
        let mut data = data.lock().unwrap();
        let mut i = 0;
        while i < data.sound_entries.len() {
            if data.sound_entries[i].name == sound_name {
                /* Send a message to see if the sound is still playing */
                let res = data.sound_entries[i].tx.send(sounds::SoundCommand::Ping);
                if res.is_err() {
                    log::debug!("play_sound: found closed channel for {sound_name}");
                    data.sound_entries.remove(i);
                    break;
                }
                log::warn!("play_sound: Sound is already playing");
                return;
            } else {
                i += 1;
            }
        }
    }

    let sounds = hopp::sounds::get_all_sounds();
    let mut sound_path = "".to_string();
    let mut sound_config = SoundConfig::default();
    for sound in sounds {
        if sound.0.contains(&sound_name) {
            let resource_path = app.path().resolve(sound.0, BaseDirectory::Resource);
            if let Err(e) = resource_path {
                log::error!("play_sound: Failed to resolve sound path: {e:?}");
                return;
            }
            sound_path = resource_path.unwrap().to_string_lossy().to_string();
            sound_config = sound.1;
            break;
        }
    }
    if sound_path.is_empty() {
        log::error!("play_sound: Failed to find sound");
        return;
    }

    let (tx, rx) = std::sync::mpsc::channel();
    tauri::async_runtime::spawn(async move {
        let res = hopp::sounds::play_sound(sound_path, sound_config, rx);
        if res.is_err() {
            log::error!("play_sound: Failed to play sound: {:?}", res.err());
        }
    });

    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    data.sound_entries.push(sounds::SoundEntry {
        name: sound_name,
        tx,
    });
}

#[tauri::command(async)]
fn stop_sound(app: tauri::AppHandle, sound_name: String) {
    log::info!("stop_sound");
    let tmp_sound_name = sound_name.split("/").last();
    if let Some(tmp_sound_name) = tmp_sound_name {
        log::info!("Stopping sound: {}", tmp_sound_name);
    }
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    let mut i = 0;
    while i < data.sound_entries.len() {
        if data.sound_entries[i].name == sound_name {
            let _ = data.sound_entries[i].tx.send(sounds::SoundCommand::Stop);
            data.sound_entries.remove(i);
            break;
        } else {
            i += 1;
        }
    }
    log::debug!("stop_sound: entries left: {}", data.sound_entries.len());
}

#[tauri::command(async)]
fn reset_core_process(app: tauri::AppHandle) {
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::CallEnd) {
        log::error!("reset_core_process: failed to send message: {e:?}");
    }
}

#[tauri::command(async)]
fn store_token_cmd(app: tauri::AppHandle, token: String) {
    log::info!("store_token_cmd");
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    data.app_state.set_user_jwt(Some(token.clone()));

    if let Err(e) = app.emit("token_changed", token) {
        log::error!("Failed to emit token_changed event: {e:?}");
    }
}

#[tauri::command(async)]
fn get_stored_token(app: tauri::AppHandle) -> Option<String> {
    log::info!("get_stored_token");
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    let token = data.app_state.user_jwt().clone();
    log::debug!("get_stored_token: {token:?}");
    token
}

#[tauri::command(async)]
fn delete_stored_token(app: tauri::AppHandle) {
    log::info!("Deleting stored token");
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    data.app_state.set_user_jwt(None);

    if let Err(e) = app.emit("token_changed", "".to_string()) {
        log::error!("Failed to emit token_changed event: {e:?}");
    }
}

#[tauri::command(async)]
fn get_logs(_app: tauri::AppHandle) -> String {
    log::info!("get_logs:");
    let log_file = get_log_path();
    if let Some(path) = log_file {
        path.to_string_lossy().to_string()
    } else {
        log::error!("Failed to get log path");
        "".to_string()
    }
}

#[tauri::command(async)]
fn set_deactivate_hiding(app: tauri::AppHandle, deactivate: bool) {
    log::debug!("set_deactivate_hiding: {deactivate}");
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    let mut deactivate_hiding = data.deactivate_hiding.lock().unwrap();
    *deactivate_hiding = deactivate;
}

#[tauri::command(async)]
fn set_controller_cursor(app: tauri::AppHandle, enabled: bool) {
    log::info!("set_controller_cursor: {enabled}");
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::ControllerCursorEnabled(enabled)) {
        log::error!("set_controller_cursor: failed to send message: {e:?}");
    }
}

#[tauri::command(async)]
fn open_accessibility_settings(_app: tauri::AppHandle) {
    log::info!("open_accessibility_settings");
    let mut process = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn()
        .expect("Failed to open System Preferences for Accessibility permissions");
    let _ = process.wait();
}

#[tauri::command(async)]
fn open_microphone_settings(_app: tauri::AppHandle) {
    log::info!("open_microphone_settings");
    permissions::request_microphone();
}

#[tauri::command(async)]
fn open_camera_settings(_app: tauri::AppHandle) {
    log::info!("open_camera_settings");
    permissions::request_camera();
}

#[tauri::command(async)]
fn open_screenshare_settings(_app: tauri::AppHandle) {
    log::info!("open_screenshare_settings");
    let mut process = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture")
        .spawn()
        .expect("Failed to open System Preferences for Screen Capture permissions");
    let _ = process.wait();
}

#[tauri::command(async)]
async fn trigger_screenshare_permission(app: tauri::AppHandle) -> bool {
    log::info!("trigger_screenshare_permission");
    let content = get_available_content(app.clone()).await;
    let mut has_content = false;
    for c in content {
        if !c.base64.is_empty() {
            has_content = true;
            break;
        }
    }
    has_content
}

#[tauri::command(async)]
fn get_control_permission(_app: tauri::AppHandle) -> bool {
    let res = permissions::accessibility();
    log::info!("get_control_permission: {res}");
    res
}

#[tauri::command(async)]
fn get_microphone_permission(_app: tauri::AppHandle) -> bool {
    let res = permissions::microphone();
    log::info!("get_microphone_permission: {res}");
    res
}

#[tauri::command(async)]
fn get_screenshare_permission(_app: tauri::AppHandle) -> bool {
    let res = permissions::screenshare();
    log::info!("get_screenshare_permission: {res}");
    res
}

#[tauri::command(async)]
fn get_camera_permission(_app: tauri::AppHandle) -> bool {
    let res = permissions::camera();
    log::info!("get_camera_permission: {res}");
    res
}

#[tauri::command(async)]
fn skip_tray_notification_selection_window(app: tauri::AppHandle) {
    log::info!("executing skip_tray_notification_selection_window");
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    data.app_state.set_tray_notification(false);
}

#[allow(unused_variables)]
#[tauri::command(async)]
fn set_dock_icon_visible(app: tauri::AppHandle, visible: bool) {
    log::info!("set_dock_icon_visible: {visible}");
    #[cfg(target_os = "macos")]
    {
        if visible {
            let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
        } else {
            let content_picker_window = app.get_webview_window("contentPicker");
            if content_picker_window.is_none() {
                let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
        }

        {
            let data = app.state::<Mutex<AppData>>();
            let mut data = data.lock().unwrap();
            data.activation_policy_regular = visible;
        }
    }
}

#[tauri::command(async)]
fn get_last_used_mic(app: tauri::AppHandle) -> Option<String> {
    log::info!("get_last_used_mic");
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    let value = data.app_state.last_used_mic();
    log::info!("get_last_used_mic: {value:?}");
    value
}

#[tauri::command(async)]
fn set_last_used_mic(app: tauri::AppHandle, mic: String) {
    log::info!("set_last_used_mic: {mic}");
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    data.app_state.set_last_used_mic(mic);
}

#[tauri::command(async)]
fn get_last_used_camera(app: tauri::AppHandle) -> Option<String> {
    log::info!("get_last_used_camera");
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    let value = data.app_state.last_used_camera();
    log::info!("get_last_used_camera: {value:?}");
    value
}

#[tauri::command(async)]
fn set_last_used_camera(app: tauri::AppHandle, camera: String) {
    log::info!("set_last_used_camera: {camera}");
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    data.app_state.set_last_used_camera(camera);
}

#[tauri::command(async)]
fn get_sharer_draw_persist(app: tauri::AppHandle) -> bool {
    log::info!("get_sharer_draw_persist");
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    let value = data.app_state.sharer_draw_persist();
    log::info!("get_sharer_draw_persist: {value}");
    value
}

#[tauri::command(async)]
fn set_sharer_draw_persist(app: tauri::AppHandle, persist: bool) {
    log::info!("set_sharer_draw_persist: {persist}");
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    data.app_state.set_sharer_draw_persist(persist);
}

#[tauri::command(async)]
fn enable_drawing(app: tauri::AppHandle, permanent: bool) {
    log::info!("enable_drawing: permanent={permanent}");
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if let Err(e) = data
        .sender
        .send(Message::DrawingEnabled(DrawingEnabled { permanent }))
    {
        log::error!("enable_drawing: failed to send message: {e:?}");
    }
    drop(data);

    // Hide main window
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
}

#[tauri::command(async)]
fn minimize_main_window(app: tauri::AppHandle) {
    log::info!("minimize_main_window");
    if let Some(window) = app.get_webview_window("main") {
        if let Err(e) = window.minimize() {
            log::error!("Failed to minimize main window: {e:?}");
        }
    } else {
        log::error!("Main window not found");
    }
}

#[tauri::command(async)]
fn set_livekit_url(app: tauri::AppHandle, url: String) {
    log::info!("set_livekit_url");
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    if data.livekit_server_url != url {
        data.livekit_server_url = url.clone();
        if let Err(e) = data.sender.send(Message::LivekitServerUrl(url)) {
            log::error!("set_livekit_url: failed to send message: {e:?}");
        }
    }
}

#[tauri::command(async)]
fn get_livekit_url(app: tauri::AppHandle) -> String {
    log::info!("get_livekit_url");
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    data.livekit_server_url.clone()
}

#[tauri::command(async)]
async fn create_screenshare_window(
    app: tauri::AppHandle,
    video_token: String,
) -> Result<(), String> {
    let url = format!("screenshare.html?videoToken={}", video_token);
    hopp::create_media_window(
        &app,
        hopp::MediaWindowConfig {
            label: "screenshare",
            title: "Screen sharing",
            url: &url,
            width: 800.0,
            height: 450.0,
            resizable: true,
            always_on_top: false,
            content_protected: false,
            maximizable: false,
            minimizable: true,
            decorations: false,
            transparent: false,
            background_color: Some(tauri::webview::Color(0, 0, 0, 0)),
        },
    )
}

#[tauri::command(async)]
async fn create_camera_window(app: tauri::AppHandle, camera_token: String) -> Result<(), String> {
    log::info!("create_camera_window with token: {}", camera_token);

    let url = format!("camera.html?cameraToken={}", camera_token);
    hopp::create_media_window(
        &app,
        hopp::MediaWindowConfig {
            label: "camera",
            title: "Camera",
            url: &url,
            width: 160.0,
            height: 365.0,
            resizable: false,
            always_on_top: true,
            content_protected: true,
            maximizable: true,
            minimizable: true,
            decorations: false,
            transparent: true,
            background_color: None,
        },
    )
}

#[tauri::command(async)]
async fn create_content_picker_window(app: tauri::AppHandle) -> Result<(), String> {
    log::info!("create_content_picker_window");

    hopp::create_media_window(
        &app,
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
    )
}

#[tauri::command(async)]
fn set_sentry_metadata(app: tauri::AppHandle, user_email: String, app_version: String) {
    log::info!("set_sentry_metadata");
    sentry_utils::init_metadata(user_email.clone(), app_version.clone());
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::SentryMetadata(SentryMetadata {
        user_email,
        app_version,
    })) {
        log::error!("set_sentry_metadata: failed to send message: {e:?}");
    }
}

#[tauri::command(async)]
fn call_started(
    app: tauri::AppHandle,
    audio_token: String,
    video_token: String,
) -> Result<(), String> {
    log::info!("call_started");
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    #[cfg(target_os = "macos")]
    {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
        data.activation_policy_regular = true;
    }
    // Resolve the audio device name: last used → default → first → ""
    let audio_device_name = {
        let last_used = data.app_state.last_used_mic();
        let devices: Vec<AudioDevice> = if let Err(e) = data.sender.send(Message::ListAudioDevices)
        {
            log::error!("call_started: failed to list audio devices: {e:?}");
            vec![]
        } else {
            match recv_expected_response(&data.event_socket, |msg| match msg {
                Message::AudioDeviceList(d) => Ok(d),
                other => Err(other),
            }) {
                Ok(d) => d,
                Err(e) => {
                    log::error!("call_started: failed to receive audio device list: {e:?}");
                    vec![]
                }
            }
        };
        if let Some(last) = last_used {
            if devices.iter().any(|d| d.name == last) {
                last
            } else {
                devices
                    .iter()
                    .find(|d| d.default)
                    .or_else(|| devices.first())
                    .map(|d| d.name.clone())
                    .unwrap_or_default()
            }
        } else {
            devices
                .iter()
                .find(|d| d.default)
                .or_else(|| devices.first())
                .map(|d| d.name.clone())
                .unwrap_or_default()
        }
    };
    log::info!("call_started: resolved audio_device_name={audio_device_name:?}");
    if let Err(e) = data
        .sender
        .send(Message::CallStart(socket_lib::CallStartMessage {
            audio_token: audio_token.clone(),
            video_token: video_token.clone(),
            audio_device_name,
        }))
    {
        log::error!("call_started: failed to send: {e:?}");
        return Err("Failed to send message to hopp_core".to_string());
    }
    match recv_expected_response(&data.event_socket, |msg| match msg {
        Message::CallStartResult(r) => Ok(r),
        other => Err(other),
    }) {
        Ok(result) => result,
        Err(e) => {
            log::error!("call_started: recv failed: {e:?}");
            Err("Failed to receive message from hopp_core".to_string())
        }
    }
}

/// When enabled=true, shows the notification variant of the icon.
/// When enabled=false, shows the default variant.
#[tauri::command(async)]
fn set_tray_notification(app: tauri::AppHandle, enabled: bool) {
    log::info!("set_tray_notification: enabled={}", enabled);
    let data = app.state::<std::sync::Mutex<hopp::AppData>>();
    let mut data = data.lock().unwrap();
    if let Some(ref mut tray) = data.tray_state {
        tray.set_notification_enabled(enabled);
    }
}

#[tauri::command(async)]
fn get_hopp_server_url(app: tauri::AppHandle) -> Option<String> {
    log::info!("get_hopp_server_url");
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    let url = data.app_state.hopp_server_url();
    log::debug!("get_hopp_server_url: {url:?}");
    url
}

#[tauri::command(async)]
fn set_hopp_server_url(app: tauri::AppHandle, url: Option<String>) {
    log::info!("set_hopp_server_url: {url:?}");
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    data.app_state.set_hopp_server_url(url);
}

#[tauri::command(async)]
fn get_feedback_disabled(app: tauri::AppHandle) -> bool {
    log::info!("get_feedback_disabled");
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    data.app_state.feedback_disabled()
}

#[tauri::command(async)]
fn set_feedback_disabled(app: tauri::AppHandle, disabled: bool) {
    log::info!("set_feedback_disabled: {disabled}");
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    data.app_state.set_feedback_disabled(disabled);
}

#[tauri::command(async)]
async fn create_feedback_window(
    app: tauri::AppHandle,
    team_id: String,
    room_id: String,
    participant_id: String,
) -> Result<(), String> {
    log::info!("create_feedback_window");

    let url = format!(
        "feedback.html?teamId={}&roomId={}&participantId={}",
        team_id, room_id, participant_id
    );
    hopp::create_media_window(
        &app,
        hopp::MediaWindowConfig {
            label: "feedback",
            title: "Call Feedback",
            url: &url,
            width: 500.0,
            height: 420.0,
            resizable: false,
            always_on_top: true,
            content_protected: false,
            maximizable: false,
            minimizable: false,
            decorations: true,
            transparent: false,
            background_color: Some(tauri::webview::Color(0, 0, 0, 0)),
        },
    )
}

#[tauri::command(async)]
fn mute_mic(app: tauri::AppHandle) {
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::MuteAudio) {
        log::error!("mute_mic: failed to send: {e:?}");
    }
}

#[tauri::command(async)]
fn unmute_mic(app: tauri::AppHandle) {
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::UnmuteAudio) {
        log::error!("unmute_mic: failed to send: {e:?}");
    }
}

#[tauri::command(async)]
fn toggle_mic(app: tauri::AppHandle) {
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::ToggleMic) {
        log::error!("toggle_mic: failed to send: {e:?}");
    }
}

#[tauri::command(async)]
fn start_camera(app: tauri::AppHandle, device_name: Option<String>) -> Result<(), String> {
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    if let Err(e) = data
        .sender
        .send(Message::StartCamera(socket_lib::CameraStartMessage {
            device_name,
        }))
    {
        log::error!("start_camera: failed to send: {e:?}");
        return Err("Failed to send message to hopp_core".to_string());
    }
    match recv_expected_response(&data.event_socket, |msg| match msg {
        Message::StartCameraResult(r) => Ok(r),
        other => Err(other),
    }) {
        Ok(result) => result,
        Err(e) => {
            log::error!("start_camera: recv failed: {e:?}");
            Err("Failed to receive message from hopp_core".to_string())
        }
    }
}

#[tauri::command(async)]
fn stop_camera(app: tauri::AppHandle) {
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::StopCamera) {
        log::error!("stop_camera: failed to send: {e:?}");
    }
}

#[tauri::command(async)]
fn open_camera_preview(app: tauri::AppHandle) {
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::OpenCamera) {
        log::error!("open_camera_preview: failed to send: {e:?}");
    }
}

#[tauri::command(async)]
fn open_screenshare_viewer(app: tauri::AppHandle) {
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::OpenScreenShareWindow) {
        log::error!("open_screenshare_viewer: failed to send: {e:?}");
    }
}

#[tauri::command(async)]
fn close_screenshare_viewer(app: tauri::AppHandle) {
    let data = app.state::<Mutex<AppData>>();
    let data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::CloseScreenShareWindow) {
        log::error!("close_screenshare_viewer: failed to send: {e:?}");
    }
}

#[tauri::command(async)]
fn list_microphones(app: tauri::AppHandle) -> Vec<AudioDevice> {
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::ListAudioDevices) {
        log::error!("list_microphones: failed to send: {e:?}");
        return vec![];
    }
    match recv_expected_response(&data.event_socket, |msg| match msg {
        Message::AudioDeviceList(d) => Ok(d),
        other => Err(other),
    }) {
        Ok(devices) => devices,
        Err(e) => {
            log::error!("list_microphones: recv failed: {e:?}");
            vec![]
        }
    }
}

#[tauri::command(async)]
fn select_microphone(app: tauri::AppHandle, device_name: String) {
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    if let Err(e) = data
        .sender
        .send(Message::StartAudioCapture(AudioCaptureMessage {
            device_name,
        }))
    {
        log::error!("select_microphone: failed to send: {e:?}");
        return;
    }
    match recv_expected_response(&data.event_socket, |msg| match msg {
        Message::StartAudioCaptureResult(r) => Ok(r),
        other => Err(other),
    }) {
        Ok(Err(e)) => log::error!("select_microphone: core failed: {e}"),
        Err(e) => log::error!("select_microphone: no result: {e:?}"),
        Ok(Ok(())) => {}
    }
}

#[tauri::command(async)]
fn list_webcams(app: tauri::AppHandle) -> Vec<CameraDevice> {
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::ListCameras) {
        log::error!("list_webcams: failed to send: {e:?}");
        return vec![];
    }
    match recv_expected_response(&data.event_socket, |msg| match msg {
        Message::CameraList(d) => Ok(d),
        other => Err(other),
    }) {
        Ok(devices) => devices,
        Err(e) => {
            log::error!("list_webcams: recv failed: {e:?}");
            vec![]
        }
    }
}

#[tauri::command(async)]
fn bring_windows_to_front(app: tauri::AppHandle) -> bool {
    log::info!("bring_windows_to_front");
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::BringWindowsToFront) {
        log::error!("bring_windows_to_front: failed to send: {e:?}");
        return false;
    }
    match recv_expected_response(&data.event_socket, |msg| match msg {
        Message::BringWindowsToFrontResult(f) => Ok(f),
        other => Err(other),
    }) {
        Ok(focused) => focused,
        Err(e) => {
            log::error!("bring_windows_to_front: recv failed: {e:?}");
            false
        }
    }
}

#[tauri::command(async)]
fn end_call(app: tauri::AppHandle) {
    let data = app.state::<Mutex<AppData>>();
    let mut data = data.lock().unwrap();
    if let Err(e) = data.sender.send(Message::CallEnd) {
        log::error!("end_call: failed to send: {e:?}");
    }
    #[cfg(target_os = "macos")]
    {
        data.sleep_prevention.disable();
        let suppress = data.suppress_hide_on_call_end.clone();
        suppress.store(true, Ordering::Relaxed);
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
        data.activation_policy_regular = false;
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            suppress.store(false, Ordering::Relaxed);
        });
    }
}

#[tauri::command(async)]
fn toggle_call_sleep_prevention(app: tauri::AppHandle, enabled: bool) {
    #[cfg(target_os = "macos")]
    {
        let data = app.state::<Mutex<AppData>>();
        let mut data = data.lock().unwrap();
        if enabled {
            data.sleep_prevention.enable();
        } else {
            data.sleep_prevention.disable();
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, enabled);
    }
}

fn forward_core_events(events_rx: std_mpsc::Receiver<Message>, app: tauri::AppHandle) {
    log::info!("forward_core_events: starting event forwarding thread");
    for message in events_rx.iter() {
        match message {
            Message::ParticipantsSnapshot(snapshot) => {
                log::info!(
                    "forward_core_events: participants snapshot ({} participants)",
                    snapshot.len()
                );
                if let Err(e) = app.emit("core_participants_snapshot", &snapshot) {
                    log::error!("forward_core_events: failed to emit participants snapshot: {e:?}");
                }
            }
            Message::RoleChange(event) => {
                log::info!("forward_core_events: role change: {event:?}");
                if let Err(e) = app.emit("core_role_change", &event) {
                    log::error!("forward_core_events: failed to emit role change: {e:?}");
                }
            }
            Message::CameraFailed(error) => {
                log::error!("forward_core_events: camera failed: {error}");
                if let Err(e) = app.emit("core_camera_failed", &error) {
                    log::error!("forward_core_events: failed to emit camera failed: {e:?}");
                }
            }
            Message::CallEnded => {
                log::info!("forward_core_events: call ended");
                if let Err(e) = app.emit("core_call_ended", &()) {
                    log::error!("forward_core_events: failed to emit call ended: {e:?}");
                }
                #[cfg(target_os = "macos")]
                {
                    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
                    let data = app.state::<Mutex<AppData>>();
                    let mut data = data.lock().unwrap();
                    data.sleep_prevention.disable();
                    data.activation_policy_regular = false;
                    let suppress = data.suppress_hide_on_call_end.clone();
                    drop(data);
                    suppress.store(true, Ordering::Relaxed);
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                        suppress.store(false, Ordering::Relaxed);
                    });
                }
            }
            Message::ControllerDrawPersistChanged(persist) => {
                log::info!("forward_core_events: controller draw persist changed: {persist}");
                let data = app.state::<Mutex<AppData>>();
                let mut data = data.lock().unwrap();
                data.app_state.set_controller_draw_persist(persist);
            }
            Message::LastModeChanged(mode) => {
                log::info!("forward_core_events: last mode changed: {mode:?}");
                let data = app.state::<Mutex<AppData>>();
                let mut data = data.lock().unwrap();
                data.app_state.set_last_mode(mode);
            }
            Message::OpenContentPicker => {
                log::info!("forward_core_events: open content picker");
                if let Err(e) = hopp::create_media_window(
                    &app,
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
                    log::error!("forward_core_events: failed to open content picker: {e}");
                }
            }
            Message::RoomConnectionFailed(reason) => {
                log::error!("forward_core_events: room connection failed: {reason}");
                if let Err(e) = app.emit("core_room_connection_failed", &reason) {
                    log::error!(
                        "forward_core_events: failed to emit room connection failed: {e:?}"
                    );
                }
            }
            Message::QueryPreferredCamera => {
                log::info!("forward_core_events: query preferred camera");
                let data = app.state::<Mutex<AppData>>();
                let data = data.lock().unwrap();
                let preferred = data.app_state.last_used_camera();
                if let Err(e) = data.sender.send(Message::PreferredCamera(preferred)) {
                    log::error!("forward_core_events: failed to send preferred camera: {e:?}");
                }
            }
            other => {
                log::error!("forward_core_events: unhandled event: {other:?}");
            }
        }
    }
    log::info!("forward_core_events: event forwarding thread exiting");
}

fn main() {
    let _guard = sentry_utils::init_sentry("Tauri backend".to_string(), Some(get_sentry_dsn()));

    /*
     * Flag for disabling hiding the window on focus lost.
     * This is used to prevent the window from hiding when the user is writing feedback.
     */
    let deactivate_hiding = Arc::new(Mutex::new(false));
    let deactivate_hiding_clone = deactivate_hiding.clone();

    /*
     * Flag for disabling hiding the window on focus lost.
     * This is used to prevent the window from hiding when the user uses Raycast/Spotlight
     * to open the app again.
     */
    let reopen_requested = Arc::new(Mutex::new(false));
    #[allow(unused_variables)]
    let reopen_requested_clone = reopen_requested.clone();

    /* This is used to guard against showing the main window if the location is not set. */
    #[allow(unused_variables)]
    let location_set = Arc::new(Mutex::new(false));
    #[allow(unused_variables)]
    let location_set_clone = location_set.clone();
    #[allow(unused_variables)]
    let location_set_setup = location_set.clone();

    /* Flag set during tray icon clicks to suppress spurious activation events. */
    let tray_clicked = Arc::new(AtomicBool::new(false));

    /* Flag to suppress main window hide when activation policy switches to Accessory after a call ends. */
    let suppress_hide_on_call_end = Arc::new(AtomicBool::new(false));
    let suppress_hide_on_call_end_clone = suppress_hide_on_call_end.clone();

    let log_level = get_log_level();
    let mut app = tauri::Builder::default().plugin(tauri_plugin_opener::init());
    if !cfg!(debug_assertions) {
        app = app.plugin(tauri_plugin_single_instance::init(
            move |app, _args, _cwd| {
                log::info!("Reopening the app, single instance handler");
                log::debug!("app {app:?}");
                #[cfg(target_os = "macos")]
                {
                    let location_set = location_set_clone.lock().unwrap();
                    if !*location_set {
                        log::info!("Location not set, don't show the main window");
                        return;
                    }

                    let main_window = app.get_webview_window("main");
                    if let Some(window) = main_window {
                        log::info!("Single instance handler: showing main window");
                        let _ = window.show();
                        let _ = window.set_focus();
                    } else {
                        log::error!("Main window not found");
                    }
                }
            },
        ));
    }
    let log_file_name = if cfg!(debug_assertions) {
        Some("debug".to_string())
    } else {
        None
    };
    let app = app
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_positioner::init())
        .plugin(
            tauri_plugin_log::Builder::default()
                .targets([
                    Target::new(TargetKind::LogDir {
                        file_name: log_file_name,
                    }),
                    Target::new(TargetKind::Stdout),
                    Target::new(TargetKind::Webview),
                ])
                .level(LevelFilter::Warn)
                .level_for("hopp", log_level)
                .max_file_size(50 * 1024 * 1024) // We are emptying them on startup
                .build(),
        )
        .setup(move |app| {
            /* Create the app_data_dir if it doesn't exist. */
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data dir.");
            if !app_data_dir.exists() {
                if let Err(e) = std::fs::create_dir_all(&app_data_dir) {
                    log::error!("Failed to create app data dir: {e:?}");
                }
            }

            let (_core_process, sender, mut event_socket) =
                create_core_process(app.handle()).expect("Failed to create core process");

            let core_events_rx = event_socket.take_events();

            let app_state = AppState::new(&app_data_dir);
            if let Err(e) = sender.send(Message::ControllerDrawPersistChanged(app_state.controller_draw_persist())) {
                log::error!("Failed to send initial controller_draw_persist: {e:?}");
            }
            if let Some(mode) = app_state.last_mode() {
                if let Err(e) = sender.send(Message::LastModeChanged(mode)) {
                    log::error!("Failed to send initial last_mode: {e:?}");
                }
            }
            let data = Mutex::new(AppData::new(
                sender,
                event_socket,
                deactivate_hiding_clone,
                app_state,
                suppress_hide_on_call_end.clone(),
            ));
            app.manage(data);

            // Background thread to forward core events to the frontend
            let event_app_handle = app.handle().clone();
            std::thread::spawn(move || {
                forward_core_events(core_events_rx, event_app_handle);
            });

            let quit = MenuItemBuilder::new("Quit")
                .id("quit")
                .accelerator("Cmd+Q")
                .build(app)?;
            let menu = MenuBuilder::new(app).items(&[&quit]).build()?;

            setup_tray_icon(app, &menu, location_set_setup.clone(), tray_clicked.clone())?;

            /* Clear app logs in the beginning of a session. */
            let dir = app.path().app_log_dir();
            if let Err(e) = dir {
                log::warn!("Failed to get app log dir: {e:?}");
            } else {
                let dir = dir.unwrap();
                let log_file = dir.join("hopp.log");
                if log_file.exists() {
                    if let Err(e) = std::fs::write(&log_file, "") {
                        log::warn!("Failed to clear log file: {e:?}");
                    }
                }
            }

            /*
             * We are sending a ping event to the frontend
             * to keep it alive.
             * TODO: do graceful shutdown on exit
             */
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                ping_frontend(app_handle);
            });

            let first_run = {
                let data = app.state::<Mutex<AppData>>();
                let data = data.lock().unwrap();
                data.app_state.first_run()
            };

            setup_start_on_launch(&app.autolaunch(), first_run);

            /* Set first run to false after checking the start on launch. */
            {
                let data = app.state::<Mutex<AppData>>();
                let mut data = data.lock().unwrap();
                if first_run {
                    data.app_state.set_first_run(false);
                }
            }

            /* Main window configuration on windows */
            #[cfg(any(target_os = "windows", target_os = "linux"))]
            {
                let handle = app.handle();
                if let Some(window) = handle.get_webview_window("main") {
                    let _ = window.set_shadow(false);
                    let _ = window.set_skip_taskbar(false);
                    /* Place window on the bottom right corner of the active display. */
                    let current_monitor = window.current_monitor();
                    if let Ok(Some(monitor)) = current_monitor {
                        let monitor_size = monitor.size();
                        let monitor_pos = monitor.position();
                        let window_size = window.inner_size().unwrap();
                        let base_offset = 20 * monitor.scale_factor() as u32;
                        let offset_y = (25. * monitor.scale_factor()) as u32 + base_offset;
                        let x = monitor_pos.x
                            + (monitor_size.width - window_size.width - base_offset) as i32;
                        let y = monitor_pos.y
                            + (monitor_size.height - window_size.height - offset_y) as i32;
                        let new_position = PhysicalPosition::new(x as f64, y as f64);
                        let _ = window.set_position(new_position);
                    }
                    let _ = window.set_always_on_top(false);
                    let _ = window.show();
                }
            }

            /* macOS specific setup */
            #[cfg(target_os = "macos")]
            {
                disable_app_nap();
                /* Start as Accessory — switch to Regular during calls or when permission windows are visible */
                app.set_activation_policy(tauri::ActivationPolicy::Accessory);

                /*
                 * First show the notification window which explains that hopp lives in the
                 * menubar. Then show the permissions window if needed.
                 */
                let mut show_dock = false;
                let show_tray_notification_selection = {
                    let data = app.state::<Mutex<AppData>>();
                    let data = data.lock().unwrap();
                    data.app_state.tray_notification()
                };
                if show_tray_notification_selection {
                    let height = 250.;
                    let width = 450.;

                    let notification_window = tauri::WebviewWindowBuilder::new(
                        app,
                        "trayNotification",
                        tauri::WebviewUrl::App("trayNotification.html".into()),
                    )
                    .visible(true)
                    .focused(true)
                    .resizable(false)
                    .hidden_title(true)
                    .always_on_top(true)
                    .title_bar_style(tauri::TitleBarStyle::Overlay)
                    .title("Tray Notification")
                    .inner_size(width, height)
                    .build();
                    if let Err(e) = notification_window {
                        log::error!("Failed to create notification window: {e:?}");
                    } else {
                        let notification_window = notification_window.unwrap();
                        let _ = notification_window.show();
                        let _ = notification_window.set_focus();
                        show_dock = true;
                    }
                }

                if permissions::has_ungranted_permissions() {
                    log::info!("Opening permissions window");
                    let permissions_window = tauri::WebviewWindowBuilder::new(
                        app,
                        "permissions",
                        tauri::WebviewUrl::App("permissions.html".into()),
                    )
                    .visible(false)
                    .focused(true)
                    .resizable(false)
                    .hidden_title(true)
                    .always_on_top(false)
                    .title_bar_style(tauri::TitleBarStyle::Overlay)
                    .title("Permissions Configuration")
                    .inner_size(900., 730.)
                    .transparent(true)
                    .shadow(true)
                    .build();
                    if let Err(e) = permissions_window {
                        log::error!("Failed to create permissions window: {e:?}");
                    } else {
                        let permissions_window = permissions_window.unwrap();
                        show_dock = true;

                        // Apply native styling on macOS
                        #[cfg(target_os = "macos")]
                        {
                            set_window_corner_radius_and_decorations(
                                &permissions_window,
                                CORNER_RADIUS,
                                true,
                            );
                        }

                        /*
                         * Focus the window only if the notification window is not shown.
                         * When the notification window is shown we open the permissions window
                         * when it's closed.
                         */
                        if !show_tray_notification_selection {
                            let _ = permissions_window.show();
                            let _ = permissions_window.set_focus();
                        }
                    }
                }

                // Tackles Alt+Tab activation
                if show_dock {
                    app.set_activation_policy(tauri::ActivationPolicy::Regular);
                }
                {
                    let data = app.state::<Mutex<AppData>>();
                    let mut data = data.lock().unwrap();
                    if show_dock {
                        data.activation_policy_regular = true;
                    }
                    if !cfg!(debug_assertions) {
                        data.activation_observer =
                            Some(hopp::app_activation::AppActivationObserver::new(
                                app.handle().clone(),
                                location_set_setup.clone(),
                                reopen_requested_clone.clone(),
                                tray_clicked.clone(),
                            ));
                    }
                }
            }

            Ok(())
        })
        .on_window_event(move |window, event| {
            if let tauri::WindowEvent::Focused(is_focused) = event {
                #[cfg(any(target_os = "windows", target_os = "linux"))]
                if *is_focused && window.label() == "main" {
                    /* Place window on the bottom right corner of the active display. */
                    let current_monitor = window.current_monitor();
                    if let Ok(Some(monitor)) = current_monitor {
                        let monitor_size = monitor.size();
                        let monitor_pos = monitor.position();
                        let window_size = window.inner_size().unwrap();
                        let base_offset = 20 * monitor.scale_factor() as u32;
                        let offset_y = (25. * monitor.scale_factor()) as u32 + base_offset;
                        let x = monitor_pos.x
                            + (monitor_size.width - window_size.width - base_offset) as i32;
                        let y = monitor_pos.y
                            + (monitor_size.height - window_size.height - offset_y) as i32;
                        let new_position = PhysicalPosition::new(x as f64, y as f64);
                        let _ = window.set_position(new_position);
                    }
                }

                // detect click outside of the focused window and hide the app
                let deactivate_hiding = deactivate_hiding.lock().unwrap();
                let reopen_requested = reopen_requested.lock().unwrap();
                if !is_focused
                    && window.label() == "main"
                    && !cfg!(debug_assertions)
                    && !*deactivate_hiding
                    && !*reopen_requested
                    && !suppress_hide_on_call_end_clone.load(Ordering::Relaxed)
                {
                    log::info!("Hiding main window on focus lost: {}", *reopen_requested);

                    #[cfg(target_os = "macos")]
                    window.hide().unwrap();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            screenshare,
            stop_sharing,
            get_available_content,
            store_token_cmd,
            get_stored_token,
            delete_stored_token,
            play_sound,
            stop_sound,
            reset_core_process,
            get_logs,
            set_deactivate_hiding,
            set_controller_cursor,
            open_accessibility_settings,
            open_microphone_settings,
            open_screenshare_settings,
            trigger_screenshare_permission,
            get_control_permission,
            get_microphone_permission,
            get_screenshare_permission,
            skip_tray_notification_selection_window,
            set_last_used_mic,
            get_last_used_mic,
            set_last_used_camera,
            get_last_used_camera,
            get_sharer_draw_persist,
            set_sharer_draw_persist,
            enable_drawing,
            minimize_main_window,
            set_livekit_url,
            get_livekit_url,
            get_camera_permission,
            open_camera_settings,
            create_camera_window,
            create_screenshare_window,
            create_content_picker_window,
            set_sentry_metadata,
            call_started,
            set_tray_notification,
            get_hopp_server_url,
            set_hopp_server_url,
            get_feedback_disabled,
            set_feedback_disabled,
            create_feedback_window,
            mute_mic,
            unmute_mic,
            toggle_mic,
            list_microphones,
            select_microphone,
            list_webcams,
            start_camera,
            stop_camera,
            open_camera_preview,
            open_screenshare_viewer,
            close_screenshare_viewer,
            end_call,
            toggle_call_sleep_prevention,
            bring_windows_to_front,
            open_stats_window,
        ])
        .build(tauri::generate_context!())
        .expect("error while running tauri application");

    app.run(move |app_handle, event| match event {
        tauri::RunEvent::ExitRequested { .. } => {
            log::info!("Exit requested");
        }
        tauri::RunEvent::WindowEvent {
            label,
            event: tauri::WindowEvent::CloseRequested { .. },
            ..
        } => {
            log::info!("Close requested for window: {label}");
            if label == "trayNotification" {
                /* Make the permissions window visible in this case. */
                let permissions_window = app_handle.get_webview_window("permissions");
                if let Some(window) = permissions_window {
                    log::info!("Show permissions window");
                    let _ = window.show();
                    let _ = window.set_focus();
                } else {
                    #[cfg(target_os = "macos")]
                    {
                        let _ =
                            app_handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
                        app_handle
                            .state::<Mutex<AppData>>()
                            .lock()
                            .unwrap()
                            .activation_policy_regular = false;
                    }
                }
            } else if label == "permissions" {
                #[cfg(target_os = "macos")]
                {
                    let _ = app_handle.set_activation_policy(tauri::ActivationPolicy::Accessory);
                    app_handle
                        .state::<Mutex<AppData>>()
                        .lock()
                        .unwrap()
                        .activation_policy_regular = false;
                }
            }
        }
        _ => {}
    });
}
