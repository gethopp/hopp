import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";

const isTauri = typeof window !== "undefined" && window.__TAURI_INTERNALS__ !== undefined;

export let appVersion: null | string = null;
getVersion().then((version) => {
  appVersion = version;
});

const createScreenShareWindow = async (videoToken: string, bringToFront: boolean = true) => {
  const URL = `screenshare.html?videoToken=${videoToken}`;

  // Check if there is already a window open,
  // then focus on it and bring it to the front
  const isWindowOpen = await WebviewWindow.getByLabel("screenshare");
  if (isWindowOpen && bringToFront) {
    await isWindowOpen.setFocus();
    return;
  }

  if (isTauri) {
    const newWindow = new WebviewWindow("screenshare", {
      width: 800,
      height: 450,
      url: URL,
      hiddenTitle: true,
      titleBarStyle: "overlay",
      resizable: true,
      // alwaysOnTop: true,
      maximizable: false,
      alwaysOnTop: false,
      visible: true,
      title: "Screen sharing",
    });
    newWindow.once("tauri://window-created", () => {
      newWindow.setFocus();
    });
  } else {
    window.open(URL);
  }
};

const createContentPickerWindow = async (videoToken: string) => {
  const URL = `contentPicker.html?videoToken=${videoToken}`;

  if (isTauri) {
    const newWindow = new WebviewWindow("contentPicker", {
      width: 800,
      height: 450,
      url: URL,
      hiddenTitle: true,
      titleBarStyle: "overlay",
      resizable: true,
      alwaysOnTop: false,
      visible: true,
      title: "Content picker",
    });
    newWindow.once("tauri://window-created", () => {
      newWindow.setFocus();
    });
  } else {
    window.open(URL);
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
  const token = await invoke<string | null>("get_stored_token");
  return token;
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

const closeContentPickerWindow = async () => {
  if (isTauri) {
    const contentPickerWindow = await WebviewWindow.getByLabel("contentPicker");
    if (contentPickerWindow) {
      console.debug("Closing content picker window");
      await contentPickerWindow.close();
    }
  }
};

const getVideoTokenParam = () => {
  const urlParams = new URLSearchParams(window.location.search);
  return urlParams.get("videoToken");
};

const endCallCleanup = async () => {
  await resetCoreProcess();
  await closeScreenShareWindow();
  await closeContentPickerWindow();
  await setDockIconVisible(false);
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

const hideTrayIconInstruction = async () => {
  await invoke("skip_tray_notification_selection_window");
};

const setDockIconVisible = async (visible: boolean) => {
  await invoke("set_dock_icon_visible", { visible });
};

const getLastUsedMic = async () => {
  return await invoke<string | null>("get_last_used_mic");
};

const setLastUsedMic = async (micId: string) => {
  return await invoke("set_last_used_mic", { mic: micId });
};

const minimizeMainWindow = async () => {
  return await invoke("minimize_main_window");
};

const setLivekitUrl = async (url: string) => {
  return await invoke("set_livekit_url", { url });
};

const getLivekitUrl = async () => {
  const url = await invoke<string>("get_livekit_url");
  return url;
};

export const tauriUtils = {
  createScreenShareWindow,
  closeScreenShareWindow,
  createContentPickerWindow,
  showWindow,
  storeTokenBackend,
  getStoredToken,
  deleteStoredToken,
  stopSharing,
  endCallCleanup,
  hideTrayIconInstruction,
  setControllerCursor,
  getVideoTokenParam,
  openAccessibilitySettings,
  openMicrophoneSettings,
  openScreenShareSettings,
  triggerScreenSharePermission,
  getControlPermission,
  getMicPermission,
  getScreenSharePermission,
  setDockIconVisible,
  getLastUsedMic,
  setLastUsedMic,
  minimizeMainWindow,
  setLivekitUrl,
  getLivekitUrl,
};
