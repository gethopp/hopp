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

export const BACKEND_URLS = {
  BASE: `https://${URLS.API_BASE_URL}`,
  LOGIN_JWT: `https://${URLS.API_BASE_URL}/login-app`,
} as const;

/**
 * Gets the API base URL (custom or default).
 * Non-reactive - use for non-React code or when you don't need reactivity.
 */
export const getApiBaseUrl = (): string => {
  const customUrl = useStore.getState().customServerUrl;
  return customUrl || URLS.API_BASE_URL;
};

/**
 * Gets the full backend base URL (with https://).
 */
export const getBackendBaseUrl = (): string => {
  return `https://${getApiBaseUrl()}`;
};

/**
 * Gets the websocket URL.
 */
export const getWebsocketUrl = (): string => {
  return `wss://${getApiBaseUrl()}/api/auth/websocket`;
};

// ============ React Hooks (reactive) ============

/**
 * Hook that returns the API base URL and re-renders when it changes.
 */
export const useApiBaseUrl = (): string => {
  const customUrl = useStore((state) => state.customServerUrl);
  return customUrl || URLS.API_BASE_URL;
};

/**
 * Hook that returns the backend base URL and re-renders when it changes.
 */
export const useBackendBaseUrl = (): string => {
  const baseUrl = useApiBaseUrl();
  return `https://${baseUrl}`;
};

/**
 * Hook that returns the login JWT URL and re-renders when it changes.
 */
export const useLoginJwtUrl = (): string => {
  const baseUrl = useApiBaseUrl();
  return `https://${baseUrl}/login-app`;
};
