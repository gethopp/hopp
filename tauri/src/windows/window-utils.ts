import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";

const isTauri = typeof window !== "undefined" && window.__TAURI_INTERNALS__ !== undefined;

export let appVersion: null | string = null;
getVersion().then((version) => {
  appVersion = version;
});

export interface CaptureContent {
  content: {
    content_type: "Display" | { Window: { display_id: number } };
    id: number;
  };
  base64: string;
  title: string;
}

export type ResolutionKey = "1080p" | "2K" | "1440p" | "2160p" | "4K";

const createScreenShareWindow = async (videoToken: string, bringToFront: boolean = true) => {
  const streamWsPort = await getStreamWsPort();

  const URL = `screenshare.html?streamWsPort=${streamWsPort}`;

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

const createContentPickerWindow = async () => {
  const URL = `contentPicker.html`;

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

const getAvailableContent = async () => {
  return await invoke<CaptureContent[]>("get_available_content");
};

const screenshare = async (content: CaptureContent["content"], resolution: ResolutionKey) => {
  const resolutionMap: Record<ResolutionKey, { width: number; height: number }> = {
    "1080p": { width: 1920, height: 1080 },
    "2K": { width: 2048, height: 1080 },
    "1440p": { width: 2560, height: 1440 },
    "2160p": { width: 3840, height: 2160 },
    "4K": { width: 4096, height: 2160 },
  };
  return await invoke<boolean>("screenshare", { content, resolution: resolutionMap[resolution] });
};

const stopSharing = async () => {
  await invoke("stop_sharing");
};

const showMainWindow = async () => {
  if (isTauri) {
    const mainWindow = await WebviewWindow.getByLabel("main");
    if (mainWindow) {
      await mainWindow.show();
      await mainWindow.unminimize();
      await mainWindow.setFocus();
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

const callStarted = async (token: string) => {
  return await invoke<boolean>("call_started", { token });
};

const callEnded = async () => {
  await invoke("call_ended");
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
  await callEnded();
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

const getStreamWsPort = async () => {
  return await invoke<number>("stream_ws_port");
};

export const tauriUtils = {
  createScreenShareWindow,
  closeScreenShareWindow,
  createContentPickerWindow,
  showMainWindow,
  storeTokenBackend,
  getStoredToken,
  deleteStoredToken,
  getAvailableContent,
  screenshare,
  stopSharing,
  callStarted,
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
  getStreamWsPort,
};
