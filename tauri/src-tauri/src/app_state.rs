use serde::{Deserialize, Serialize};
pub use socket_lib::StoredMode;

/// User-facing settings exposed in the Settings window.
/// All fields are non-optional with sensible defaults.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserSettings {
    pub call_feedback_popup: bool,
    pub show_dock_icon_in_call: bool,
    pub start_camera_on_call: bool,
    pub start_mic_on_call: bool,
    pub hopp_server_url: Option<String>,
    pub shortcut_toggle_mic: Option<String>,
    pub shortcut_toggle_camera: Option<String>,
    pub shortcut_toggle_screenshare: Option<String>,
}

impl Default for UserSettings {
    fn default() -> Self {
        UserSettings {
            call_feedback_popup: true,
            show_dock_icon_in_call: true,
            start_camera_on_call: false,
            start_mic_on_call: true,
            hopp_server_url: None,
            shortcut_toggle_mic: None,
            shortcut_toggle_camera: None,
            shortcut_toggle_screenshare: None,
        }
    }
}

#[cfg(target_os = "macos")]
const DEFAULT_SHORTCUT_MIC: &str = "Cmd+Shift+A";
#[cfg(not(target_os = "macos"))]
const DEFAULT_SHORTCUT_MIC: &str = "Ctrl+Shift+A";

#[cfg(target_os = "macos")]
const DEFAULT_SHORTCUT_CAMERA: &str = "Cmd+Shift+V";
#[cfg(not(target_os = "macos"))]
const DEFAULT_SHORTCUT_CAMERA: &str = "Ctrl+Shift+V";

#[cfg(target_os = "macos")]
const DEFAULT_SHORTCUT_SCREENSHARE: &str = "Cmd+Shift+S";
#[cfg(not(target_os = "macos"))]
const DEFAULT_SHORTCUT_SCREENSHARE: &str = "Ctrl+Shift+S";

impl UserSettings {
    pub fn resolve_shortcuts(&mut self) {
        if self.shortcut_toggle_mic.is_none() {
            self.shortcut_toggle_mic = Some(DEFAULT_SHORTCUT_MIC.to_string());
        }
        if self.shortcut_toggle_camera.is_none() {
            self.shortcut_toggle_camera = Some(DEFAULT_SHORTCUT_CAMERA.to_string());
        }
        if self.shortcut_toggle_screenshare.is_none() {
            self.shortcut_toggle_screenshare = Some(DEFAULT_SHORTCUT_SCREENSHARE.to_string());
        }
    }
}
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

const OLD_USER_TOKEN_FILE: &str = "user_token.txt";

/// Returns the name for file that stores the app state.
fn get_app_state_filename() -> String {
    /*
     * Initialize the app state filename based on debug/release mode.
     * The suffix is added on debug when starting the replica app in the
     * same machine for faster debugging.
     */
    if cfg!(debug_assertions) {
        let random_suffix = std::env::var("HOPP_SUFFIX").unwrap_or_default();
        format!("app_state_{random_suffix}.json")
    } else {
        "app_state.json".to_string()
    }
}

/// Current version of the application state structure.
///
/// This struct represents the complete application state that gets
/// persisted to disk. It includes all user preferences and settings
/// that should survive between application restarts.
/// All new fields should be Option<T>, so we avoid broken parsing and initializing to defaults.
#[derive(Debug, Serialize, Deserialize)]
struct AppStateInternal {
    /// Whether the notifications which shows that hopp is in the menu bar will be shown
    pub tray_notification: bool,

    /// The device ID of the last used microphone.
    pub last_used_mic: Option<String>,

    /// The device name of the last used camera.
    pub last_used_camera: Option<String>,

    /// Flag indicating if this is the user's first time running the application.
    pub first_run: bool,

    /// User JWT
    pub user_jwt: Option<String>,

    /// The user's preferred interaction mode for screen sharing sessions
    pub last_mode: Option<StoredMode>,

    /// Whether the sharer's drawing mode should persist until right-click
    #[serde(alias = "drawing_permanent")]
    pub sharer_draw_persist: Option<bool>,

    /// Whether the controller's drawing mode should persist until right-click
    pub controller_draw_persist: Option<bool>,

    /// Whether the first-time drawing hint toast has been shown
    pub drawing_hint_shown: Option<bool>,

    /// User-facing settings from the Settings window
    pub user_settings: Option<UserSettings>,
}

/// Legacy version of the application state structure.
#[derive(Debug, Serialize, Deserialize)]
struct OldAppStateInternal {
    pub tray_notification: bool,
    pub last_used_mic: Option<String>,
    pub first_run: bool,
    pub user_jwt: Option<String>,
}

impl Default for AppStateInternal {
    /// Creates a new application state with default values.
    ///
    /// Default settings:
    /// - Tray notification: enabled
    /// - Last used microphone: none
    /// - Last used camera: none
    /// - First run: true
    /// - User JWT: none
    /// - Hopp server URL: none
    /// - Last mode: none
    /// - Sharer draw persist: none
    /// - Controller draw persist: none
    /// - Drawing hint shown: none
    fn default() -> Self {
        AppStateInternal {
            tray_notification: true,
            last_used_mic: None,
            last_used_camera: None,
            first_run: true,
            user_jwt: None,
            last_mode: None,
            sharer_draw_persist: None,
            controller_draw_persist: None,
            drawing_hint_shown: None,
            user_settings: None,
        }
    }
}

/// Thread-safe application state manager.
///
/// This struct provides thread-safe access to application settings and handles
/// persistence to disk. It includes migration logic for backward compatibility
/// and uses a mutex to ensure safe concurrent access.
#[derive(Debug, Serialize, Deserialize)]
pub struct AppState {
    /// The internal state data.
    state: AppStateInternal,

    /// Root folder path where state files are stored.
    root_folder: PathBuf,

    /// Mutex for thread-safe access to state modifications.
    lock: Mutex<()>,
}

/// Retrieves and migrates the legacy user JWT stored outside the app state file.
fn retrieve_old_jwt(root_folder: &Path) -> Option<String> {
    let mut path = root_folder.to_path_buf();
    path.push(OLD_USER_TOKEN_FILE);
    if !path.exists() {
        return None;
    }

    log::debug!("Migrating legacy user token from: {}", path.display());

    match fs::read_to_string(&path) {
        Ok(token) => {
            if let Err(e) = fs::remove_file(&path) {
                log::error!(
                    "Failed to remove legacy user token file {}: {e:?}",
                    path.display()
                );
            }
            Some(token)
        }
        Err(e) => {
            log::error!(
                "Failed to read legacy user token from {}: {e:?}",
                path.display()
            );
            None
        }
    }
}

impl AppState {
    /// Creates a new AppState instance, loading from disk or using defaults.
    ///
    /// This constructor handles the complete initialization process including:
    /// - Loading existing state from disk
    /// - Migrating from legacy formats
    /// - Creating default state if no existing state is found
    /// - Setting up thread-safe access
    ///
    /// # Arguments
    ///
    /// * `root_folder` - Directory where state files should be stored
    ///
    /// # Returns
    ///
    /// A new `AppState` instance ready for use
    pub fn new(root_folder: &Path) -> Self {
        let app_state_filename = get_app_state_filename();
        let app_state_path = root_folder.join(app_state_filename.clone());
        if !app_state_path.exists() {
            let state = AppStateInternal::default();

            if let Ok(serialized) = serde_json::to_string_pretty(&state) {
                let _ = fs::write(app_state_path, serialized);
            }

            return AppState {
                state,
                root_folder: root_folder.to_path_buf(),
                lock: Mutex::new(()),
            };
        }

        match fs::read_to_string(app_state_path) {
            Ok(contents) => match serde_json::from_str::<AppStateInternal>(&contents) {
                Ok(mut state) => {
                    let old_jwt = retrieve_old_jwt(root_folder);
                    if old_jwt.is_some() {
                        state.user_jwt = old_jwt;
                        let app_state_path = root_folder.join(app_state_filename);
                        if !Self::write_file(&app_state_path, &state) {
                            log::error!("Failed to write new app state to file.");
                        }
                    }
                    return AppState {
                        state,
                        root_folder: root_folder.to_path_buf(),
                        lock: Mutex::new(()),
                    };
                }
                Err(_) => {
                    log::error!("Failed to parse app state from file, using default state.");
                    /* Fallback for migration from old app state. */
                    match serde_json::from_str::<OldAppStateInternal>(&contents) {
                        Ok(state) => {
                            let mut new_state = AppStateInternal {
                                tray_notification: state.tray_notification,
                                last_used_mic: state.last_used_mic,
                                first_run: false,
                                ..Default::default()
                            };
                            new_state.user_jwt = retrieve_old_jwt(root_folder);
                            if new_state.user_jwt.is_none() {
                                new_state.user_jwt = state.user_jwt;
                            }

                            let app_state_path = root_folder.join(app_state_filename);
                            if !Self::write_file(&app_state_path, &new_state) {
                                log::error!("Failed to write new app state to file.");
                            }

                            return AppState {
                                state: new_state,
                                root_folder: root_folder.to_path_buf(),
                                lock: Mutex::new(()),
                            };
                        }
                        Err(_) => {
                            log::error!(
                                "Failed to parse old app state from file, using default state."
                            );
                        }
                    }
                }
            },
            Err(_) => {
                log::error!("Failed to read app state file, using default state.");
            }
        }
        AppState {
            state: AppStateInternal::default(),
            root_folder: root_folder.to_path_buf(),
            lock: Mutex::new(()),
        }
    }

    /// Gets the current tray notification setting.
    pub fn tray_notification(&self) -> bool {
        let _lock = self.lock.lock().unwrap();
        self.state.tray_notification
    }

    /// Updates the tray notification setting and saves to disk.
    pub fn set_tray_notification(&mut self, value: bool) {
        log::info!("set_tray_notification: {value}");
        let _lock = self.lock.lock().unwrap();
        self.state.tray_notification = value;
        if !self.save() {
            log::error!("set_tray_notification: Failed to save app state");
        }
    }

    /// Gets the last used microphone device ID.
    pub fn last_used_mic(&self) -> Option<String> {
        let _lock = self.lock.lock().unwrap();
        self.state.last_used_mic.clone()
    }

    /// Updates the last used microphone setting and saves to disk.
    pub fn set_last_used_mic(&mut self, mic: String) {
        log::info!("set_last_used_mic: {mic}");
        let _lock = self.lock.lock().unwrap();
        self.state.last_used_mic = Some(mic);
        if !self.save() {
            log::error!("set_last_used_mic: Failed to save app state");
        }
    }

    /// Gets the last used camera device name.
    pub fn last_used_camera(&self) -> Option<String> {
        let _lock = self.lock.lock().unwrap();
        self.state.last_used_camera.clone()
    }

    /// Updates the last used camera setting and saves to disk.
    pub fn set_last_used_camera(&mut self, camera: String) {
        log::info!("set_last_used_camera: {camera}");
        let _lock = self.lock.lock().unwrap();
        self.state.last_used_camera = Some(camera);
        if !self.save() {
            log::error!("set_last_used_camera: Failed to save app state");
        }
    }

    /// Checks if this is the user's first time running the application.
    pub fn first_run(&self) -> bool {
        let _lock = self.lock.lock().unwrap();
        self.state.first_run
    }

    /// Updates the first-run flag and saves to disk.
    pub fn set_first_run(&mut self, value: bool) {
        let _lock = self.lock.lock().unwrap();
        self.state.first_run = value;
        if !self.save() {
            log::error!("set_first_run: Failed to save app state");
        }
    }

    /// Gets the user JWT.
    pub fn user_jwt(&self) -> Option<String> {
        let _lock = self.lock.lock().unwrap();
        self.state.user_jwt.clone()
    }

    /// Updates the user JWT and saves to disk.
    pub fn set_user_jwt(&mut self, jwt: Option<String>) {
        let _lock = self.lock.lock().unwrap();
        self.state.user_jwt = jwt;
        if !self.save() {
            log::error!("set_user_jwt: Failed to save app state");
        }
    }

    /// Gets the user's preferred interaction mode.
    pub fn last_mode(&self) -> Option<StoredMode> {
        let _lock = self.lock.lock().unwrap();
        self.state.last_mode.clone()
    }

    /// Updates the user's preferred interaction mode and saves to disk.
    pub fn set_last_mode(&mut self, mode: StoredMode) {
        log::info!("set_last_mode: {mode:?}");
        let _lock = self.lock.lock().unwrap();
        self.state.last_mode = Some(mode);
        if !self.save() {
            log::error!("set_last_mode: Failed to save app state");
        }
    }

    /// Gets whether the sharer's drawing mode should persist until right-click.
    pub fn sharer_draw_persist(&self) -> bool {
        let _lock = self.lock.lock().unwrap();
        self.state.sharer_draw_persist.unwrap_or(false)
    }

    /// Updates the sharer draw persist setting and saves to disk.
    pub fn set_sharer_draw_persist(&mut self, persist: bool) {
        log::info!("set_sharer_draw_persist: {persist}");
        let _lock = self.lock.lock().unwrap();
        self.state.sharer_draw_persist = Some(persist);
        if !self.save() {
            log::error!("set_sharer_draw_persist: Failed to save app state");
        }
    }

    /// Gets whether the controller's drawing mode should persist until right-click.
    pub fn controller_draw_persist(&self) -> bool {
        let _lock = self.lock.lock().unwrap();
        self.state.controller_draw_persist.unwrap_or(false)
    }

    /// Updates the controller draw persist setting and saves to disk.
    pub fn set_controller_draw_persist(&mut self, persist: bool) {
        log::info!("set_controller_draw_persist: {persist}");
        let _lock = self.lock.lock().unwrap();
        self.state.controller_draw_persist = Some(persist);
        if !self.save() {
            log::error!("set_controller_draw_persist: Failed to save app state");
        }
    }

    /// Gets whether the first-time drawing hint toast has been shown.
    pub fn drawing_hint_shown(&self) -> bool {
        let _lock = self.lock.lock().unwrap();
        self.state.drawing_hint_shown.unwrap_or(false)
    }

    /// Updates the drawing hint shown flag and saves to disk.
    pub fn set_drawing_hint_shown(&mut self, shown: bool) {
        log::info!("set_drawing_hint_shown: {shown}");
        let _lock = self.lock.lock().unwrap();
        self.state.drawing_hint_shown = Some(shown);
        if !self.save() {
            log::error!("set_drawing_hint_shown: Failed to save app state");
        }
    }

    /// Gets the user settings, returning defaults if not yet stored.
    pub fn user_settings(&self) -> UserSettings {
        let _lock = self.lock.lock().unwrap();
        self.state.user_settings.clone().unwrap_or_default()
    }

    /// Updates a single user setting field and saves to disk.
    pub fn update_user_setting(&mut self, f: impl FnOnce(&mut UserSettings)) {
        let _lock = self.lock.lock().unwrap();
        let settings = self
            .state
            .user_settings
            .get_or_insert_with(UserSettings::default);
        f(settings);
        if !self.save() {
            log::error!("update_user_setting: Failed to save app state");
        }
    }

    /// Saves the current state to disk.
    ///
    /// # Returns
    ///
    /// `true` if the save was successful, `false` if an error occurred
    ///
    /// # Thread Safety
    ///
    /// This method assumes the caller already holds the internal lock.
    /// It should only be called from other methods that have acquired the lock.
    fn save(&self) -> bool {
        let app_state_path = self.root_folder.join(get_app_state_filename());
        Self::write_file(&app_state_path, &self.state)
    }

    /// Writes the state data to a file in JSON format.
    ///
    /// # Arguments
    ///
    /// * `path` - Path where the state file should be written
    /// * `state` - State data to serialize and write
    ///
    /// # Returns
    ///
    /// `true` if the write was successful, `false` if an error occurred
    ///
    /// # Error Handling
    ///
    /// Logs serialization and file write errors but does not panic.
    fn write_file(path: &PathBuf, state: &AppStateInternal) -> bool {
        match serde_json::to_string_pretty(state) {
            Ok(serialized) => {
                return fs::write(path, serialized).is_ok();
            }
            Err(e) => log::error!("Failed to serialize app state: {e}"),
        }
        false
    }
}
