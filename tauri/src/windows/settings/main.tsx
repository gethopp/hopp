import "../../App.css";
import React, { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { useDisableNativeContextMenu } from "@/lib/hooks";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
  typedInvoke,
  type AppVeilApplication,
  type InstalledApplication,
  type ScreenSharePickerMode,
  type ScreenShareResolution,
} from "@/core_payloads";
import { QueryClient, QueryClientProvider, useQuery } from "@tanstack/react-query";
import { tauriUtils } from "@/windows/window-utils";
import { OS, URLS } from "@/constants";
import posthog from "posthog-js";
import useStore from "@/store/store";

const queryClient = new QueryClient();

const screenShareResolutionItems = [
  { id: "P1080", title: "1080p", description: "Lowest latency" },
  { id: "P1440", title: "1440p", description: "Balance latency and resolution" },
  { id: "P4K", title: "4K", description: "Maximum resolution" },
] satisfies Array<{ id: ScreenShareResolution; title: string; description: string }>;

const screenSharePickerModeItems = [
  { id: "Screen", title: "Screen", description: "Choose an entire display" },
  { id: "Window", title: "Window", description: "Choose a single app window" },
] satisfies Array<{ id: ScreenSharePickerMode; title: string; description: string }>;

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

function ResolutionRow({
  value,
  onValueChange,
}: {
  value: ScreenShareResolution;
  onValueChange: (value: ScreenShareResolution) => void;
}) {
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col">
        <span className="text-sm font-medium text-gray-700 dark:text-gray-300">Screen share resolution</span>
        <span className="text-sm text-gray-500 dark:text-gray-400">Choose stream quality for screen sharing</span>
      </div>
      <div className="flex flex-col gap-2">
        {screenShareResolutionItems.map((item) => (
          <label key={item.id} className="flex items-start gap-2 cursor-pointer">
            <input
              type="radio"
              name="screen-share-resolution"
              className="mt-0.5 size-4 cursor-pointer accent-slate-700 dark:accent-slate-300"
              checked={value === item.id}
              onChange={(event) => {
                if (event.target.checked && value !== item.id) {
                  onValueChange(item.id);
                }
              }}
            />
            <div className="flex flex-col">
              <span className="text-sm font-medium text-gray-700 dark:text-gray-300">{item.title}</span>
              <span className="text-sm text-gray-500 dark:text-gray-400">{item.description}</span>
            </div>
          </label>
        ))}
      </div>
    </div>
  );
}

function PickerModeRow({
  value,
  onValueChange,
}: {
  value: ScreenSharePickerMode;
  onValueChange: (value: ScreenSharePickerMode) => void;
}) {
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-col">
        <span className="text-sm font-medium text-gray-700 dark:text-gray-300">Default picker mode</span>
        <span className="text-sm text-gray-500 dark:text-gray-400">
          Choose what to select when screen sharing starts
        </span>
      </div>
      <div className="flex flex-col gap-2">
        {screenSharePickerModeItems.map((item) => (
          <label key={item.id} className="flex items-start gap-2 cursor-pointer">
            <input
              type="radio"
              name="screen-share-picker-mode"
              className="mt-0.5 size-4 cursor-pointer accent-slate-700 dark:accent-slate-300"
              checked={value === item.id}
              onChange={(event) => {
                if (event.target.checked && value !== item.id) {
                  onValueChange(item.id);
                }
              }}
            />
            <div className="flex flex-col">
              <span className="text-sm font-medium text-gray-700 dark:text-gray-300">{item.title}</span>
              <span className="text-sm text-gray-500 dark:text-gray-400">{item.description}</span>
            </div>
          </label>
        ))}
      </div>
    </div>
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

function applicationIcon(application: InstalledApplication): string | undefined {
  if (!application.icon_png) return undefined;
  let binary = "";
  for (const byte of application.icon_png) binary += String.fromCharCode(byte);
  return `data:image/png;base64,${btoa(binary)}`;
}

function AppVeilSettings({
  rows,
  installedApplications,
  onChange,
}: {
  rows: AppVeilApplication[];
  installedApplications: InstalledApplication[];
  onChange: (rows: AppVeilApplication[]) => Promise<void>;
}) {
  const [query, setQuery] = useState("");
  const [highlighted, setHighlighted] = useState(0);
  const [saving, setSaving] = useState(false);
  const addedBundleIds = new Set(rows.map((row) => row.bundle_id));
  const normalizedQuery = query.trim().toLocaleLowerCase();
  const results =
    normalizedQuery ?
      installedApplications
        .filter((application) => application.name.toLocaleLowerCase().includes(normalizedQuery))
        .slice(0, 8)
    : [];
  const nameCounts = installedApplications.reduce<Record<string, number>>((counts, application) => {
    counts[application.name] = (counts[application.name] ?? 0) + 1;
    return counts;
  }, {});
  const commit = async (nextRows: AppVeilApplication[]) => {
    if (saving) return;
    setSaving(true);
    try {
      await onChange(nextRows);
    } catch (error) {
      console.error("Failed to update App Veil settings", error);
    } finally {
      setSaving(false);
    }
  };

  const add = async (application: InstalledApplication) => {
    if (addedBundleIds.has(application.bundle_id)) return;
    await commit([...rows, { bundle_id: application.bundle_id, enabled: true }]);
    setQuery("");
    setHighlighted(0);
  };

  return (
    <div className="flex flex-col gap-3">
      <span className="text-sm text-gray-500 dark:text-gray-400">
        Hide selected applications from viewers when sharing a screen. You will still see and use them normally.
      </span>

      <div className="relative">
        <Input
          aria-label="Add application to App Veil"
          role="combobox"
          aria-expanded={results.length > 0}
          aria-controls="app-veil-results"
          aria-activedescendant={results[highlighted] ? `app-veil-result-${highlighted}` : undefined}
          placeholder="Add application…"
          value={query}
          disabled={saving}
          onChange={(event) => {
            setQuery(event.target.value);
            setHighlighted(0);
          }}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              setQuery("");
              setHighlighted(0);
            } else if (event.key === "ArrowDown" && results.length) {
              event.preventDefault();
              setHighlighted((index) => (index + 1) % results.length);
            } else if (event.key === "ArrowUp" && results.length) {
              event.preventDefault();
              setHighlighted((index) => (index + results.length - 1) % results.length);
            } else if (event.key === "Enter" && results[highlighted]) {
              event.preventDefault();
              const selected = results[highlighted];
              if (selected) void add(selected);
            }
          }}
        />
        {normalizedQuery && (
          <div
            id="app-veil-results"
            role="listbox"
            className="absolute z-20 mt-1 max-h-56 w-full overflow-y-auto rounded-md border border-gray-200 bg-white p-1 shadow-lg dark:border-gray-700 dark:bg-gray-900"
          >
            {results.length === 0 ?
              <div className="px-2 py-3 text-sm text-gray-500">No installed applications found.</div>
            : results.map((application, index) => {
                const added = addedBundleIds.has(application.bundle_id);
                const icon = applicationIcon(application);
                return (
                  <button
                    id={`app-veil-result-${index}`}
                    role="option"
                    aria-selected={index === highlighted}
                    key={application.bundle_id}
                    disabled={added || saving}
                    className={`flex w-full items-center gap-2 rounded px-2 py-2 text-left text-sm ${
                      index === highlighted ? "bg-gray-100 dark:bg-gray-800" : ""
                    } disabled:opacity-60`}
                    onMouseEnter={() => setHighlighted(index)}
                    onClick={() => void add(application)}
                  >
                    {icon && <img src={icon} alt="" className="size-7 shrink-0" />}
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-gray-700 dark:text-gray-300">{application.name}</span>
                      {(nameCounts[application.name] ?? 0) > 1 && (
                        <span className="block truncate text-xs text-gray-500">{application.bundle_id}</span>
                      )}
                    </span>
                    {added && <span className="text-xs text-gray-500">Added</span>}
                  </button>
                );
              })
            }
          </div>
        )}
      </div>

      <div className="max-h-[220px] overflow-y-auto rounded-md border border-gray-200 dark:border-gray-700">
        {rows.length === 0 ?
          <div className="px-3 py-4 text-sm text-gray-500">No applications are hidden yet.</div>
        : rows.map((row) => {
            const application = installedApplications.find((candidate) => candidate.bundle_id === row.bundle_id);
            const name = application?.name ?? "Application not found";
            const icon = application && applicationIcon(application);
            return (
              <div
                key={row.bundle_id}
                className="group flex min-h-11 items-center gap-2 border-b border-gray-200 px-3 py-2 last:border-b-0 dark:border-gray-700"
              >
                {icon ?
                  <img src={icon} alt="" className="size-7 shrink-0" />
                : <div aria-hidden className="size-7 shrink-0 rounded bg-gray-100 dark:bg-gray-800" />}
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-sm text-gray-700 dark:text-gray-300">{name}</span>
                  {!application && <span className="block truncate text-xs text-gray-500">{row.bundle_id}</span>}
                </span>
                <Switch
                  aria-label={`Hide ${application?.name ?? row.bundle_id} from viewers`}
                  checked={row.enabled}
                  disabled={!application || saving}
                  onCheckedChange={(enabled) =>
                    void commit(
                      rows.map((candidate) =>
                        candidate.bundle_id === row.bundle_id ? { ...candidate, enabled } : candidate,
                      ),
                    )
                  }
                />
                <button
                  type="button"
                  aria-label={`Remove ${application?.name ?? row.bundle_id} from App Veil`}
                  disabled={saving}
                  className="size-7 rounded text-lg text-gray-500 opacity-0 hover:bg-gray-100 focus:opacity-100 focus:outline-none focus:ring-2 focus:ring-gray-400 group-hover:opacity-100 dark:hover:bg-gray-800"
                  onClick={() => void commit(rows.filter((candidate) => candidate.bundle_id !== row.bundle_id))}
                >
                  −
                </button>
              </div>
            );
          })
        }
      </div>
    </div>
  );
}

type SectionId = "call" | "app-veil" | "shortcuts" | "misc";

const SECTIONS: { id: SectionId; title: string; macOnly?: boolean }[] = [
  { id: "call", title: "Call settings" },
  { id: "app-veil", title: "App Veil", macOnly: true },
  { id: "shortcuts", title: "Shortcuts" },
  { id: "misc", title: "Miscellaneous" },
];

function SettingsWindow() {
  useDisableNativeContextMenu();

  const [serverUrl, setServerUrl] = useState("");
  const [section, setSection] = useState<SectionId>("call");

  const { data: settings, refetch: refetchSettings } = useQuery({
    queryKey: ["user-settings"],
    queryFn: () => typedInvoke("get_user_settings"),
    select: (data) => data,
    refetchOnWindowFocus: true,
  });

  const { data: installedApplications = [] } = useQuery({
    queryKey: ["installed-applications"],
    queryFn: () => typedInvoke("list_installed_applications"),
    enabled: OS === "macos",
    staleTime: Infinity,
  });

  useEffect(() => {
    if (settings) {
      setServerUrl(settings.hopp_server_url ?? "");
    }
  }, [settings]);

  async function commitShortcut(which: "mic" | "camera" | "screenshare" | "end_call", accel: string) {
    const setters = {
      mic: "set_shortcut_toggle_mic",
      camera: "set_shortcut_toggle_camera",
      screenshare: "set_shortcut_toggle_screenshare",
      end_call: "set_shortcut_end_call",
    } as const;

    if (settings) {
      const others = (["mic", "camera", "screenshare", "end_call"] as const).filter((k) => k !== which);
      for (const other of others) {
        const otherVal =
          other === "mic" ? settings.shortcut_toggle_mic
          : other === "camera" ? settings.shortcut_toggle_camera
          : other === "screenshare" ? settings.shortcut_toggle_screenshare
          : settings.shortcut_end_call;
        if (otherVal === accel) {
          await typedInvoke(setters[other], { accel: "" });
        }
      }
    }

    await typedInvoke(setters[which], { accel });
    refetchSettings();
  }

  if (!settings) return null;

  const visibleSections = SECTIONS.filter((s) => !s.macOnly || OS === "macos");

  return (
    <div className="h-full min-h-full overflow-hidden text-black dark:text-white flex flex-col">
      <div data-tauri-drag-region className="h-[32px] min-w-full w-full" />

      <div className="flex-1 flex flex-col min-h-0 px-5 pb-5 py-4">
        <h1 className="text-[22px] font-semibold mb-6 text-black dark:text-white">Settings</h1>

        <div className="flex-1 flex min-h-0 gap-8">
          <nav className="w-[150px] shrink-0 flex flex-col gap-1">
            {visibleSections.map((s) => (
              <button
                key={s.id}
                onClick={() => setSection(s.id)}
                className={`rounded-md px-3 py-1.5 text-left text-sm ${
                  section === s.id ?
                    "bg-gray-200 font-medium text-black dark:bg-gray-700 dark:text-white"
                  : "text-gray-500 hover:bg-gray-100 dark:text-gray-400 dark:hover:bg-gray-800"
                }`}
              >
                {s.title}
              </button>
            ))}
          </nav>

          <main className="flex-1 min-w-0 overflow-y-auto pr-1">
            {section === "call" && (
              <div className="flex flex-col gap-5">
                <div className="flex flex-col gap-3">
                  <h3 className="text-base font-medium text-black dark:text-white">Call settings</h3>
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

                <hr className="h-px w-full border-none bg-gray-300 dark:bg-gray-600" />

                <div className="flex flex-col gap-3">
                  <h3 className="text-base font-medium text-black dark:text-white">Camera settings</h3>
                  <CheckboxRow
                    title="Start camera on call start"
                    description="Open camera when you start the call"
                    checked={settings.start_camera_on_call}
                    onCheckedChange={(v) => {
                      typedInvoke("set_start_camera_on_call", { enabled: v }).then(() => refetchSettings());
                    }}
                  />
                </div>

                <hr className="h-px w-full border-none bg-gray-300 dark:bg-gray-600" />

                <div className="flex flex-col gap-3">
                  <h3 className="text-base font-medium text-black dark:text-white">Audio settings</h3>
                  <CheckboxRow
                    title="Start microphone on call start"
                    description="Unmute microphone when you start the call"
                    checked={settings.start_mic_on_call}
                    onCheckedChange={(v) => {
                      typedInvoke("set_start_mic_on_call", { enabled: v }).then(() => refetchSettings());
                    }}
                  />
                  <CheckboxRow
                    title="Noise cancellation"
                    description="Noise suppression on microphone input"
                    checked={settings.noise_cancellation_enabled}
                    onCheckedChange={(v) => {
                      typedInvoke("set_noise_cancellation", { enabled: v }).then(() => refetchSettings());
                    }}
                  />
                </div>

                <hr className="h-px w-full border-none bg-gray-300 dark:bg-gray-600" />

                <div className="flex flex-col gap-3">
                  <h3 className="text-base font-medium text-black dark:text-white">Screen share settings</h3>
                  <CheckboxRow
                    title="Enable remote control by default"
                    description="Allow teammates to control your computer when screen sharing starts"
                    checked={settings.remote_control_enabled}
                    onCheckedChange={(v) => {
                      typedInvoke("set_remote_control_enabled", { enabled: v }).then(() => refetchSettings());
                    }}
                  />
                  <ResolutionRow
                    value={settings.screen_share_resolution}
                    onValueChange={(resolution) => {
                      typedInvoke("set_screen_share_resolution", { resolution }).then(() => refetchSettings());
                    }}
                  />
                  {OS === "macos" && (
                    <PickerModeRow
                      value={settings.screen_share_picker_mode}
                      onValueChange={(mode) => {
                        typedInvoke("set_screen_share_picker_mode", { mode }).then(() => refetchSettings());
                      }}
                    />
                  )}
                </div>
              </div>
            )}

            {section === "app-veil" && OS === "macos" && (
              <AppVeilSettings
                rows={settings.app_veil_applications}
                installedApplications={installedApplications}
                onChange={async (applications) => {
                  try {
                    await typedInvoke("set_app_veil_applications", { applications });
                  } finally {
                    await refetchSettings();
                  }
                }}
              />
            )}

            {section === "shortcuts" && (
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
                <ShortcutRow
                  title="End call"
                  description="Leave the current call"
                  value={settings.shortcut_end_call}
                  onCommit={(accel) => commitShortcut("end_call", accel)}
                />
              </div>
            )}

            {section === "misc" && (
              <div className="flex flex-col gap-3">
                <CheckboxRow
                  title="Send anonymous telemetry"
                  description="Help improve Hopp by sending anonymous usage data and error reports"
                  checked={settings.telemetry_enabled}
                  onCheckedChange={(v) => {
                    typedInvoke("set_telemetry_enabled", { enabled: v }).then(() => {
                      refetchSettings();
                      if (v) {
                        posthog.opt_in_capturing();
                      } else {
                        posthog.opt_out_capturing();
                      }
                    });
                  }}
                />
                {OS === "macos" && (
                  <CheckboxRow
                    title="Automatic updates"
                    description="Download and install updates automatically when you're not in a call"
                    checked={settings.auto_update_enabled}
                    onCheckedChange={(v) => {
                      typedInvoke("set_auto_update_enabled", { enabled: v }).then(() => refetchSettings());
                    }}
                  />
                )}
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
                        useStore.getState().setCustomServerUrl(trimmed);
                        posthog.capture("custom_backend_url_changed");
                        refetchSettings();
                      }
                    }}
                    onBlur={async () => {
                      const trimmed = serverUrl.trim() || null;
                      if (trimmed !== settings.hopp_server_url) {
                        await tauriUtils.setHoppServerUrl(trimmed);
                        useStore.getState().setCustomServerUrl(trimmed);
                        posthog.capture("custom_backend_url_changed");
                        refetchSettings();
                      }
                    }}
                  />
                </div>
              </div>
            )}
          </main>
        </div>
      </div>
    </div>
  );
}
