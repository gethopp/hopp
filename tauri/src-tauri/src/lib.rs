#[cfg(target_os = "macos")]
pub mod app_activation;
pub mod app_state;
pub mod application_catalog;
pub mod permissions;
#[cfg(target_os = "macos")]
pub mod sleep_prevention;
pub mod sounds;
pub mod tray;

use log::LevelFilter;
use rand::{distributions::Alphanumeric, Rng};
use sounds::SoundEntry;
use std::env;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "macos")]
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
#[cfg(target_os = "macos")]
use std::time::Duration;
use tauri::async_runtime::Receiver;
#[cfg(target_os = "macos")]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, Wry};
#[cfg(target_os = "macos")]
use tauri::{Rect, TitleBarStyle, WebviewWindow};
use tauri_plugin_autostart::AutoLaunchManager;
use tauri_plugin_shell::{process::CommandChild, process::CommandEvent, ShellExt};

use socket_lib::{EventSocket, Message, SocketSender};
#[cfg(target_os = "macos")]
use tauri::{LogicalPosition, PhysicalPosition, PhysicalSize};

#[cfg(all(target_os = "macos", not(debug_assertions)))]
use smappservice_rs::*;

const PING_SLEEP_SECS: u64 = 30;
const PING_CORE_PROCESS_INTERVAL_SECS: u64 = 15;
pub const CORNER_RADIUS: f64 = 12.0;

#[derive(Debug, thiserror::Error)]
pub enum CoreProcessCreationError {
    #[error("Failed to create socket")]
    SocketCreationFailed,
    #[error("Failed to send message to core process")]
    SendMessageFailed,
}

/// Wrapper for the core process child handle.
pub struct CoreProcess {
    pub process: CommandChild,
}

struct PendingCallMedia {
    mic_enabled: bool,
    camera_enabled: bool,
}

/// Central application data structure that holds all the runtime state and resources
/// needed by the Tauri application.
pub struct AppData {
    /// Send half of the socket connection to the core process.
    pub sender: SocketSender,

    /// Receive half that routes messages into `events` and `responses` channels.
    pub event_socket: Arc<Mutex<EventSocket>>,

    /// Active sound entries currently being played by the application.
    /// Each entry contains the sound name and a channel transmitter to control playback.
    /// Used to prevent duplicate sounds and manage sound lifecycle.
    pub sound_entries: Vec<SoundEntry>,

    /// Flag to control whether the main window should hide when it loses focus.
    /// This is set to true when the user is writing feedback.
    pub deactivate_hiding: Arc<Mutex<bool>>,

    /// Persistent application state that survives across app restarts.
    /// Manages settings like first run status, tray notifications, and user preferences.
    pub app_state: app_state::AppState,

    /// Livekit server URL.
    pub livekit_server_url: String,

    /// Tray icon state. On macOS, contains the actual tray state.
    pub tray_state: Option<tray::TrayState>,

    /// Suppresses main window hide when activation policy is switched to Accessory after a call ends.
    pub suppress_hide_on_call_end: Arc<AtomicBool>,

    /// Whether drawing mode is currently enabled (runtime state).
    pub drawing_enabled: bool,

    /// Whether a call is currently active.
    pub call_active: bool,

    /// A failed core is unavailable until the application is restarted.
    core_failed: bool,

    /// Identifies pending and active calls so stale lifecycle events cannot clear a new call.
    pub current_call_id: Option<socket_lib::CallId>,

    pending_call_media: Option<PendingCallMedia>,
    camera_revision: u64,

    /// Whether the local camera is currently on (pushed from frontend snapshots).
    pub is_camera_on: bool,

    /// Whether the local participant is currently screensharing (pushed from frontend snapshots).
    pub is_screensharing: bool,

    /// macOS app activation observer — keeps the NSNotificationCenter observer alive.
    #[cfg(target_os = "macos")]
    pub activation_observer: Option<app_activation::AppActivationObserver>,

    /// Tracks whether the activation policy is currently Regular (true) or Accessory (false).
    #[cfg(target_os = "macos")]
    pub activation_policy_regular: bool,

    /// macOS sleep prevention state — holds an activity assertion while a call is active.
    #[cfg(target_os = "macos")]
    pub sleep_prevention: sleep_prevention::SleepPrevention,
}

impl AppData {
    pub fn new(
        sender: SocketSender,
        event_socket: EventSocket,
        deactivate_hiding: Arc<Mutex<bool>>,
        app_state: app_state::AppState,
        suppress_hide_on_call_end: Arc<AtomicBool>,
    ) -> Self {
        AppData {
            sender,
            event_socket: Arc::new(Mutex::new(event_socket)),
            sound_entries: Vec::new(),
            deactivate_hiding,
            app_state,
            livekit_server_url: "".to_string(),
            tray_state: None,
            #[cfg(target_os = "macos")]
            activation_observer: None,
            #[cfg(target_os = "macos")]
            activation_policy_regular: false,
            suppress_hide_on_call_end,
            drawing_enabled: false,
            call_active: false,
            core_failed: false,
            current_call_id: None,
            pending_call_media: None,
            camera_revision: 0,
            is_camera_on: false,
            is_screensharing: false,
            #[cfg(target_os = "macos")]
            sleep_prevention: sleep_prevention::SleepPrevention::new(),
        }
    }

    pub fn begin_call(&mut self, call_id: socket_lib::CallId) -> Result<(), String> {
        if self.core_failed {
            return Err(
                "Core is unavailable. Please restart Hopp before starting another call.".into(),
            );
        }
        let settings = self.app_state.user_settings();
        self.current_call_id = Some(call_id);
        self.call_active = false;
        self.pending_call_media = Some(PendingCallMedia {
            mic_enabled: settings.start_mic_on_call,
            camera_enabled: settings.start_camera_on_call,
        });
        self.camera_revision = self.camera_revision.wrapping_add(1);
        Ok(())
    }

    fn fail_core(&mut self) -> bool {
        if self.core_failed {
            return false;
        }
        self.core_failed = true;
        self.sender.shutdown();
        true
    }

    pub fn clear_call(&mut self) {
        self.current_call_id = None;
        self.call_active = false;
        self.is_camera_on = false;
        self.is_screensharing = false;
        self.pending_call_media = None;
        self.camera_revision = self.camera_revision.wrapping_add(1);
    }

    pub fn camera_enabled(&self) -> bool {
        self.pending_call_media
            .as_ref()
            .map_or(self.is_camera_on, |media| media.camera_enabled)
    }

    fn set_camera_intent(&mut self, call_id: &str, enabled: bool) -> Result<u64, String> {
        if self.current_call_id.as_deref() != Some(call_id) {
            return Err("Call is no longer active".into());
        }
        self.camera_revision = self.camera_revision.wrapping_add(1);
        if let Some(media) = self.pending_call_media.as_mut() {
            media.camera_enabled = enabled;
        }
        Ok(self.camera_revision)
    }

    pub fn send_media_command(&mut self, message: Message) -> Result<(), String> {
        match &message {
            Message::MuteAudio | Message::UnmuteAudio | Message::ToggleMic => {
                if let Some(media) = self.pending_call_media.as_mut() {
                    media.mic_enabled = match message {
                        Message::MuteAudio => false,
                        Message::UnmuteAudio => true,
                        _ => !media.mic_enabled,
                    };
                }
            }
            Message::StopCamera(call_id) => {
                self.set_camera_intent(call_id, false)?;
            }
            Message::StartCamera(camera) => {
                self.set_camera_intent(&camera.call_id, true)?;
                // Before CallStart, its media flags own camera startup.
                if self.pending_call_media.is_some() {
                    return match &camera.device_name {
                        Some(name) => self
                            .sender
                            .send(Message::SetPreferredCamera(Some(name.clone())))
                            .map_err(|error| error.to_string()),
                        None => Ok(()),
                    };
                }
            }
            _ => {}
        }
        self.sender.send(message).map_err(|error| error.to_string())
    }

    fn send_core_request(
        &mut self,
        event_socket: &Arc<Mutex<EventSocket>>,
        mut message: Message,
    ) -> Result<socket_lib::RequestId, String> {
        if self.core_failed {
            return Err("Core is unavailable. Please restart Hopp.".into());
        }
        if !Arc::ptr_eq(&self.event_socket, event_socket) {
            return Err("Core connection was replaced".into());
        }
        let call_id = match &message {
            Message::CallStart(message) => Some(&message.call_id),
            Message::StartCamera(message) => Some(&message.call_id),
            _ => None,
        };
        if call_id.is_some() && self.current_call_id.as_ref() != call_id {
            return Err("Call is no longer active".into());
        }
        let is_call_start = matches!(message, Message::CallStart(_));
        if let Message::CallStart(start) = &mut message {
            if let Some(media) = self.pending_call_media.as_ref() {
                start.start_mic_on_call = Some(media.mic_enabled);
                start.start_camera_on_call = Some(media.camera_enabled);
            }
        }
        let request_id = self
            .sender
            .send_request(message)
            .map_err(|error| error.to_string())?;
        if is_call_start {
            self.pending_call_media = None;
        }
        Ok(request_id)
    }
}

/// This sender is managed separately from AppData so stalled requests cannot delay quit.
pub fn end_core_before_exit(sender: &Mutex<SocketSender>) -> Result<(), std::io::Error> {
    let sender = sender.lock().unwrap().clone();
    sender.send(Message::CallEnd(None))?;
    sender.flush(std::time::Duration::from_secs(1))
}

/// Serialize request/reply pairs without holding application state while waiting.
pub fn request_core<T>(
    state: &Mutex<AppData>,
    message: Message,
    extract: impl Fn(Message) -> Result<T, Message>,
) -> Result<T, String> {
    let event_socket = state.lock().unwrap().event_socket.clone();
    let socket = event_socket.lock().unwrap();
    let request_id = send_core_request(state, &event_socket, message)?;
    recv_expected_response(&socket, request_id, extract).map_err(|error| error.to_string())
}

pub fn start_camera(
    state: &Mutex<AppData>,
    camera: socket_lib::CameraStartMessage,
) -> Result<(), String> {
    let (event_socket, revision) = {
        let mut data = state.lock().unwrap();
        if data.pending_call_media.is_some() {
            return data.send_media_command(Message::StartCamera(camera));
        }
        let revision = data.set_camera_intent(&camera.call_id, true)?;
        (data.event_socket.clone(), revision)
    };
    let socket = event_socket.lock().unwrap();
    let request_id = {
        let mut data = state.lock().unwrap();
        if data.camera_revision != revision {
            return Ok(());
        }
        data.send_core_request(&event_socket, Message::StartCamera(camera))?
    };
    let result = recv_expected_response(&socket, request_id, |message| match message {
        Message::StartCameraResult(result) => Ok(result),
        other => Err(other),
    })
    .map_err(|error| error.to_string());
    if state.lock().unwrap().camera_revision != revision {
        return Ok(());
    }
    result?
}

/// Caller holds this connection's response lock, acquired before the short AppData lock.
pub fn send_core_request(
    state: &Mutex<AppData>,
    event_socket: &Arc<Mutex<EventSocket>>,
    message: Message,
) -> Result<socket_lib::RequestId, String> {
    state
        .lock()
        .unwrap()
        .send_core_request(event_socket, message)
}

/// Receive the reply to this request, discarding unrelated or untagged responses.
/// Callers must serialize waits on this receiver. `extract` validates the reply variant.
pub fn recv_expected_response<T>(
    event_socket: &EventSocket,
    request_id: socket_lib::RequestId,
    extract: impl Fn(Message) -> Result<T, Message>,
) -> Result<T, std::sync::mpsc::RecvTimeoutError> {
    // TODO: 10 seconds is too much. We should make our calls to be as fast as possible
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(std::sync::mpsc::RecvTimeoutError::Timeout);
        }
        match event_socket.responses.recv_timeout(remaining) {
            Ok(Message::Response { request_id: id, message }) if id == request_id => match extract(*message) {
                Ok(val) => return Ok(val),
                Err(other) => {
                    let t = std::any::type_name::<T>();
                    log::error!(
                        "recv_expected_response<{t}>: unexpected response (stale?): {other:?}"
                    );
                    sentry_utils::upload_logs_event(format!(
                        "recv_expected_response<{t}>: unexpected response: {other:?}"
                    ));
                }
            },
            Ok(_) => log::debug!("Ignoring unrelated or untagged core response while waiting for request {request_id}"),
            Err(e) => return Err(e),
        }
    }
}

#[cfg(all(test, unix))]
mod response_tests {
    use super::*;
    use std::sync::mpsc;
    use std::sync::mpsc::RecvTimeoutError;
    use std::time::{Duration, Instant};

    fn test_connection() -> (Arc<Mutex<AppData>>, SocketSender, EventSocket) {
        let root = std::env::temp_dir().join(format!("hc-{}", rand::random::<u64>()));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("s").to_str().unwrap().to_string();
        let server_path = path.clone();
        let server = std::thread::spawn(move || socket_lib::listen(&server_path).unwrap());
        let deadline = Instant::now() + Duration::from_secs(2);
        let (sender, socket) = loop {
            match socket_lib::connect(&path) {
                Ok(pair) => break pair,
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("could not connect to simulated core: {error}"),
            }
        };
        let (peer, peer_socket) = server.join().unwrap();
        let mut app_state = app_state::AppState::new(&root);
        app_state.update_user_setting(|settings| {
            settings.start_mic_on_call = true;
            settings.start_camera_on_call = true;
        });
        let state = AppData::new(
            sender,
            socket,
            Arc::new(Mutex::new(false)),
            app_state,
            Arc::new(AtomicBool::new(false)),
        );
        std::fs::remove_dir_all(root).unwrap();
        (Arc::new(Mutex::new(state)), peer, peer_socket)
    }

    fn next_request(socket: &EventSocket) -> (socket_lib::RequestId, Message) {
        match socket.events.recv_timeout(Duration::from_secs(2)).unwrap() {
            Message::Request {
                request_id,
                message,
            } => (request_id, *message),
            other => panic!("expected a request, got {other:?}"),
        }
    }

    fn wait_for_camera_intent(state: &Mutex<AppData>, previous: u64) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while state.lock().unwrap().camera_revision == previous {
            assert!(
                Instant::now() < deadline,
                "camera intent was not registered"
            );
            std::thread::yield_now();
        }
    }

    #[test]
    fn camera_off_cancels_waiting_start_and_newer_on_wins() {
        for reenable in [false, true] {
            let (state, peer, peer_socket) = test_connection();
            state.lock().unwrap().current_call_id = Some("A".into());
            let event_socket = state.lock().unwrap().event_socket.clone();
            let responses = event_socket.lock().unwrap();
            let revision = state.lock().unwrap().camera_revision;
            let first_state = state.clone();
            let first = std::thread::spawn(move || {
                start_camera(
                    &first_state,
                    socket_lib::CameraStartMessage {
                        call_id: "A".into(),
                        device_name: Some("Old".into()),
                    },
                )
            });
            wait_for_camera_intent(&state, revision);
            {
                let mut data = state.lock().unwrap();
                data.send_media_command(Message::StopCamera("A".into()))
                    .unwrap();
            }
            assert!(matches!(
                peer_socket.events.recv_timeout(Duration::from_secs(2)).unwrap(),
                Message::StopCamera(id) if id == "A"
            ));
            let second = if reenable {
                let revision = state.lock().unwrap().camera_revision;
                let second_state = state.clone();
                let second = std::thread::spawn(move || {
                    start_camera(
                        &second_state,
                        socket_lib::CameraStartMessage {
                            call_id: "A".into(),
                            device_name: Some("New".into()),
                        },
                    )
                });
                wait_for_camera_intent(&state, revision);
                Some(second)
            } else {
                None
            };
            // A stale action from a previous call must not cancel the current intent.
            {
                let mut data = state.lock().unwrap();
                let revision = data.camera_revision;
                assert!(data
                    .send_media_command(Message::StopCamera("old call".into()))
                    .is_err());
                assert_eq!(data.camera_revision, revision);
            }
            drop(responses);
            if let Some(second) = second {
                let (request_id, message) = next_request(&peer_socket);
                assert!(matches!(message, Message::StartCamera(camera)
                    if camera.device_name.as_deref() == Some("New")));
                peer.reply(Some(request_id), Message::StartCameraResult(Ok(())))
                    .unwrap();
                second.join().unwrap().unwrap();
            }
            // Superseded starts resolve harmlessly, preserving a newer optimistic UI state.
            first.join().unwrap().unwrap();
            state
                .lock()
                .unwrap()
                .sender
                .flush(Duration::from_secs(1))
                .unwrap();
            assert!(
                peer_socket.events.try_recv().is_err(),
                "cancelled start reached core"
            );
        }
    }

    #[test]
    fn superseded_camera_failure_does_not_fail_a_newer_on_action() {
        let (state, peer, peer_socket) = test_connection();
        state.lock().unwrap().current_call_id = Some("A".into());
        let first_state = state.clone();
        let first = std::thread::spawn(move || {
            start_camera(
                &first_state,
                socket_lib::CameraStartMessage {
                    call_id: "A".into(),
                    device_name: Some("Missing".into()),
                },
            )
        });
        let (first_id, _) = next_request(&peer_socket);
        state
            .lock()
            .unwrap()
            .send_media_command(Message::StopCamera("A".into()))
            .unwrap();
        assert!(matches!(
            peer_socket
                .events
                .recv_timeout(Duration::from_secs(2))
                .unwrap(),
            Message::StopCamera(_)
        ));
        let revision = state.lock().unwrap().camera_revision;
        let second_state = state.clone();
        let second = std::thread::spawn(move || {
            start_camera(
                &second_state,
                socket_lib::CameraStartMessage {
                    call_id: "A".into(),
                    device_name: Some("New".into()),
                },
            )
        });
        wait_for_camera_intent(&state, revision);
        peer.reply(
            Some(first_id),
            Message::StartCameraResult(Err("camera missing".into())),
        )
        .unwrap();
        first.join().unwrap().unwrap();
        let (second_id, message) = next_request(&peer_socket);
        assert!(matches!(message, Message::StartCamera(camera)
            if camera.device_name.as_deref() == Some("New")));
        peer.reply(Some(second_id), Message::StartCameraResult(Ok(())))
            .unwrap();
        second.join().unwrap().unwrap();
        assert!(peer_socket.events.try_recv().is_err());
    }

    #[test]
    fn call_start_uses_media_actions_received_during_device_lookup() {
        for (toggle_mic, reenable) in [(false, false), (false, true), (true, false), (true, true)] {
            let (state, peer, peer_socket) = test_connection();
            state.lock().unwrap().begin_call("A".into()).unwrap();
            let startup_state = state.clone();
            let startup = std::thread::spawn(move || {
                let event_socket = startup_state.lock().unwrap().event_socket.clone();
                let socket = event_socket.lock().unwrap();
                let id =
                    send_core_request(&startup_state, &event_socket, Message::ListAudioDevices)
                        .unwrap();
                recv_expected_response(&socket, id, Ok).unwrap();
                let id = send_core_request(
                    &startup_state,
                    &event_socket,
                    Message::CallStart(socket_lib::CallStartMessage {
                        call_id: "A".into(),
                        audio_token: String::new(),
                        video_token: String::new(),
                        audio_device_name: String::new(),
                        start_mic_on_call: Some(true),
                        start_camera_on_call: Some(true),
                    }),
                )
                .unwrap();
                recv_expected_response(&socket, id, Ok).unwrap()
            });
            let (lookup_id, message) = next_request(&peer_socket);
            assert!(matches!(message, Message::ListAudioDevices));
            {
                let mut data = state.lock().unwrap();
                assert!(
                    data.camera_enabled(),
                    "shortcut must see the pending preference"
                );
                data.send_media_command(if toggle_mic {
                    Message::ToggleMic
                } else {
                    Message::MuteAudio
                })
                .unwrap();
                data.send_media_command(Message::StopCamera("A".into()))
                    .unwrap();
                assert!(!data.camera_enabled());
                if reenable {
                    data.send_media_command(if toggle_mic {
                        Message::ToggleMic
                    } else {
                        Message::UnmuteAudio
                    })
                    .unwrap();
                }
            }
            assert!(matches!(
                peer_socket
                    .events
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap(),
                Message::MuteAudio | Message::ToggleMic
            ));
            assert!(matches!(
                peer_socket
                    .events
                    .recv_timeout(Duration::from_secs(2))
                    .unwrap(),
                Message::StopCamera(_)
            ));
            if reenable {
                start_camera(
                    &state,
                    socket_lib::CameraStartMessage {
                        call_id: "A".into(),
                        device_name: Some("Selected camera".into()),
                    },
                )
                .unwrap();
                assert!(matches!(
                    peer_socket
                        .events
                        .recv_timeout(Duration::from_secs(2))
                        .unwrap(),
                    Message::UnmuteAudio | Message::ToggleMic
                ));
                assert!(
                    matches!(peer_socket.events.recv_timeout(Duration::from_secs(2)).unwrap(),
                    Message::SetPreferredCamera(Some(name)) if name == "Selected camera")
                );
            }
            // Let lookup finish only after controls have changed the pending intent.
            peer.reply(Some(lookup_id), Message::AudioDeviceList(vec![]))
                .unwrap();
            let (start_id, message) = next_request(&peer_socket);
            match message {
                Message::CallStart(start) => {
                    assert_eq!(start.start_mic_on_call, Some(reenable));
                    assert_eq!(start.start_camera_on_call, Some(reenable));
                }
                other => panic!("expected CallStart, got {other:?}"),
            }
            peer.reply(Some(start_id), Message::CallStartResult(Ok(())))
                .unwrap();
            startup.join().unwrap();
            assert!(
                peer_socket.events.try_recv().is_err(),
                "startup sent a duplicate camera start"
            );
            {
                let mut data = state.lock().unwrap();
                assert!(data.pending_call_media.is_none());
                data.clear_call();
                data.begin_call("B".into()).unwrap();
                assert!(data.camera_enabled());
                assert!(data.pending_call_media.as_ref().unwrap().mic_enabled);
            }
        }
    }

    #[test]
    fn pending_replies_leave_app_state_and_quit_available() {
        let (state, peer, peer_socket) = test_connection();
        let exit_sender = Mutex::new(state.lock().unwrap().sender.clone());
        state.lock().unwrap().current_call_id = Some("A".into());
        let request_state = state.clone();
        let first =
            std::thread::spawn(move || request_core(&request_state, Message::ListAudioDevices, Ok));
        let request_id = match peer_socket
            .events
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
        {
            Message::Request { request_id, .. } => request_id,
            other => panic!("expected a request, got {other:?}"),
        };
        let second_state = state.clone();
        let second =
            std::thread::spawn(move || request_core(&second_state, Message::ListCameras, Ok));
        assert!(matches!(
            peer_socket.events.recv_timeout(Duration::from_millis(50)),
            Err(RecvTimeoutError::Timeout)
        ));
        let deadline = Instant::now() + Duration::from_secs(1);
        let mut data = loop {
            if let Ok(data) = state.try_lock() {
                break data;
            }
            assert!(Instant::now() < deadline, "reply wait holds AppData");
            std::thread::yield_now();
        };
        // Exercise the actual exit helper while AppData is held and core hasn't replied.
        end_core_before_exit(&exit_sender).unwrap();
        assert!(matches!(
            peer_socket
                .events
                .recv_timeout(Duration::from_secs(1))
                .unwrap(),
            Message::CallEnd(None)
        ));
        data.current_call_id = None;
        drop(data);
        peer.reply(Some(request_id), Message::AudioDeviceList(vec![]))
            .unwrap();
        assert!(matches!(
            first.join().unwrap().unwrap(),
            Message::AudioDeviceList(_)
        ));
        let second_id = match peer_socket
            .events
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
        {
            Message::Request {
                request_id,
                message,
            } => {
                assert!(matches!(*message, Message::ListCameras));
                request_id
            }
            other => panic!("expected a request, got {other:?}"),
        };
        peer.reply(Some(second_id), Message::CameraList(vec![]))
            .unwrap();
        assert!(matches!(
            second.join().unwrap().unwrap(),
            Message::CameraList(_)
        ));
        // The device lookup completed after hang-up; it must not dispatch CallStart(A).
        let event_socket = state.lock().unwrap().event_socket.clone();
        let _responses = event_socket.lock().unwrap();
        assert!(send_core_request(
            &state,
            &event_socket,
            Message::CallStart(socket_lib::CallStartMessage {
                call_id: "A".into(),
                audio_token: String::new(),
                video_token: String::new(),
                audio_device_name: String::new(),
                start_mic_on_call: None,
                start_camera_on_call: None,
            })
        )
        .is_err());
        assert!(peer_socket.events.try_recv().is_err());
    }

    #[test]
    fn core_failure_interrupts_waits_and_blocks_calls_after_cleanup() {
        let (state, _peer, peer_socket) = test_connection();
        state.lock().unwrap().begin_call("A".into()).unwrap();
        let request_state = state.clone();
        let (done, completed) = mpsc::channel();
        let request = std::thread::spawn(move || {
            done.send(request_core(&request_state, Message::ListAudioDevices, Ok))
                .unwrap();
        });
        next_request(&peer_socket);
        assert!(state.lock().unwrap().fail_core());
        assert!(!state.lock().unwrap().fail_core());
        assert!(completed
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .is_err());
        request.join().unwrap();

        let mut data = state.lock().unwrap();
        let socket = data.event_socket.clone();
        assert!(data
            .send_core_request(&socket, Message::ListCameras)
            .is_err());
        // Ending the failed call must not make the dead core available again.
        data.clear_call();
        assert!(data
            .begin_call("B".into())
            .unwrap_err()
            .contains("restart Hopp"));
        assert!(data.current_call_id.is_none());
        assert!(data.pending_call_media.is_none());
    }

    #[test]
    fn replaced_connection_interrupts_old_waits_and_rejects_stale_requests() {
        let (state, _peer, peer_socket) = test_connection();
        let old_socket = state.lock().unwrap().event_socket.clone();
        let request_state = state.clone();
        let (done, completed) = mpsc::channel();
        let request = std::thread::spawn(move || {
            done.send(request_core(&request_state, Message::ListAudioDevices, Ok))
                .unwrap();
        });
        peer_socket
            .events
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        let (replacement, _new_peer, new_peer_socket) = test_connection();
        {
            let mut data = state.lock().unwrap();
            let new_data = replacement.lock().unwrap();
            data.sender.shutdown();
            data.sender = new_data.sender.clone();
            data.event_socket = new_data.event_socket.clone();
            data.current_call_id = Some("B".into());
        }
        assert!(completed
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .is_err());
        request.join().unwrap();
        assert!(send_core_request(&state, &old_socket, Message::ListCameras).is_err());
        let current_socket = state.lock().unwrap().event_socket.clone();
        let _responses = current_socket.lock().unwrap();
        for call_id in ["A", "B"] {
            let result = send_core_request(
                &state,
                &current_socket,
                Message::StartCamera(socket_lib::CameraStartMessage {
                    call_id: call_id.into(),
                    device_name: None,
                }),
            );
            assert_eq!(result.is_ok(), call_id == "B");
        }
        assert!(matches!(
            new_peer_socket.events.recv_timeout(Duration::from_secs(1)).unwrap(),
            Message::Request { message, .. } if matches!(&*message,
                Message::StartCamera(message) if message.call_id == "B")
        ));
        assert!(new_peer_socket.events.try_recv().is_err());
    }

    #[test]
    fn late_response_does_not_complete_next_request_of_same_type() {
        let path = std::env::temp_dir().join(format!("hopp-replies-{}.sock", std::process::id()));
        let socket_path = path.to_str().unwrap().to_string();
        let core_path = socket_path.clone();
        let core = std::thread::spawn(move || {
            let (sender, socket) = socket_lib::listen(&core_path).unwrap();
            // Hold A's reply until A times out and B has been sent.
            let mut request_ids = Vec::new();
            for _ in 0..2 {
                match socket.events.recv_timeout(Duration::from_secs(30)).unwrap() {
                    Message::Request {
                        request_id,
                        message,
                    } => {
                        assert!(matches!(*message, Message::ListAudioDevices));
                        request_ids.push(request_id);
                    }
                    other => panic!("expected a tagged device-list request, got {other:?}"),
                }
            }
            assert_ne!(request_ids[0], request_ids[1]);
            sender
                .send(Message::AudioDeviceList(vec![socket_lib::AudioDevice {
                    name: "unrelated untagged response".into(),
                    default: true,
                }]))
                .unwrap();
            for (request_id, name) in request_ids
                .into_iter()
                .zip(["timed-out request A", "current request B"])
            {
                sender
                    .reply(
                        Some(request_id),
                        Message::AudioDeviceList(vec![socket_lib::AudioDevice {
                            name: name.to_string(),
                            default: true,
                        }]),
                    )
                    .unwrap();
            }
            sender.flush(Duration::from_secs(1)).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let (sender, socket) = loop {
            match socket_lib::connect(&socket_path) {
                Ok(connection) => break connection,
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("could not connect to simulated core: {error}"),
            }
        };
        let extract = |message| match message {
            Message::AudioDeviceList(devices) => Ok(devices),
            other => Err(other),
        };

        let request_a = sender.send_request(Message::ListAudioDevices).unwrap();
        assert!(matches!(
            recv_expected_response(&socket, request_a, extract),
            Err(RecvTimeoutError::Timeout)
        ));

        let request_b = sender.send_request(Message::ListAudioDevices).unwrap();
        let devices = recv_expected_response(&socket, request_b, extract).unwrap();
        core.join().unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(
            devices[0].name, "current request B",
            "a late reply to timed-out request A must not satisfy request B"
        );
    }
}

/// Stop using the failed connection; frontend cleanup ends the call without waiting for core.
pub fn core_process_failed(app: &AppHandle, reason: String) {
    if let Some(state) = app.try_state::<Mutex<AppData>>() {
        let mut data = state.lock().unwrap();
        if !data.fail_core() {
            return;
        }
    }
    if let Err(error) = app.emit("core_process_crashed", reason) {
        log::error!("Failed to emit core_process_crashed: {error:?}");
    }
}

/// Monitors core process output and emits crash events.
async fn show_stdout(mut receiver: Receiver<CommandEvent>, app_handle: AppHandle) {
    let mut crash_msg = "Core stopped unexpectedly. Please restart Hopp.".to_string();
    while let Some(event) = receiver.recv().await {
        match event {
            CommandEvent::Stdout(line) => {
                log::info!("{}", String::from_utf8(line).unwrap_or_default());
            }
            CommandEvent::Stderr(line) => {
                /* For some reason the sidecar process logs to stderr. */
                log::info!("{}", String::from_utf8(line).unwrap_or_default());
            }
            CommandEvent::Terminated(payload) => {
                log::error!("show_stdout: Terminated {payload:?}");
                match payload.code {
                    Some(code) => {
                        if code == 1 {
                            crash_msg = "Core process terminated because it failed to receive messages from tauri, please restart the app".to_string();
                        } else if code == 2 {
                            crash_msg = "Screen capture failed. Please restart Hopp.".to_string();
                        } else if code == 3 {
                            crash_msg = "Core process terminated because the event loop stopped responding, please restart the app".to_string();
                            sentry_utils::upload_logs_event(
                                "Hang protection triggered".to_string(),
                            );
                        }
                    }
                    None => {
                        crash_msg = "Core process terminated because of an unknown error. Please restart the app, please submit a bug report".to_string();
                        sentry_utils::upload_logs_event("?Unknown crash".to_string());
                    }
                }
                break;
            }
            CommandEvent::Error(e) => {
                log::error!("show_stdout: Error: {e:?}");
                break;
            }
            _ => {}
        }
    }
    log::info!("show_stdout: Finished");

    core_process_failed(&app_handle, crash_msg);
}

/// Spawns the core process sidecar with required arguments.
fn start_sidecar(
    app: &tauri::AppHandle,
    socket_path: &str,
) -> (Receiver<CommandEvent>, CommandChild) {
    log::info!("start_sidecar:");

    /* First we check if the process is already running and kill it. */
    if !cfg!(debug_assertions) {
        let system = sysinfo::System::new_all();
        for process in system.processes().values() {
            if let Some(name) = process.name().to_str() {
                if name.contains("hopp_core") {
                    log::info!("start_sidecar: Found running core process, killing it");
                    let _ = process.kill();
                }
            }
        }
    }

    let mut args = vec!["--socket-path", socket_path];

    let sentry_dsn = get_sentry_dsn();
    if !cfg!(debug_assertions) {
        args.push("--sentry-dsn");
        args.push(&sentry_dsn);
    }

    let mut hopp_core_name = "hopp_core".to_string();
    if cfg!(debug_assertions) {
        hopp_core_name = format!("hopp_core{}", env::var("HOPP_SUFFIX").unwrap_or_default());
    }
    let command = app.shell().sidecar(hopp_core_name).unwrap().args(args);
    let (rx, child) = command.spawn().expect("Failed to spawn sidecar");
    (rx, child)
}

/// Creates a socket connection to communicate with the core process.
fn create_core_process_socket(
    socket_path: &str,
) -> Result<(SocketSender, EventSocket), CoreProcessCreationError> {
    let max_tries = 20;
    let mut tries = 0;
    loop {
        match socket_lib::connect(socket_path) {
            Ok(pair) => return Ok(pair),
            Err(_) => {
                log::debug!("create_core_process_socket: Failed to connect, retrying in 1 second");
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
        tries += 1;
        if tries >= max_tries {
            log::error!(
                "create_core_process_socket: Failed to create socket after {max_tries} tries"
            );
            break;
        }
    }
    Err(CoreProcessCreationError::SocketCreationFailed)
}

/// We send this in order to stop the core process from timing out.
/// This is used for killing the core process in case the tauri app
/// has crashed.
async fn send_ping(sender: SocketSender) {
    loop {
        let res = sender.send(Message::Ping);
        if let Err(e) = res {
            log::error!("Failed to send ping: {e:?}");
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(
            PING_CORE_PROCESS_INTERVAL_SECS,
        ));
    }
    log::info!("send_ping: Finished");
}

/// Creates and initializes the core process with socket communication.
pub fn create_core_process(
    app: &tauri::AppHandle,
) -> Result<(CoreProcess, SocketSender, EventSocket), CoreProcessCreationError> {
    log::info!("create_core_process: Creating core process");
    let tmp_dir = std::env::temp_dir();
    let socket_name = format!("core-socket-{}", create_random_suffix());
    let socket_path = format!("{}/{socket_name}", tmp_dir.display());

    let (rx, core_process) = start_sidecar(app, &socket_path);
    tauri::async_runtime::spawn(show_stdout(rx, app.clone()));
    let (sender, event_socket) = create_core_process_socket(&socket_path)?;
    let ping_sender = sender.clone();
    tauri::async_runtime::spawn(send_ping(ping_sender));
    Ok((
        CoreProcess {
            process: core_process,
        },
        sender,
        event_socket,
    ))
}

/// This is a workaround which we use in order to wake up the
/// webview window and process incoming ws messages, e.g. incoming
/// call request.
pub fn ping_frontend(app: AppHandle) {
    loop {
        let res = app.emit("ping", ());
        if let Err(e) = res {
            log::error!("Failed to emit ping: {e:?}");
            sentry_utils::upload_logs_event("Failed to emit ping".to_string());
        }
        std::thread::sleep(std::time::Duration::from_secs(PING_SLEEP_SECS));
    }
}

/// Returns the platform-specific log file path.
pub fn get_log_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|mut path| {
            path.push("Library/Logs/com.hopp.app/hopp.log");
            path
        })
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_local_dir().map(|mut path| {
            path.push("com.hopp.app/logs/hopp.log");
            path
        })
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        log::warn!("get_log_path: Unsupported target OS, returning None for log path.");
        None
    }
}

/// Determines the log level from environment variables.
pub fn get_log_level() -> LevelFilter {
    let level = match env::var("RUST_LOG")
        .unwrap_or_else(|_| "info".to_string())
        .as_str()
    {
        "debug" => LevelFilter::Debug,
        "info" => LevelFilter::Info,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        _ => LevelFilter::Info,
    };
    let level_value = env::var("LOG_LEVEL").unwrap_or_else(|_| level.to_string());
    env::set_var(
        "RUST_LOG",
        format!("hopp_core={level_value},sentry_utils={level_value},socket_lib={level_value}"),
    );
    level
}

/// Centers the window relative to the tray icon position with multi-monitor support.
#[cfg(target_os = "macos")]
pub fn center_window_on_tray(window: &WebviewWindow, tray_rect: Rect, show_window: bool) {
    log::info!("center_window_on_tray: tray_rect: {tray_rect:?}, show_window: {show_window:?}");

    /*
     * Because centering the window using the move_window function is
     * broken we have to calculate the position of the window manually.
     * See https://github.com/tauri-apps/tauri/issues/7139.
     * First we find in which monitor the tray icon is located and then we store
     * the scale. Then we calculate the size of the window by checking the
     * scale and comparing the width with the expected hardcoded value defined
     * in the tauri.conf.json file. This is needed because when we the tray icon
     * is clicked from a different monitor the window size keeps the scale from the
     * previous one and this can cause wrong calculations.
     * We are setting logical position because the physical position is not
     * working as expected, probably for the same reason as the window size.
     */
    let mut scale = 1.0;
    /* The tray rect position is in physical units. Its top edge can sit just
     * outside the monitor (for example y == -2 on an external display), so
     * use the icon center for monitor detection. */
    let tray_pos: PhysicalPosition<i32> = tray_rect.position.to_physical(1.0);
    let tray_size_at_1x: PhysicalSize<f64> = tray_rect.size.to_physical(1.0);
    let tray_center = PhysicalPosition::new(
        tray_pos.x + (tray_size_at_1x.width / 2.0) as i32,
        tray_pos.y + (tray_size_at_1x.height / 2.0) as i32,
    );
    let mut found_monitor = false;
    let monitors = window.available_monitors();
    if let Ok(monitors) = monitors {
        for monitor in monitors {
            let monitor_pos = monitor.position();
            let monitor_size = monitor.size();
            let x_offset = tray_center.x - monitor_pos.x;
            let y_offset = tray_center.y - monitor_pos.y;
            if (x_offset >= 0)
                && (x_offset <= (monitor_size.width as i32))
                && (y_offset >= 0)
                && (y_offset <= (monitor_size.height as i32))
            {
                log::info!("center_window_on_tray: Found monitor: {monitor:?}");
                scale = monitor.scale_factor();
                found_monitor = true;
                break;
            }
        }
    } else {
        log::warn!("center_window_on_tray: Available monitors errored scale to 1.0");
    }

    if !found_monitor {
        log::warn!(
            "center_window_on_tray: Tray center {tray_center:?} is outside all monitors, skipping"
        );
        return;
    }

    let tray_size: PhysicalSize<f64> = tray_rect.size.to_physical(scale);
    let mut window_size = match window.outer_size() {
        Ok(size) => size,
        Err(e) => {
            log::error!("center_window_on_tray: Failed to get window outer size: {e:?}");
            return;
        }
    };
    if scale > 1.0 && window_size.width < 800 {
        window_size = PhysicalSize::new(
            ((window_size.width as f64) * scale) as u32,
            ((window_size.height as f64) * scale) as u32,
        );
    } else if scale == 1.0 && window_size.width >= 800 {
        // TODO: Here we hardcode the size if the size changes
        // we should change this as well.
        window_size = PhysicalSize::new(400, 500);
    }
    let x =
        ((tray_pos.x as f64) + tray_size.width / 2.0 - (window_size.width as f64) / 2.0) / scale;
    let y = (tray_pos.y as f64) / scale;

    let new_position = LogicalPosition::new(x, y);
    let _ = window.set_position(new_position);
    if show_window {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Add a tray icon to the app on macos, on windows we don't use it.
#[allow(unused_variables)]
pub fn setup_tray_icon(
    app: &mut App<Wry>,
    menu: &tauri::menu::Menu<Wry>,
    location_set: Arc<Mutex<bool>>,
    tray_clicked: Arc<AtomicBool>,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    {
        let location_set_clone = location_set.clone();
        let app_handle = app.handle().clone();

        // Use dark icon as template - macOS will automatically invert for dark menu bars
        let mut builder = TrayIconBuilder::new()
            .menu(menu)
            .show_menu_on_left_click(false)
            .icon_as_template(true);

        if let Some(icon) = tray::load_tray_icon(&app_handle, "tray-dark-default.png") {
            log::info!("setup_tray_icon: Using template icon tray-dark-default.png");
            builder = builder.icon(icon);
        } else if let Some(icon) = app.default_window_icon() {
            log::warn!("setup_tray_icon: Failed to load tray icon, using default window icon");
            builder = builder.icon(icon.clone());
        }

        let tray = builder
            .on_tray_icon_event(move |tray, event| {
                tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    tray_clicked.store(true, Ordering::Relaxed);
                    let tray_clicked_reset = tray_clicked.clone();
                    tauri::async_runtime::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                        tray_clicked_reset.store(false, Ordering::Relaxed);
                    });

                    let app_handle = tray.app_handle();
                    if let Some(window) = app_handle.get_webview_window("main") {
                        match window.is_visible() {
                            Ok(true) => {
                                let _ = window.hide();
                            }
                            Ok(false) => {
                                if let Ok(mut location_set) = location_set.lock() {
                                    if !*location_set {
                                        *location_set = true;
                                    }
                                }
                                if let Ok(Some(rect)) = tray.rect() {
                                    center_window_on_tray(&window, rect, true);
                                }
                            }
                            Err(e) => log::error!(
                                "setup_tray_icon: Failed to check window visibility: {e:?}"
                            ),
                        }
                    }
                }
            })
            .on_menu_event(|app, event| {
                if event.id.as_ref() == "quit" {
                    log::info!("Quit menu item clicked");
                    app.exit(0);
                }
            })
            .build(app)?;

        // Store the tray state in AppData for dynamic icon updates
        let tray_state = tray::TrayState::new(tray.clone());
        {
            let data = app.state::<Mutex<AppData>>();
            let mut data = data.lock().unwrap();
            data.tray_state = Some(tray_state);
        }

        let app_handle = app.handle().clone();

        /*
         * Spawns an async task to manage window positioning relative to the tray icon.
         * This runs once during app initialization and continues indefinitely.
         *
         * Initially it waits for the OS to assign a valid tray icon position (y == 0 indicates
         * the menu bar). Polls every 100ms for up to 100 attempts. Once valid,
         * centers the window on the tray if it's not visible.
         *
         * After initial centering, it polls every 200ms to detect tray icon position changes
         * (e.g., when the user rearranges menu bar items). If the position changed and the window
         * is visible, re-centers it to follow the tray icon.
         * See: https://github.com/gethopp/hopp/issues/211
         */
        tauri::async_runtime::spawn(async move {
            let mut tray_rect = match tray.rect() {
                Ok(Some(rect)) => rect,
                _ => {
                    log::warn!("setup_tray_icon: Initial tray rect not available");
                    return;
                }
            };
            for _ in 0..100 {
                if tray_rect.position.to_physical::<i32>(1.0).y == 0 {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
                if let Ok(Some(rect)) = tray.rect() {
                    tray_rect = rect;
                }
            }

            // Initial centering
            let mut last_pos = tray_rect.position.to_physical::<i32>(1.0);
            if let Some(window) = app_handle.get_webview_window("main") {
                match window.is_visible() {
                    Ok(false) => {
                        if let Ok(mut location_set) = location_set_clone.lock() {
                            if !*location_set {
                                *location_set = true;
                            }
                        }
                        center_window_on_tray(&window, tray_rect, false);
                    }
                    Ok(true) => {}
                    Err(e) => log::error!(
                        "setup_tray_icon: Failed to check window visibility in loop: {e:?}"
                    ),
                }
            }

            loop {
                tokio::time::sleep(Duration::from_millis(200)).await;

                if let Ok(Some(rect)) = tray.rect() {
                    let pos = rect.position.to_physical::<i32>(1.0);

                    if pos.x != last_pos.x || pos.y != last_pos.y {
                        last_pos = pos;
                        if let Some(window) = app_handle.get_webview_window("main") {
                            let is_visible = window.is_visible();

                            if let Ok(true) = is_visible {
                                center_window_on_tray(&window, rect, false);
                            }
                        }
                    }
                } else {
                    log::warn!("cannot pull tray rect");
                }
            }
        });
    }
    Ok(())
}

/// Setup start on launch.
#[allow(unused)]
pub fn setup_start_on_launch(manager: &AutoLaunchManager, first_run: bool) {
    // Only on macos call set_login_item
    #[cfg(all(target_os = "macos", not(debug_assertions)))]
    {
        let service = AppService::new(ServiceType::MainApp);
        let status = service.status();
        if status != ServiceStatus::Enabled && first_run {
            let res = service.register();
            if let Err(e) = res {
                log::error!("Failed to register app service: {:?}", e);
            }
        }
    }

    #[cfg(all(target_os = "windows", not(debug_assertions)))]
    {
        if first_run {
            let _ = manager.enable();
        }
    }
}

pub fn get_sentry_dsn() -> String {
    env!("SENTRY_DSN_RUST").to_string()
}

#[cfg(target_os = "macos")]
pub fn set_window_corner_radius_and_decorations(
    window: &tauri::WebviewWindow,
    radius: f64,
    decorations: bool,
) {
    use objc2_app_kit::NSWindowButton;

    let ns_window: &objc2_app_kit::NSWindow = match window.ns_window() {
        Ok(ns_window) => unsafe { &*ns_window.cast() },
        Err(e) => {
            log::error!("set_window_corner_radius: Failed to get NSWindow: {e:?}");
            return;
        }
    };

    if !decorations {
        if let Some(button) = ns_window.standardWindowButton(NSWindowButton::CloseButton) {
            button.setHidden(true);
        }
        if let Some(button) = ns_window.standardWindowButton(NSWindowButton::MiniaturizeButton) {
            button.setHidden(true);
        }
        if let Some(button) = ns_window.standardWindowButton(NSWindowButton::ZoomButton) {
            button.setHidden(true);
        }
    }

    let ns_view = match ns_window.contentView() {
        Some(view) => view,
        None => {
            log::error!("set_window_corner_radius: Failed to get NSView");
            return;
        }
    };
    ns_view.setWantsLayer(true);

    if let Some(layer) = ns_view.layer() {
        layer.setCornerRadius(radius);
        layer.setMasksToBounds(true);
    }
}

#[cfg(target_os = "macos")]
pub fn disable_app_nap() {
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};

    let process_info = NSProcessInfo::processInfo();
    let reason = NSString::from_str("Avoid WebKit throttling for uninterrupted operation");

    let activity = process_info.beginActivityWithOptions_reason(
        NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
        &reason,
    );

    // Leak the activity token so App Nap stays disabled for the lifetime of the process.
    std::mem::forget(activity);

    log::info!("App Nap disabled");
}

fn create_random_suffix() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect()
}

pub struct MediaWindowConfig<'a> {
    pub label: &'a str,
    pub title: &'a str,
    pub url: &'a str,
    pub width: f64,
    pub height: f64,
    pub resizable: bool,
    pub always_on_top: bool,
    pub content_protected: bool,
    pub maximizable: bool,
    pub minimizable: bool,
    pub decorations: bool,
    pub transparent: bool,
    pub background_color: Option<tauri::webview::Color>,
}

pub fn create_media_window(app: &AppHandle, config: MediaWindowConfig<'_>) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(config.label) {
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }

    #[allow(unused_mut)]
    let mut window_builder =
        WebviewWindowBuilder::new(app, config.label, WebviewUrl::App(config.url.into()))
            .title(config.title)
            .inner_size(config.width, config.height)
            .resizable(config.resizable)
            .visible(false)
            .transparent(config.transparent)
            .shadow(true)
            .always_on_top(config.always_on_top)
            .maximizable(config.maximizable)
            .minimizable(config.minimizable)
            .content_protected(config.content_protected);

    if let Some(bg_color) = config.background_color {
        window_builder = window_builder.background_color(bg_color);
    }

    #[cfg(target_os = "macos")]
    {
        window_builder = window_builder.hidden_title(true);
        window_builder = window_builder.title_bar_style(TitleBarStyle::Overlay);
    }

    #[cfg(target_os = "windows")]
    {
        window_builder = window_builder.decorations(config.decorations);
    }

    let window = window_builder
        .build()
        .map_err(|e| format!("Failed to create {} window: {}", config.label, e))?;

    let window_clone = window.clone();
    let label_clone = config.label.to_string();

    window
        .run_on_main_thread(move || {
            #[cfg(target_os = "macos")]
            {
                set_window_corner_radius_and_decorations(
                    &window_clone,
                    CORNER_RADIUS,
                    config.decorations,
                );
            }

            #[cfg(target_os = "windows")]
            {
                use window_vibrancy::apply_blur;

                if let Err(e) = apply_blur(&window_clone, Some((18, 18, 18, 125))) {
                    log::warn!("Failed to apply blur to {} window: {}", label_clone, e);
                }
            }

            if let Err(e) = window_clone.show() {
                log::error!("Failed to show {} window: {}", label_clone, e);
            }

            if let Err(e) = window_clone.set_focus() {
                log::error!("Failed to focus {} window: {}", label_clone, e);
            }
        })
        .map_err(|e| format!("Failed to run on main thread: {}", e))?;

    Ok(())
}
