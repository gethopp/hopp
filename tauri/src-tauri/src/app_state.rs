use serde::{Deserialize, Serialize};
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
#[derive(Debug, Serialize, Deserialize)]
struct AppStateInternal {
    /// Whether the notifications which shows that hopp is in the menu bar will be shown
    pub tray_notification: bool,

    /// The device ID of the last used microphone.
    pub last_used_mic: Option<String>,

    /// Flag indicating if this is the user's first time running the application.
    pub first_run: bool,

    /// User JWT
    pub user_jwt: Option<String>,

    /// Hopp server URL
    pub hopp_server_url: Option<String>,

    /// Whether the post-call feedback dialog is disabled
    pub feedback_disabled: bool,
}

/// Legacy version of the application state structure.
#[derive(Debug, Serialize, Deserialize)]
struct OldAppStateInternal {
    pub tray_notification: bool,
    pub last_used_mic: Option<String>,
    pub first_run: bool,
}

impl Default for AppStateInternal {
    /// Creates a new application state with default values.
    ///
    /// Default settings:
    /// - Tray notification: enabled
    /// - Last used microphone: none
    /// - First run: true
    /// - User JWT: none
    /// - Hopp server URL: none
    /// - Feedback disabled: false
    fn default() -> Self {
        AppStateInternal {
            tray_notification: true,
            last_used_mic: None,
            first_run: true,
            user_jwt: None,
            hopp_server_url: None,
            feedback_disabled: false,
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

    /// Gets the hopp server URL override.
    pub fn hopp_server_url(&self) -> Option<String> {
        let _lock = self.lock.lock().unwrap();
        self.state.hopp_server_url.clone()
    }

    /// Updates the hopp server URL override and saves to disk.
    pub fn set_hopp_server_url(&mut self, url: Option<String>) {
        log::info!("set_hopp_server_url: {url:?}");
        let _lock = self.lock.lock().unwrap();
        self.state.hopp_server_url = url;
        if !self.save() {
            log::error!("set_hopp_server_url: Failed to save app state");
        }
    }

    /// Gets whether post-call feedback dialog is disabled.
    pub fn feedback_disabled(&self) -> bool {
        let _lock = self.lock.lock().unwrap();
        self.state.feedback_disabled
    }

    /// Updates the feedback disabled setting and saves to disk.
    pub fn set_feedback_disabled(&mut self, disabled: bool) {
        log::info!("set_feedback_disabled: {disabled}");
        let _lock = self.lock.lock().unwrap();
        self.state.feedback_disabled = disabled;
        if !self.save() {
            log::error!("set_feedback_disabled: Failed to save app state");
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
