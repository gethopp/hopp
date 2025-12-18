import useStore from "@/store/store";

// @ts-ignore
export const URLS = {
  API_BASE_URL: import.meta.env.VITE_API_BASE_URL as string,
  DEV_MODE: import.meta.env.MODE === "development",
} as const;

export const BOTTOM_ARROW = import.meta.env.VITE_BOTTOM_ARROW === "true";
export const DEBUGGING_VIDEO_TRACK = false;
export const OS = import.meta.env.VITE_OS as string;
export const SENTRY_DSN = import.meta.env.VITE_SENTRY_DSN_JS as string;
export const POSTHOG_API_KEY = import.meta.env.VITE_POSTHOG_API_KEY as string;
export const POSTHOG_HOST = import.meta.env.VITE_POSTHOG_HOST as string;

/**
 * Constants class providing access to URLs with custom server URL support.
 * Uses static getters to always return the current value from the store.
 */
export class Constants {
  /** The API base URL (custom or default, without protocol). */
  static get apiBaseUrl(): string {
    const customUrl = useStore.getState().customServerUrl;
    return customUrl || URLS.API_BASE_URL;
  }

  /** The full backend base URL with https:// protocol. */
  static get backendUrl(): string {
    return `https://${this.apiBaseUrl}`;
  }

  /** The web app URL for sharing room links. */
  static get webAppUrl(): string {
    return `https://${this.apiBaseUrl}`;
  }

  /** The WebSocket URL for the auth websocket. */
  static get websocketUrl(): string {
    return `wss://${this.apiBaseUrl}/api/auth/websocket`;
  }

  /** The login JWT URL. */
  static get loginJwtUrl(): string {
    return `https://${this.apiBaseUrl}/login-app`;
  }
}
