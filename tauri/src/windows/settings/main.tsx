import "../../App.css";
import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { useDisableNativeContextMenu, useSystemTheme } from "@/lib/hooks";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { typedInvoke } from "@/core_payloads";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { tauriUtils } from "@/windows/window-utils";
import { URLS } from "@/constants";
import posthog from "posthog-js";

const queryClient = new QueryClient();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <SettingsWindow />
    </QueryClientProvider>
  </React.StrictMode>,
);

function CheckboxRow({
  title,
  description,
  checked,
  onCheckedChange,
}: {
  title: string;
  description: string;
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <label className="flex items-start gap-2 cursor-pointer">
      <Checkbox
        className="mt-0.5 rounded border-gray-300 dark:border-gray-600"
        checked={checked}
        onCheckedChange={(v) => onCheckedChange(v === true)}
      />
      <div className="flex flex-col">
        <span className="text-sm font-medium text-gray-700 dark:text-gray-300">{title}</span>
        <span className="text-sm text-gray-500 dark:text-gray-400">{description}</span>
      </div>
    </label>
  );
}

function SettingsWindow() {
  useDisableNativeContextMenu();
  useSystemTheme();

  const [serverUrl, setServerUrl] = useState("");

  const { data: settings, refetch: refetchSettings } = useQuery({
    queryKey: ["user-settings"],
    queryFn: () => typedInvoke("get_user_settings"),
    select: (data) => data,
    refetchOnWindowFocus: true,
  });

  useEffect(() => {
    if (settings) {
      setServerUrl(settings.hopp_server_url ?? "");
    }
  }, [settings]);

  if (!settings) return null;

  return (
    <div className="h-full min-h-full text-black dark:text-white flex flex-col">
      <div data-tauri-drag-region className="h-[32px] min-w-full w-full" />

      <div className="flex-1 flex flex-col px-5 pb-5 py-4">
        <h1 className="text-[22px] font-semibold mb-6  text-black dark:text-white">Settings</h1>

        <div className="flex flex-col gap-5">
          <div className="grid grid-cols-[minmax(100px,140px)_1fr] gap-8">
            <h3 className="text-base font-medium text-black dark:text-white">Call settings</h3>
            <div className="flex flex-col gap-3">
              <CheckboxRow
                title="Call feedback popup"
                description="Show a feedback popup when call ends"
                checked={settings.call_feedback_popup}
                onCheckedChange={(v) => {
                  typedInvoke("set_call_feedback_popup", { enabled: v }).then(() => refetchSettings());
                }}
              />
              <CheckboxRow
                title="Show dock icon when in call"
                description="Hide dock icon to save space when you are in a call"
                checked={settings.show_dock_icon_in_call}
                onCheckedChange={(v) => {
                  typedInvoke("set_show_dock_icon_in_call", { enabled: v }).then(() => refetchSettings());
                }}
              />
            </div>
          </div>

          <hr className="h-px w-full border-none bg-gray-300 dark:bg-gray-600" />

          <div className="grid grid-cols-[minmax(100px,140px)_1fr] gap-8">
            <h3 className="text-base font-medium text-black dark:text-white">Camera settings</h3>
            <div className="flex flex-col gap-3">
              <CheckboxRow
                title="Start camera on call start"
                description="Open camera when you start the call"
                checked={settings.start_camera_on_call}
                onCheckedChange={(v) => {
                  typedInvoke("set_start_camera_on_call", { enabled: v }).then(() => refetchSettings());
                }}
              />
            </div>
          </div>

          <hr className="h-px w-full border-none bg-gray-300 dark:bg-gray-600" />

          <div className="grid grid-cols-[minmax(100px,140px)_1fr] gap-8">
            <h3 className="text-base font-medium text-black dark:text-white">Audio settings</h3>
            <div className="flex flex-col gap-3">
              <CheckboxRow
                title="Start microphone on call start"
                description="Unmute microphone when you start the call"
                checked={settings.start_mic_on_call}
                onCheckedChange={(v) => {
                  typedInvoke("set_start_mic_on_call", { enabled: v }).then(() => refetchSettings());
                }}
              />
            </div>
          </div>

          <hr className="h-px w-full border-none bg-gray-300 dark:bg-gray-600" />

          <div className="grid grid-cols-[minmax(100px,140px)_1fr] gap-8">
            <h3 className="text-base font-medium text-black dark:text-white">Miscellaneous</h3>
            <div className="flex flex-col gap-3">
              <div className="flex flex-col gap-1">
                <span className="text-sm font-medium text-gray-700 dark:text-gray-300">Custom Backend URL</span>
                <span className="text-sm text-gray-500 dark:text-gray-400">
                  Change backend server. Leave empty to use default.
                </span>
                <Input
                  type="text"
                  placeholder={URLS.API_BASE_URL}
                  value={serverUrl}
                  onChange={(e) => setServerUrl(e.target.value)}
                  onKeyDown={async (e) => {
                    if (e.key === "Enter") {
                      const trimmed = serverUrl.trim() || null;
                      await tauriUtils.setHoppServerUrl(trimmed);
                      posthog.capture("custom_backend_url_changed");
                      refetchSettings();
                    }
                  }}
                  onBlur={async () => {
                    const trimmed = serverUrl.trim() || null;
                    if (trimmed !== settings.hopp_server_url) {
                      await tauriUtils.setHoppServerUrl(trimmed);
                      posthog.capture("custom_backend_url_changed");
                      refetchSettings();
                    }
                  }}
                />
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
