import "@/services/sentry";
/**
 * Core JS polyfills to allow for compatibility with Safari
 * on cases like conditional spreading of elements in Array.
 * (Example in Sidebar.tsx)
 */
import "core-js/actual/iterator";
import "../../App.css";
import { OS } from "@/constants";
import { disableWebViewAppNap } from "@/lib/utils";
import React from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { QueryProvider } from "@/services/query";
import App from "./app";
import { Toaster } from "react-hot-toast";
import { PostHogProvider } from "posthog-js/react";
import { PostHogConfig } from "posthog-js";
import { BOTTOM_ARROW, POSTHOG_API_KEY, POSTHOG_HOST } from "@/constants";

const options: Partial<PostHogConfig> = {
  api_host: POSTHOG_HOST,
  // Commenting out until we figure out WTF is going on
  // api_host: "https://webhook.site/4ce330a4-4bb3-497c-9cfe-997515e9093b",
  // autocapture: false,
  loaded: function (ph) {
    if (import.meta.env.MODE == "development") {
      ph.opt_out_capturing(); // opts a user out of event capture
      ph.set_config({ disable_session_recording: true });
    }
  },
};

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      refetchOnWindowFocus: true,
    },
  },
});

if (BOTTOM_ARROW) {
  document.body.className = "arrow_bottom";
} else {
  document.body.className = "arrow";
}

if (OS === "macos") {
  disableWebViewAppNap();
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <PostHogProvider apiKey={POSTHOG_API_KEY} options={options}>
      <Toaster
        position="bottom-right"
        toastOptions={{
          duration: 1_500,
          loading: { duration: Infinity },
        }}
      />
      <QueryClientProvider client={queryClient}>
        {/* Custom type-safe provider */}
        <QueryProvider>
          <App />
        </QueryProvider>
      </QueryClientProvider>
    </PostHogProvider>
  </React.StrictMode>,
);
