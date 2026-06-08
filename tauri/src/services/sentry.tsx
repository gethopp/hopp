import { SENTRY_DSN } from "@/constants";
import * as Sentry from "@sentry/react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { typedInvoke } from "@/core_payloads";

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
    Sentry.setTag("window", "unknown");
    Sentry.setContext("window", {
      name: "unknown",
      timestamp: new Date().toISOString(),
      error: error instanceof Error ? error.message : "Failed to get window info",
    });
  }
};

const sentryConfig: Sentry.BrowserOptions = {
  dsn: SENTRY_DSN,
  integrations: [
    Sentry.captureConsoleIntegration({
      levels: ["error"],
    }),
    Sentry.browserTracingIntegration(),
  ],
  replaysSessionSampleRate: 0.1,
  replaysOnErrorSampleRate: 1.0,
  tracesSampleRate: 1.0,
};

Sentry.init(sentryConfig);
setWindowContext();

export const disableSentry = () => {
  Sentry.close();
};

export const enableSentry = () => {
  Sentry.init(sentryConfig);
};

typedInvoke("get_user_settings").then((settings) => {
  if (!settings.telemetry_enabled) {
    disableSentry();
  }
});
