import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import { invoke } from "@tauri-apps/api/core";
import { components } from "@/openapi";
import { isEqual } from "lodash";
import { emit, listen } from "@tauri-apps/api/event";
import { TCallTokensMessage } from "@/payloads";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { CoreParticipantState, CoreRoleEvent } from "@/core_payloads";

const windowName = getCurrentWindow().label;

export const SidebarTabs = ["user-list", "invite", "debug", "login", "report-issue", "rooms"] as const;
export type Tab = (typeof SidebarTabs)[number];

export enum ParticipantRole {
  SHARER = "sharer",
  CONTROLLER = "controller",
  NONE = "none",
}

export type CallState = {
  timeStarted: Date;
  hasAudioEnabled: boolean;
  hasCameraEnabled: boolean;
  role: ParticipantRole;
  isRemoteControlEnabled: boolean;
  isRoomCall?: boolean;
  room?: components["schemas"]["Room"];
  isReconnecting?: boolean;
  isInitialisingCall?: boolean;
  participants: CoreParticipantState[];
  micLevel: number;
} & TCallTokensMessage["payload"];

type State = {
  authToken: string | null;
  needsUpdate: boolean;
  updateInProgress: boolean;
  tab: Tab;
  socketConnected: boolean;
  user: components["schemas"]["PrivateUser"] | null;
  teammates: components["schemas"]["BaseUser"][] | null;
  // The targeted user id (callee)
  calling: string | null;
  // Call tokens for LiveKit
  callTokens: CallState | null;
  customServerUrl: string | null;
  livekitUrl: string | null;
};

type Actions = {
  setAuthToken: (token: string | null) => void;
  setNeedsUpdate: (needsUpdate: boolean) => void;
  setUpdateInProgress: (inProgress: boolean) => void;
  setTab: (tab: Tab) => void;
  setSocketConnected: (connected: boolean) => void;
  setUser: (user: components["schemas"]["PrivateUser"] | null) => void;
  setTeammates: (teammates: components["schemas"]["BaseUser"][] | null) => void;
  getStoredToken: () => Promise<string | null>;
  reset: () => void;
  setCalling: (calling: string | null) => void;
  setCallTokens: (tokens: CallState | null) => void;
  // TODO(@konsalex): Rename `xxCallToken` as its not
  // representative anymore or the actual state it holds.
  updateCallTokens: (tokens: Partial<CallState>) => void;
  setCustomServerUrl: (url: string | null) => void;
  setLivekitUrl: (url: string | null) => void;
};

const initialState: State = {
  authToken: null,
  needsUpdate: false,
  updateInProgress: false,
  tab: "login",
  socketConnected: false,
  user: null,
  teammates: null,
  calling: null,
  callTokens: null,
  customServerUrl: null,
  livekitUrl: null,
};

/**
 * NOTE TO FUTURE SELF:
 *
 * The values in the state, even if they are "Date",
 * as they are serialized as strings in the store for persistence
 * and sending across windows, they are not saved as native JS objects.
 */
const useStore = create<State & Actions>()(
  immer((set) => ({
    // State
    ...initialState,
    // Actions
    setAuthToken: (token) =>
      set((state) => {
        state.authToken = token;
      }),
    setUser: (user) =>
      set((state) => {
        state.user = user;
      }),
    setTeammates: (teammates) =>
      set((state) => {
        state.teammates = teammates;
      }),
    setNeedsUpdate: (needsUpdate) =>
      set((state) => {
        state.needsUpdate = needsUpdate;
      }),
    setUpdateInProgress: (inProgress) =>
      set((state) => {
        state.updateInProgress = inProgress;
      }),
    setCalling: (calling) =>
      set((state) => {
        state.calling = calling;
      }),
    setCallTokens: (tokens) =>
      set((state) => {
        state.callTokens = tokens ? { ...tokens, micLevel: tokens.micLevel ?? 0 } : null;
      }),
    updateCallTokens: (tokens) =>
      set((state) => {
        if (!state.callTokens) return;
        state.callTokens = { ...state.callTokens, ...tokens };
      }),
    getStoredToken: async () => {
      return await invoke<string | null>("get_stored_token");
    },
    setCustomServerUrl: (url) =>
      set((state) => {
        state.customServerUrl = url;
      }),
    setLivekitUrl: (url) =>
      set((state) => {
        state.livekitUrl = url;
      }),
    setTab: (tab) =>
      set((state) => {
        state.tab = tab;
      }),
    setSocketConnected: (connected) =>
      set((state) => {
        state.socketConnected = connected;
      }),
    reset: () =>
      set((state) => {
        // First clear the auth token to prevent re-fetching
        // Then reset all other state properties
        Object.assign(state, {
          ...initialState,
        });
      }),
  })),
);

/**
 * Below the logic is for the state sync between windows
 * This is a workaround for the fact that zustand is a singleton
 * and we need to sync the state between windows.
 *
 * This can take us so far, it does not incorporate for weird edge cases
 * and clients that may need to be updated at the same time.
 */
let isProcessingUpdate = false;

// Subscribe to all state changes and broadcast them
useStore.subscribe((state, prevState) => {
  // Don't emit if we're currently processing an update from another window
  if (isProcessingUpdate) return;
  if (windowName === "main" && !isEqual(state, prevState)) {
    emit("store-update", state);
  }

  // Update tray icon and sleep prevention based on call state (only from main window to avoid duplicates)
  if (windowName === "main") {
    const wasInCall = prevState.callTokens !== null;
    const isInCall = state.callTokens !== null;

    if (!wasInCall && isInCall) {
      // Entering a call - show notification dot
      invoke("set_tray_notification", { enabled: true }).catch((e) => {
        console.error("Failed to set tray notification:", e);
      });
      invoke("toggle_call_sleep_prevention", { enabled: true }).catch((e) => {
        console.error("Failed to enable sleep prevention:", e);
      });
    } else if (wasInCall && !isInCall) {
      // Leaving a call - hide notification dot
      invoke("set_tray_notification", { enabled: false }).catch((e) => {
        console.error("Failed to set tray notification:", e);
      });
      invoke("toggle_call_sleep_prevention", { enabled: false }).catch((e) => {
        console.error("Failed to disable sleep prevention:", e);
      });
    }
  }
});

// Set up listener for store updates from other windows
listen("store-update", (event) => {
  // Main is the source of truth; ignore cross-window writes into main.
  if (windowName === "main") return;

  const newState = event.payload as State;
  // Only update if the state is different
  if (!isEqual(useStore.getState(), newState)) {
    isProcessingUpdate = true;
    useStore.setState(newState);
    isProcessingUpdate = false;
  } else {
    console.debug("Store did not update, state is the same");
  }
});

// Request current state from other windows when initializing
let hasReceivedInitialState = false;
listen("get-store-response", async (event) => {
  // Main does not hydrate from peers.
  if (windowName === "main") return;

  // The below replicates the race-condition issue
  // alongside swapping `get-store` emit/listen order
  // await new Promise((resolve) => setTimeout(resolve, 200));
  // console.log(`Received state from ${event.payload.window}`);
  const newState = (event.payload as any).state as State;
  if (!hasReceivedInitialState) {
    hasReceivedInitialState = true;
    useStore.setState(newState);
  }
});

// Request initial state from other windows
emit("get-store");

// Listen for state requests from new windows
listen("get-store", () => {
  // Only main responds with canonical state.
  if (windowName !== "main") return;

  emit("get-store-response", {
    state: useStore.getState(),
    window: windowName,
  });
});

// Listen for full participant snapshot from core and replace the participants array.
// Also derive local audio/camera state so the UI stays in sync when toggled from core
// (e.g. camera window mute button).
listen<CoreParticipantState[]>("core_participants_snapshot", (event) => {
  // Core snapshots should be processed by main window only.
  // Non-main windows can race and rebroadcast stale callTokens via store sync.
  if (windowName !== "main") return;

  const { callTokens, user } = useStore.getState();
  if (!callTokens) return;

  const updates: Partial<CallState> = { participants: event.payload, isInitialisingCall: false };

  if (user) {
    const localParticipant = event.payload.find((p) => p.identity.includes("local"));
    if (localParticipant) {
      updates.hasCameraEnabled = localParticipant.has_camera;
      updates.hasAudioEnabled = !localParticipant.muted;

      if (localParticipant.is_screensharing) {
        updates.role = ParticipantRole.SHARER;
      } else {
        const someoneElseSharing = event.payload.some((p) => p.is_screensharing && !p.identity.includes(user.id));
        updates.role = someoneElseSharing ? ParticipantRole.CONTROLLER : ParticipantRole.NONE;
      }
    }
  }

  useStore.getState().updateCallTokens(updates);
});

listen<number>("core_mic_audio_level", (event) => {
  // Keep mic level updates single-writer to avoid cross-window state races
  // (aux windows can rebroadcast stale callTokens role via store-sync).
  if (windowName !== "main") return;

  const { callTokens } = useStore.getState();
  if (!callTokens) return;
  if (callTokens.micLevel === event.payload) return;

  useStore.getState().updateCallTokens({ micLevel: event.payload });
});

// Listen for core role change events
listen<CoreRoleEvent>("core_role_change", (event) => {
  // Keep role derivation single-writer in main window.
  if (windowName !== "main") return;

  const { callTokens } = useStore.getState();
  if (!callTokens) return;

  const roleMap: Record<string, ParticipantRole> = {
    Sharer: ParticipantRole.SHARER,
    Controller: ParticipantRole.CONTROLLER,
    None: ParticipantRole.NONE,
  };

  const newRole = roleMap[event.payload.role] ?? ParticipantRole.NONE;
  useStore.getState().updateCallTokens({ role: newRole });
});

export default useStore;
