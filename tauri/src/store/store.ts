import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import { invoke } from "@tauri-apps/api/core";
import { components } from "@/openapi";
import { isEqual } from "lodash";
import { emit, listen } from "@tauri-apps/api/event";
import { TCallTokensMessage } from "@/payloads";
import { getCurrentWindow } from "@tauri-apps/api/window";

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
  // Managing buttons for starting/joining/terminating screenshare streams
  role: ParticipantRole;
  isRemoteControlEnabled: boolean;
  isRoomCall?: boolean;
  cameraTrackId?: string | null;
  room?: components["schemas"]["Room"];
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
  updateCallTokens: (tokens: Partial<CallState>) => void;
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
        state.callTokens = tokens;
      }),
    updateCallTokens: (tokens) =>
      set((state) => {
        if (!state.callTokens) return;
        state.callTokens = { ...state.callTokens, ...tokens };
      }),
    getStoredToken: async () => {
      return await invoke<string | null>("get_stored_token");
    },
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
  if (!isEqual(state, prevState)) {
    emit("store-update", state);
  }
  // console.debug("Did not emit update, state is the same");
});

// Set up listener for store updates from other windows
listen("store-update", (event) => {
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
  emit("get-store-response", {
    state: useStore.getState(),
    window: windowName,
  });
});

export default useStore;
