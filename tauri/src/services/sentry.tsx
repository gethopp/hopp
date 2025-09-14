import { SENTRY_DSN } from "@/constants";
import * as Sentry from "@sentry/react";
import { getCurrentWindow } from "@tauri-apps/api/window";

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
Sentry.init({
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
});

// Set initial window context
setWindowContext();
