pub mod app_state;
pub mod permissions;
pub mod sounds;

use log::LevelFilter;
use rand::{distributions::Alphanumeric, Rng};
use sounds::SoundEntry;
use std::env;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
#[cfg(target_os = "macos")]
use std::time::Duration;
use tauri::async_runtime::Receiver;
use tauri::path::BaseDirectory;
#[cfg(target_os = "macos")]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Emitter, Manager, Wry};
#[cfg(target_os = "macos")]
use tauri::{Rect, WebviewWindow};
use tauri_plugin_autostart::AutoLaunchManager;
use tauri_plugin_shell::{process::CommandChild, process::CommandEvent, ShellExt};

use socket_lib::{CursorSocket, Message};
#[cfg(target_os = "macos")]
use tauri::{LogicalPosition, PhysicalPosition, PhysicalSize};

#[cfg(all(target_os = "macos", not(debug_assertions)))]
use smappservice_rs::*;

const PING_SLEEP_SECS: u64 = 30;
const PING_CORE_PROCESS_INTERVAL_SECS: u64 = 15;

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

/// Central application data structure that holds all the runtime state and resources
/// needed by the Tauri application.
pub struct AppData {
    /// Socket connection to the core process for inter-process communication.
    /// Used to send commands like screen sharing requests, cursor control,
    /// and receive responses from the native core process.
    pub socket: CursorSocket,

    /// Active sound entries currently being played by the application.
    /// Each entry contains the sound name and a channel transmitter to control playback.
    /// Used to prevent duplicate sounds and manage sound lifecycle.
    pub sound_entries: Vec<SoundEntry>,

    /// Flag to control whether the main window should hide when it loses focus.
    /// This is set to true when the user is writing feedback.
    pub deactivate_hiding: Arc<Mutex<bool>>,

    /// On macOS, controls the application's activation policy and dock icon visibility.
    /// Wrapped in Arc<Mutex<>> for thread-safe access.
    /// On macOS we want to have a doc icon when the user is a controller in a call
    /// and when the onboarding windows are open. The icon is needed in order to
    /// allow cmd+tab to cycle through the windows.
    pub dock_enabled: Arc<Mutex<bool>>,

    /// Persistent application state that survives across app restarts.
    /// Manages settings like first run status, tray notifications, and user preferences.
    pub app_state: app_state::AppState,

    /// Livekit server URL.
    pub livekit_server_url: String,
}

impl AppData {
    /// Creates a new `AppData` instance with the provided dependencies.
    ///
    /// # Arguments
    ///
    /// * `socket` - The cursor socket for communicating with the core process
    /// * `deactivate_hiding` - Shared flag to control window hiding behavior
    /// * `dock_enabled` - Shared flag to control dock icon visibility
    /// * `app_state` - Persistent application state manager
    ///
    /// # Returns
    ///
    /// A new `AppData` instance with empty sound entries and the provided state.
    pub fn new(
        socket: CursorSocket,
        deactivate_hiding: Arc<Mutex<bool>>,
        dock_enabled: Arc<Mutex<bool>>,
        app_state: app_state::AppState,
    ) -> Self {
        AppData {
            socket,
            sound_entries: Vec::new(),
            deactivate_hiding,
            dock_enabled,
            app_state,
            livekit_server_url: "".to_string(),
        }
    }
}

/// Monitors core process output and emits crash events.
async fn show_stdout(mut receiver: Receiver<CommandEvent>, app_handle: AppHandle) {
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

    // Communicate to the frontend that the core process has crashed.
    let res = app_handle.emit("core_process_crashed", ());
    if let Err(e) = res {
        log::error!("Failed to emit core_process_crashed: {e:?}");
    }
}

/// Spawns the core process sidecar with required arguments.
fn start_sidecar(
    app: &tauri::AppHandle,
    textures_path: &Path,
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

    let mut args = vec![
        "--socket-path",
        socket_path,
        "--textures-path",
        textures_path.to_str().unwrap(),
    ];

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
fn create_core_process_socket(socket_path: &str) -> Result<CursorSocket, CoreProcessCreationError> {
    let max_tries = 10;
    let mut tries = 0;
    loop {
        match CursorSocket::new(socket_path) {
            Ok(socket) => return Ok(socket),
            Err(_) => {
                log::debug!(
                    "create_render_process_socket: Failed to create socket, retrying in 1 second"
                );
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
async fn send_ping(mut socket: CursorSocket) {
    loop {
        let res = socket.send_message(Message::Ping);
        if let Err(e) = res {
            log::error!("Failed to send ping: {e:?}");
            sentry_utils::upload_logs_event("Failed to send ping".to_string());
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
) -> Result<(CoreProcess, CursorSocket), CoreProcessCreationError> {
    log::info!("create_core_process: Creating core process");
    let mut resources_dir = app
        .path()
        .resolve("resources", BaseDirectory::Resource)
        .unwrap();
    resources_dir.push("core");
    /*
     * We need to do this because this has
     * UNC path, which is incompatible with File::open
     */
    #[cfg(target_os = "windows")]
    {
        resources_dir = resources_dir.canonicalize().unwrap();
        resources_dir = resources_dir
            .to_str()
            .and_then(|s| s.get(4..))
            .map(PathBuf::from)
            .unwrap_or(resources_dir);
    }

    let tmp_dir = std::env::temp_dir();
    let socket_name = format!("core-socket-{}", create_random_suffix());
    let socket_path = format!("{}/{socket_name}", tmp_dir.display());

    let (rx, core_process) = start_sidecar(app, &resources_dir, &socket_path);
    tauri::async_runtime::spawn(show_stdout(rx, app.clone()));
    let socket = create_core_process_socket(&socket_path)?;
    let socket_clone = socket.duplicate().unwrap();
    tauri::async_runtime::spawn(send_ping(socket_clone));
    Ok((
        CoreProcess {
            process: core_process,
        },
        socket,
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
fn center_window_on_tray(window: &WebviewWindow, tray_rect: Rect, show_window: bool) {
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
    /* The tray rect position is in physical units */
    let tray_pos: PhysicalPosition<i32> = tray_rect.position.to_physical(1.0);
    let monitors = window.available_monitors();
    if let Ok(monitors) = monitors {
        for monitor in monitors {
            let monitor_pos = monitor.position();
            let monitor_size = monitor.size();
            let x_offset = tray_pos.x - monitor_pos.x;
            let y_offset = tray_pos.y - monitor_pos.y;
            if (x_offset >= 0)
                && (x_offset <= (monitor_size.width as i32))
                && (y_offset >= 0)
                && (y_offset <= (monitor_size.height as i32))
            {
                log::info!("center_window_on_tray: Found monitor: {monitor:?}");
                scale = monitor.scale_factor();
                break;
            }
        }
    } else {
        log::warn!("center_window_on_tray: Available monitors errored scale to 1.0");
    }

    let tray_size: PhysicalSize<f64> = tray_rect.size.to_physical(scale);
    let mut window_size = window.outer_size().unwrap();
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

/// Returns the name for file that stores the token.
pub fn get_token_filename() -> String {
    /*
     * Initialize the token filename based on debug/release mode.
     * The suffix is added on debug when starting the replica app in the
     * same machine for faster debugging.
     */
    if cfg!(debug_assertions) {
        let random_suffix = env::var("HOPP_SUFFIX").unwrap_or_default();
        format!("user_token_{random_suffix}.txt")
    } else {
        "user_token.txt".to_string()
    }
}

/// Add a tray icon to the app on macos, on windows we don't use it.
#[allow(unused_variables)]
pub fn setup_tray_icon(
    app: &mut App<Wry>,
    menu: &tauri::menu::Menu<Wry>,
    location_set: Arc<Mutex<bool>>,
) -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(target_os = "macos")]
    {
        let location_set_clone = location_set.clone();
        let tray = TrayIconBuilder::new()
            .menu(menu)
            .show_menu_on_left_click(false)
            .icon(app.default_window_icon().unwrap().clone())
            .on_tray_icon_event(move |tray, event| {
                tauri_plugin_positioner::on_tray_event(tray.app_handle(), &event);
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    let app_handle = tray.app_handle();
                    if let Some(window) = app_handle.get_webview_window("main") {
                        if window.is_visible().unwrap() {
                            let _ = window.hide();
                        } else {
                            let mut location_set = location_set.lock().unwrap();
                            if !*location_set {
                                *location_set = true;
                            }
                            center_window_on_tray(&window, tray.rect().unwrap().unwrap(), true);
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
        let app_handle = app.handle().clone();

        /*
         * For simplicity we are not joining this thread. This function
         * is called once during app initialization.
         *
         * This function checks if the system has assigned the correct position to
         * the tray icon. When we get it, we set the position to the window, so
         * if it opens via a route which doesn't use the tray icon, it will be
         * positioned correctly.
         */
        std::thread::spawn(move || {
            let mut tray_rect = tray.rect().unwrap().unwrap();
            let mut attempts = 0;
            let max_attempts = 20;
            loop {
                let position: PhysicalPosition<i32> = tray_rect.position.to_physical(1.0);
                if position.y == 0 || attempts > max_attempts {
                    break;
                }
                std::thread::sleep(Duration::from_millis(500));
                attempts += 1;
                tray_rect = tray.rect().unwrap().unwrap();
            }
            if let Some(window) = app_handle.get_webview_window("main") {
                if !window.is_visible().unwrap() && attempts < max_attempts {
                    let mut location_set = location_set_clone.lock().unwrap();
                    if !*location_set {
                        *location_set = true;
                    }
                    center_window_on_tray(&window, tray_rect, false);
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
pub fn set_window_corner_radius(window: &tauri::WebviewWindow, radius: f64) {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_app_kit::NSView;
    use std::os::raw::c_void;

    window
        .with_webview(move |webview| {
            let ns_window = webview.ns_window() as *mut AnyObject;
            let view: *mut c_void = unsafe { msg_send![ns_window, contentView] };
            let ns_view: Option<Retained<NSView>> = unsafe { Retained::retain(view.cast()) };
            if ns_view.is_none() {
                log::error!("set_window_corner_radius: Failed to get NSView");
                return;
            }
            let ns_view = ns_view.unwrap();
            ns_view.setWantsLayer(true);

            if let Some(layer) = unsafe { ns_view.layer() } {
                layer.setCornerRadius(radius);
                layer.setMasksToBounds(true);
            }
        })
        .unwrap();
}

fn create_random_suffix() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect()
}
