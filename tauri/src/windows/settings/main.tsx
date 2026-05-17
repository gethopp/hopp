import "../../App.css";
import React, { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { useDisableNativeContextMenu, useSystemTheme } from "@/lib/hooks";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { typedInvoke } from "@/core_payloads";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { tauriUtils } from "@/windows/window-utils";
import { OS, URLS } from "@/constants";
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

function formatAccel(accel: string): string {
  return accel;
}

function ShortcutRow({
  title,
  description,
  value,
  onCommit,
}: {
  title: string;
  description: string;
  value: string;
  onCommit: (accel: string) => void;
}) {
  const [recording, setRecording] = useState(false);
  const listenerRef = useRef<((e: KeyboardEvent) => void) | null>(null);

  const startRecording = () => {
    setRecording(true);
    const handler = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      if (e.key === "Escape" && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
        stopRecording();
        return;
      }

      if (["Meta", "Control", "Alt", "Shift"].includes(e.key)) return;

      const hasModifier = e.metaKey || e.ctrlKey || e.altKey;
      if (!hasModifier) return;

      const parts: string[] = [];
      if (OS === "macos") {
        if (e.metaKey) parts.push("Cmd");
        if (e.ctrlKey) parts.push("Ctrl");
        if (e.altKey) parts.push("Alt");
        if (e.shiftKey) parts.push("Shift");
      } else {
        if (e.ctrlKey) parts.push("Ctrl");
        if (e.altKey) parts.push("Alt");
        if (e.shiftKey) parts.push("Shift");
      }
      parts.push(e.key.toUpperCase());
      const accel = parts.join("+");

      stopRecording();
      onCommit(accel);
    };

    listenerRef.current = handler;
    window.addEventListener("keydown", handler, true);
  };

  const stopRecording = () => {
    setRecording(false);
    if (listenerRef.current) {
      window.removeEventListener("keydown", listenerRef.current, true);
      listenerRef.current = null;
    }
  };

  return (
    <div className="flex items-start justify-between gap-2">
      <div className="flex flex-col">
        <span className="text-sm font-medium text-gray-700 dark:text-gray-300">{title}</span>
        <span className="text-sm text-gray-500 dark:text-gray-400">{description}</span>
      </div>
      <Button
        variant="outline"
        className="w-[180px] font-mono text-sm justify-start shrink-0"
        onClick={() => (recording ? stopRecording() : startRecording())}
      >
        {recording ?
          <span className="text-gray-400 dark:text-gray-500 text-xs">Recording...</span>
        : <span className="text-gray-600 dark:text-gray-400 text-s">{formatAccel(value)}</span>}
      </Button>
    </div>
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

  async function commitShortcut(which: "mic" | "camera" | "screenshare", accel: string) {
    const setters = {
      mic: "set_shortcut_toggle_mic",
      camera: "set_shortcut_toggle_camera",
      screenshare: "set_shortcut_toggle_screenshare",
    } as const;

    if (settings) {
      const others = (["mic", "camera", "screenshare"] as const).filter((k) => k !== which);
      for (const other of others) {
        const otherVal =
          other === "mic" ? settings.shortcut_toggle_mic
          : other === "camera" ? settings.shortcut_toggle_camera
          : settings.shortcut_toggle_screenshare;
        if (otherVal === accel) {
          await typedInvoke(setters[other], { accel: "" });
        }
      }
    }

    await typedInvoke(setters[which], { accel });
    refetchSettings();
  }

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
            <h3 className="text-base font-medium text-black dark:text-white">Shortcuts</h3>
            <div className="flex flex-col gap-3">
              <ShortcutRow
                title="Mute / unmute mic"
                description="Toggle microphone during call"
                value={settings.shortcut_toggle_mic}
                onCommit={(accel) => commitShortcut("mic", accel)}
              />
              <ShortcutRow
                title="Toggle camera"
                description="Turn camera on or off during call"
                value={settings.shortcut_toggle_camera}
                onCommit={(accel) => commitShortcut("camera", accel)}
              />
              <ShortcutRow
                title="Toggle screen share"
                description="Start or stop screen sharing"
                value={settings.shortcut_toggle_screenshare}
                onCommit={(accel) => commitShortcut("screenshare", accel)}
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
