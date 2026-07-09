import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
const isTauri = typeof window !== "undefined" && window.__TAURI_INTERNALS__ !== undefined;

export let appVersion: null | string = null;
getVersion().then((version) => {
  appVersion = version;
});

const getAvailableContent = async () => {
  if (isTauri) await invoke("get_available_content");
};

const closeCameraWindow = async () => {
  if (isTauri) {
    const cameraWindow = await WebviewWindow.getByLabel("camera");
    if (cameraWindow) {
      await cameraWindow.close();
    }
  }
};

const storeTokenBackend = async (token: string) => {
  if (isTauri) {
    try {
      await invoke("store_token_cmd", { token });
    } catch (err) {
      console.error("Failed to store token:", err);
    }
  }
};

const getStoredToken = async () => {
  return await invoke<string | null>("get_stored_token");
};

const deleteStoredToken = async () => {
  if (isTauri) {
    try {
      await invoke("delete_stored_token");
    } catch (err) {
      console.error("Failed to delete stored token:", err);
    }
  }
};

const stopSharing = async () => {
  await invoke("stop_sharing");
};

const showWindow = async (windowLabel: string) => {
  if (isTauri) {
    const window = await WebviewWindow.getByLabel(windowLabel);
    if (window) {
      await window.show();
      await window.unminimize();
      await window.setFocus();
    }
  }
};

const closeScreenShareWindow = async () => {
  if (isTauri) {
    const screenShareWindow = await WebviewWindow.getByLabel("screenshare");
    if (screenShareWindow) {
      console.debug("Closing screen share window");
      await screenShareWindow.close();
    }
  }
};

const resetCoreProcess = async () => {
  await invoke("reset_core_process");
};

const endCallCleanup = async () => {
  await resetCoreProcess();
  await closeScreenShareWindow();
  await closeCameraWindow();
};

const getTokenParam = (param: string) => {
  const urlParams = new URLSearchParams(window.location.search);
  return urlParams.get(param);
};

const setControllerCursor = async (enabled: boolean) => {
  await invoke("set_controller_cursor", { enabled: enabled });
};

const openAccessibilitySettings = async () => {
  return await invoke("open_accessibility_settings");
};

const openMicrophoneSettings = async () => {
  return await invoke("open_microphone_settings");
};

const openCameraSettings = async () => {
  return await invoke("open_camera_settings");
};

const openScreenShareSettings = async () => {
  return await invoke("open_screenshare_settings");
};

const triggerScreenSharePermission = async () => {
  return await invoke<boolean>("trigger_screenshare_permission");
};

const getControlPermission = async () => {
  return await invoke<boolean>("get_control_permission");
};

const getMicPermission = async () => {
  return await invoke<boolean>("get_microphone_permission");
};

const getScreenSharePermission = async () => {
  return await invoke<boolean>("get_screenshare_permission");
};

const getCameraPermission = async () => {
  return await invoke<boolean>("get_camera_permission");
};

const hideTrayIconInstruction = async () => {
  await invoke("skip_tray_notification_selection_window");
};

const getLastUsedMic = async () => {
  return await invoke<string | null>("get_last_used_mic");
};

const setLastUsedMic = async (micId: string) => {
  return await invoke("set_last_used_mic", { mic: micId });
};

const getLastUsedCamera = async () => {
  return await invoke<string | null>("get_last_used_camera");
};

const setLastUsedCamera = async (camera: string) => {
  return await invoke("set_last_used_camera", { camera });
};

const getSharerDrawPersist = async (): Promise<boolean> => {
  return await invoke<boolean>("get_sharer_draw_persist");
};

const setSharerDrawPersist = async (persist: boolean): Promise<void> => {
  return await invoke("set_sharer_draw_persist", { persist });
};

const getDrawingEnabled = async (): Promise<boolean> => {
  return await invoke<boolean>("get_drawing_enabled");
};

const setDrawingEnabled = async (enabled: boolean, permanent: boolean): Promise<void> => {
  return await invoke("set_drawing_enabled", { enabled, permanent });
};

const getDrawingHintShown = async (): Promise<boolean> => {
  return await invoke<boolean>("get_drawing_hint_shown");
};

const setDrawingHintShown = async (shown: boolean): Promise<void> => {
  return await invoke("set_drawing_hint_shown", { shown });
};

const minimizeMainWindow = async () => {
  return await invoke("minimize_main_window");
};

const setLivekitUrl = async (url: string) => {
  return await invoke("set_livekit_url", { url });
};

const getLivekitUrl = async () => {
  return await invoke<string>("get_livekit_url");
};

const setSentryMetadata = async (userId: string) => {
  const appVersion = await getVersion();
  return await invoke("set_sentry_metadata", { userId, appVersion });
};

const callStarted = async (audioToken: string, videoToken: string) => {
  return await invoke("call_started", { audioToken, videoToken });
};

/**
 * Loads the custom server URL from Tauri backend.
 */
const loadCustomServerUrl = async (): Promise<string | null> => {
  try {
    return await invoke<string | null>("get_hopp_server_url");
  } catch (error) {
    console.error("Failed to load custom server url from backend:", error);
  }
  return null;
};

/**
 * Sets a custom Hopp server URL.
 * Pass null to clear the custom URL and use the default.
 * Signs out the user when the URL changes.
 */
const setHoppServerUrl = async (url: string | null): Promise<void> => {
  try {
    await invoke("set_hopp_server_url", { url });
    // Sign out the user when changing the server URL
    await deleteStoredToken();
  } catch (error) {
    console.error("Failed to set hopp server url:", error);
    throw error;
  }
};

const setCallFeedbackPopup = async (enabled: boolean): Promise<void> => {
  await invoke("set_call_feedback_popup", { enabled });
};

const openSettingsWindow = async (): Promise<void> => {
  if (isTauri) {
    try {
      await invoke("create_settings_window");
      const windowHandle = await WebviewWindow.getByLabel("settings");
      if (windowHandle) {
        await windowHandle.setFocus();
      }
    } catch (error) {
      console.error("Failed to open settings window:", error);
    }
  }
};

const createFeedbackWindow = async (teamId: string, roomId: string, participantId: string): Promise<void> => {
  if (isTauri) {
    try {
      await invoke("create_feedback_window", { teamId, roomId, participantId });
      const windowHandle = await WebviewWindow.getByLabel("feedback");
      if (windowHandle) {
        await windowHandle.setFocus();
      }
    } catch (error) {
      console.error("Failed to create feedback window:", error);
    }
  } else {
    const URL = `feedback.html?teamId=${teamId}&roomId=${roomId}&participantId=${participantId}`;
    window.open(URL);
  }
};

const getUserSettings = async () => {
  return await invoke<{
    call_feedback_popup: boolean;
    show_dock_icon_in_call: boolean;
    start_camera_on_call: boolean;
    start_mic_on_call: boolean;
    noise_cancellation_enabled: boolean;
    screen_share_resolution: "P1080" | "P1440" | "P4K";
    hopp_server_url: string | null;
  }>("get_user_settings");
};

const showFeedbackWindowIfEnabled = async (teamId: string, roomId: string, participantId: string): Promise<void> => {
  if (!isTauri) return;

  try {
    const settings = await invoke<{ call_feedback_popup: boolean }>("get_user_settings");
    if (settings.call_feedback_popup) {
      await createFeedbackWindow(teamId, roomId, participantId);
    }
  } catch (error) {
    console.error("Failed to check/show feedback window:", error);
  }
};

export const tauriUtils = {
  closeScreenShareWindow,
  getAvailableContent,
  showWindow,
  closeCameraWindow,
  storeTokenBackend,
  getStoredToken,
  deleteStoredToken,
  stopSharing,
  endCallCleanup,
  hideTrayIconInstruction,
  setControllerCursor,
  getTokenParam,
  openAccessibilitySettings,
  openMicrophoneSettings,
  openScreenShareSettings,
  openCameraSettings,
  triggerScreenSharePermission,
  getControlPermission,
  getMicPermission,
  getScreenSharePermission,
  getCameraPermission,
  getLastUsedMic,
  setLastUsedMic,
  getLastUsedCamera,
  setLastUsedCamera,
  getSharerDrawPersist,
  setSharerDrawPersist,
  getDrawingEnabled,
  setDrawingEnabled,
  getDrawingHintShown,
  setDrawingHintShown,
  minimizeMainWindow,
  setLivekitUrl,
  getLivekitUrl,
  setSentryMetadata,
  callStarted,
  loadCustomServerUrl,
  setHoppServerUrl,
  setCallFeedbackPopup,
  openSettingsWindow,
  createFeedbackWindow,
  showFeedbackWindowIfEnabled,
  getUserSettings,
};
