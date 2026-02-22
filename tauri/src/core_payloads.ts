import { invoke } from "@tauri-apps/api/core";
import { TStoredMode } from "@/payloads";

/**
 * Socket lib types are defined in (core/socket_lib/src/lib.rs).
 *
 * At some point we could use:
 * https://github.com/Aleph-Alpha/ts-rs
 *
 * But skipping for now, as it would need quite
 * many changes to the socket code, and tweaking.
 */
export interface Extent {
  width: number;
  height: number;
}

export interface WindowFrameMessage {
  origin_x: number;
  origin_y: number;
  width: number;
  height: number;
}

export interface CursorPositionMessage {
  x: number;
  y: number;
}

export interface MouseClickMessage {
  x: number;
  y: number;
  button: number;
  clicks: number;
  shift_key: boolean;
}

export interface ScrollMessage {
  x: number;
  y: number;
}

export interface KeystrokeMessage {
  key: string;
  meta: boolean;
  shift: boolean;
  ctrl: boolean;
  alt: boolean;
  down: boolean;
}

export type ContentType = "Display" | { Window: { display_id: number } };

export interface Content {
  content_type: ContentType;
  id: number;
}

export interface CaptureContent {
  content: Content;
  base64: string;
  title: string;
}

export interface AvailableContentMessage {
  content: CaptureContent[];
}

export interface ScreenShareMessage {
  content: Content;
  resolution: Extent;
  accessibility_permission: boolean;
  use_av1: boolean;
}

export interface CallStartMessage {
  token: string;
}

export interface SentryMetadata {
  user_email: string;
  app_version: string;
}

export interface DrawingEnabled {
  permanent: boolean;
}

export interface AudioDevice {
  name: string;
  id: string;
}

export interface AudioCaptureMessage {
  device_id: string;
}

export interface CameraDevice {
  name: string;
  id: string;
}

export interface CameraStartMessage {
  device_name: string;
}

export interface CoreParticipantState {
  identity: string;
  name: string;
  connected: boolean;
  muted: boolean;
  has_camera: boolean;
  is_screensharing: boolean;
}

export type CoreRoleChange = "Sharer" | "Controller" | "None";

export interface CoreRoleEvent {
  role: CoreRoleChange;
}

/**
 * Tauri command map.
 *
 * This is a map of all the commands that can be invoked from the Tauri side.
 * It is used to generate the type-safe invoke function.
 */
export interface CommandMap {
  screenshare: {
    args: {
      content: Content;
      token: string;
      resolution: Extent;
      accessibilityPermission: boolean;
      useAv1: boolean;
    };
    return: void;
  };
  stop_sharing: { args: void; return: void };
  get_available_content: { args: void; return: CaptureContent[] };

  // Token management
  store_token_cmd: { args: { token: string }; return: void };
  get_stored_token: { args: void; return: string | null };
  delete_stored_token: { args: void; return: void };

  // Sound
  play_sound: { args: { soundName: string }; return: void };
  stop_sound: { args: { soundName: string }; return: void };

  // Core process
  reset_core_process: { args: void; return: void };

  // Logs
  get_logs: { args: void; return: string };

  // UI
  set_deactivate_hiding: { args: { deactivate: boolean }; return: void };
  set_controller_cursor: { args: { enabled: boolean }; return: void };
  minimize_main_window: { args: void; return: void };
  set_dock_icon_visible: { args: { visible: boolean }; return: void };
  set_tray_notification: { args: { enabled: boolean }; return: void };

  // Permissions
  open_accessibility_settings: { args: void; return: void };
  open_microphone_settings: { args: void; return: void };
  open_camera_settings: { args: void; return: void };
  open_screenshare_settings: { args: void; return: void };
  trigger_screenshare_permission: { args: void; return: boolean };
  get_control_permission: { args: void; return: boolean };
  get_microphone_permission: { args: void; return: boolean };
  get_screenshare_permission: { args: void; return: boolean };
  get_camera_permission: { args: void; return: boolean };
  skip_tray_notification_selection_window: { args: void; return: void };

  // Preferences
  get_last_used_mic: { args: void; return: string | null };
  set_last_used_mic: { args: { mic: string }; return: void };
  get_last_mode: { args: void; return: TStoredMode | null };
  set_last_mode: { args: { mode: TStoredMode }; return: void };
  get_drawing_permanent: { args: void; return: boolean };
  set_drawing_permanent: { args: { permanent: boolean }; return: void };
  enable_drawing: { args: { permanent: boolean }; return: void };

  // LiveKit
  set_livekit_url: { args: { url: string }; return: void };
  get_livekit_url: { args: void; return: string };

  // Windows
  create_screenshare_window: { args: { videoToken: string }; return: void };
  create_camera_window: { args: { cameraToken: string }; return: void };
  create_content_picker_window: { args: { videoToken: string; useAv1: boolean }; return: void };
  create_feedback_window: { args: { teamId: string; roomId: string; participantId: string }; return: void };

  // Sentry
  set_sentry_metadata: { args: { userEmail: string; appVersion: string }; return: void };

  // Call
  call_started: { args: { token: string }; return: void };
  end_call: { args: void; return: void };

  // Server
  get_hopp_server_url: { args: void; return: string | null };
  set_hopp_server_url: { args: { url: string | null }; return: void };

  // Feedback
  get_feedback_disabled: { args: void; return: boolean };
  set_feedback_disabled: { args: { disabled: boolean }; return: void };

  // Core socket messages — audio
  mute_mic: { args: void; return: void };
  unmute_mic: { args: void; return: void };
  toggle_mic: { args: void; return: void };
  stop_audio_capture: { args: void; return: void };
  list_microphones: { args: void; return: AudioDevice[] };
  select_microphone: { args: { deviceId: string }; return: void };

  // Core socket messages — camera
  list_webcams: { args: void; return: CameraDevice[] };
  start_camera: { args: { deviceName: string }; return: void };
  switch_camera: { args: { deviceName: string }; return: void };
  stop_camera: { args: void; return: void };
  open_camera_preview: { args: void; return: void };

  // Core socket messages — screenshare viewer
  open_screenshare_viewer: { args: void; return: void };
  close_screenshare_viewer: { args: void; return: void };
}

type InvokeArgs<K extends keyof CommandMap> = CommandMap[K]["args"] extends void ? [] : [CommandMap[K]["args"]];

/**
 * Typed invoke wrapper.
 *
 * This is a wrapper around the invoke function that
 * allows us to pass in the arguments and return type
 * of the command.
 */
export function typedInvoke<K extends keyof CommandMap>(
  cmd: K,
  ...args: InvokeArgs<K>
): Promise<CommandMap[K]["return"]> {
  return invoke<CommandMap[K]["return"]>(cmd, args[0] as Record<string, unknown>);
}
