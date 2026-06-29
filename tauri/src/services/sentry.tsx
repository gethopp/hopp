import { SENTRY_DSN } from "@/constants";
import * as Sentry from "@sentry/react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { typedInvoke } from "@/core_payloads";

// Helper function to set window context
export const setWindowContext = async () => {
  try {
    const currentWindow = getCurrentWindow();
    const windowLabel = currentWindow.label;

    Sentry.setTag("window", windowLabel);
    Sentry.setContext("window", {
      name: windowLabel,
      title: await currentWindow.title(),
    });
  } catch (error) {
    // Fallback for non-Tauri environments or errors
    Sentry.setTag("window", "unknown");
    Sentry.setContext("window", {
      name: "unknown",
      timestamp: new Date().toISOString(),
      error: error instanceof Error ? error.message : "Failed to get window info",
    });
  }
};

// Initialize Sentry and set up window context
const sentryConfig: Sentry.BrowserOptions = {
  dsn: SENTRY_DSN,
  integrations: [
    Sentry.captureConsoleIntegration({
      levels: ["error"],
    }),
    Sentry.browserTracingIntegration(),
  ],
  // Learn more at
  // https://docs.sentry.io/platforms/javascript/session-replay/configuration/#general-integration-configuration
  replaysSessionSampleRate: 0.1,
  replaysOnErrorSampleRate: 1.0,
  tracesSampleRate: 1.0,
};

let sentryEnabled = false;

export const disableSentry = () => {
  if (sentryEnabled) {
    Sentry.close();
    sentryEnabled = false;
  }
};

export const enableSentry = async () => {
  if (!sentryEnabled) {
    Sentry.init(sentryConfig);
    await setWindowContext();
    sentryEnabled = true;
  }
};

typedInvoke("get_user_settings").then((settings) => {
  if (settings.telemetry_enabled) {
    enableSentry();
  }
});
