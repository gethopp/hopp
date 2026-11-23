import { getPreferenceValues } from "@raycast/api";
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

/** Hopp cloud backend, used when the app has no custom server configured. */
export const DEFAULT_SERVER_URL = "pair.gethopp.app";

/**
 * The desktop app persists its state (session JWT and the optional custom
 * server URL for self-hosted deployments) in its app-data directory. The
 * extension reads it so it works out of the box once the user is signed into
 * the app — no separate credentials to configure.
 */
export const APP_STATE_PATH = join(homedir(), "Library", "Application Support", "com.hopp.app", "app_state.json");

interface HoppAppState {
  user_jwt?: string | null;
  user_settings?: {
    hopp_server_url?: string | null;
  } | null;
}

export interface Teammate {
  id: string;
  first_name: string;
  last_name: string;
  email: string;
  team_name: string;
  avatar_url?: string | null;
  is_active?: boolean;
}

export interface CallPresence {
  peerIds?: string[];
  roomName?: string;
}

export interface CallsPresenceResponse {
  presence: Record<string, CallPresence>;
}

export interface Room {
  id: string;
  name: string;
  user_id: string;
}

export interface RoomsPresenceResponse {
  rooms: Record<string, string[]>;
}

interface Preferences {
  serverUrl?: string;
}

/** Raised when no signed-in Hopp session can be found on this machine. */
export class NotSignedInError extends Error {}

export interface HoppConnection {
  serverUrl: string;
  headers: Record<string, string>;
}

/**
 * Resolves the backend URL and session token to talk to the Hopp API.
 * The server URL preference (when set) wins over the one stored in the app.
 */
export function getHoppConnection(): HoppConnection {
  const { serverUrl: serverUrlPreference } = getPreferenceValues<Preferences>();

  let state: HoppAppState | null = null;
  try {
    if (existsSync(APP_STATE_PATH)) {
      state = JSON.parse(readFileSync(APP_STATE_PATH, "utf-8")) as HoppAppState;
    }
  } catch {
    // Fall through to the defaults below.
  }

  const serverUrl = sanitizeServerUrl(
    serverUrlPreference || state?.user_settings?.hopp_server_url || DEFAULT_SERVER_URL,
  );

  const token = state?.user_jwt;
  if (!token) {
    throw new NotSignedInError("Couldn't find a signed-in Hopp session. Open the Hopp app and sign in first.");
  }

  return {
    serverUrl,
    headers: { Authorization: `Bearer ${token}` },
  };
}

function sanitizeServerUrl(url: string): string {
  return url
    .trim()
    .replace(/^https?:\/\//, "")
    .replace(/\/+$/, "");
}

/**
 * Deep links handled by the desktop app. See `tauri/src/lib/deepLinkUtils.ts`
 * for the handlers backing these.
 */
export const deepLink = {
  call: (userId: string) => `hopp:///call/${userId}`,
  joinCall: (userId: string) => `hopp:///join-call/${userId}`,
  room: (roomId: string) => `hopp:///room/${roomId}`,
};

export function teammateName(teammate: Teammate): string {
  return `${teammate.first_name} ${teammate.last_name}`.trim() || teammate.email;
}

/** Human-readable presence line shown next to a teammate. */
export function presenceText(teammate: Teammate, presence?: CallPresence): string {
  if (presence?.roomName) return `In ${presence.roomName}`;
  if (presence) return "In a call";
  return teammate.is_active ? "Online" : "Offline";
}
