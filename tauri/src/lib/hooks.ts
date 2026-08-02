import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { once } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import hotkeys from "hotkeys-js";
import toast from "react-hot-toast";
import useStore, { ParticipantRole } from "@/store/store";
import { tauriUtils } from "@/windows/window-utils";
import { usePostHog } from "posthog-js/react";
import { socketService } from "@/services/socket";
import { sounds } from "@/constants/sounds";
import { useAPI, useFetchClient } from "@/services/query";

const appWindow = getCurrentWebviewWindow();
const CALL_END_TIMEOUT_MS = 5_000;

/**
 * Hook to detect and listen for system theme changes (light/dark mode).
 * Updates the document's root element with the 'dark' class based on system preference.
 * @returns The current theme ('light' or 'dark')
 */
export const useSystemTheme = () => {
  const [theme, setTheme] = useState<"light" | "dark">(() => {
    // Initialize with current system preference
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  });

  useEffect(() => {
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");

    // Update theme and document class
    const updateTheme = (e: MediaQueryListEvent | MediaQueryList) => {
      const isDark = e.matches;
      setTheme(isDark ? "dark" : "light");
      document.documentElement.classList.toggle("dark", isDark);
    };

    // Set initial state
    updateTheme(mediaQuery);

    // Listen for changes
    mediaQuery.addEventListener("change", updateTheme);

    return () => {
      mediaQuery.removeEventListener("change", updateTheme);
    };
  }, []);

  return theme;
};

/**
 * This is a hack to prevent the context menu from being shown
 * when the user right clicks on the screen.
 * @see: https://github.com/tauri-apps/tauri/discussions/3844#discussioncomment-8578187
 */
export const useDisableNativeContextMenu = () => {
  useEffect(() => {
    let isDevToolsEnabled = false;

    // Register the hotkey
    // For macOS, 'command+shift+d'
    // For Windows/Linux, 'ctrl+shift+d'
    hotkeys("cmd+shift+d, ctrl+shift+d", (event) => {
      event.preventDefault();
      isDevToolsEnabled = !isDevToolsEnabled;
    });

    document.addEventListener("contextmenu", (event) => {
      if (import.meta.env.MODE === "development") return;
      if (isDevToolsEnabled) return;
      event.preventDefault();
    });
  }, []);
};

/**
 * Hook to end a call and clean up all associated resources.
 * Simplified: no longer manages LiveKit room or browser-based tracks.
 */
export function useEndCall() {
  const { callTokens, setCallTokens, user } = useStore();
  const posthog = usePostHog();
  const fetchClient = useFetchClient();

  const endCall = useCallback(() => {
    if (!callTokens) return;

    const { timeStarted, participant, room } = callTokens;

    // Capture call info before clearing tokens for feedback
    const teamId = user?.team_id?.toString() || "";
    const roomId = room?.id || "";
    const participantId = user?.id || "";

    // Notify backend that user left the room (fire and forget)
    // This is used for Slack rooms to remove the user from the Slack call
    if (room?.id) {
      fetchClient
        .POST("/api/auth/room/{id}/leave", {
          params: {
            path: { id: room.id },
          },
        })
        .catch((e) => {
          console.error("leave room request failed:", e);
        })
        .finally(() => {
          console.log("leave room request successful fired and forgotten:", room.id);
        });
    }

    // Send websocket message to end call
    socketService.send({
      type: "call_end",
      payload: {
        participant_id: participant,
      },
    });

    // Play end call sound
    sounds.callAccepted.play();

    // Clear call tokens
    if (callTokens.role === ParticipantRole.SHARER) {
      tauriUtils.stopSharing();
    }
    tauriUtils.endCallCleanup();

    setCallTokens(null);

    // Show feedback window for the person ending the call
    if (participantId && teamId) {
      tauriUtils.showFeedbackWindowIfEnabled(teamId, roomId, participantId);
    }

    // Send posthog event on how much
    // time in seconds the call lasted.
    // Time is serialized as a string in store
    // so its not saved as a Date object
    console.log(`Duration of the call: ${(Date.now() - new Date(timeStarted).getTime()) / 1000}seconds`);
    posthog.capture("call_ended", {
      duration_in_seconds: Date.now() - new Date(timeStarted).getTime() / 1000,
      participant,
    });
  }, [callTokens, setCallTokens, user, posthog, fetchClient]);

  return endCall;
}

export async function endCallAndWait(endCall: () => void) {
  let resolveCallEnded = () => {};
  const callEnded = new Promise<void>((resolve) => {
    resolveCallEnded = () => resolve();
  });
  const unlisten = await once("core_call_ended", resolveCallEnded);
  let timeoutId: ReturnType<typeof setTimeout> | undefined;

  try {
    endCall();
    await Promise.race([
      callEnded,
      new Promise<never>((_, reject) => {
        timeoutId = setTimeout(() => reject(new Error("Timed out waiting for core call cleanup")), CALL_END_TIMEOUT_MS);
      }),
    ]);
  } finally {
    if (timeoutId) clearTimeout(timeoutId);
    unlisten();
  }
}

/**
 * Hook to join another user's ongoing call, whether from an invite or from
 * teammate presence. Handles the full shared join flow: tears down any current
 * call (waiting for core cleanup), requests session tokens, initialises call
 * state (including named-room handling) and starts the call in core.
 * Failures are reported to the user via toasts and roll back call state.
 *
 * @returns Function that joins `userId`'s call and resolves to true when the
 * call started, false when it failed.
 */
export function useJoinCall() {
  const setCallTokens = useStore((state) => state.setCallTokens);
  const endCall = useEndCall();
  const { useMutation } = useAPI();
  const { mutateAsync: joinCallRequest } = useMutation("post", "/api/auth/call/join/{userId}", undefined);

  return useCallback(
    async (userId: string): Promise<boolean> => {
      try {
        // If already in a call, use the shared teardown and wait for core.
        if (useStore.getState().callTokens) {
          await endCallAndWait(endCall);
        }

        const tokens = await joinCallRequest({ params: { path: { userId } } });
        if (!tokens) {
          toast.error("Error joining call");
          return false;
        }

        sounds.callAccepted.play();
        const { startMic, startCamera } = await tauriUtils.getCallStartPreferences();
        setCallTokens({
          ...tokens,
          timeStarted: new Date(),
          hasAudioEnabled: startMic,
          hasCameraEnabled: startCamera,
          role: ParticipantRole.NONE,
          isRemoteControlEnabled: true,
          isRoomCall: !!tokens.room,
          participants: [],
          isInitialisingCall: true,
          micLevel: 0,
        });

        try {
          await tauriUtils.callStarted(tokens.audioToken, tokens.videoToken);
        } catch {
          socketService.send({ type: "call_end", payload: { participant_id: tokens.participant } });
          tauriUtils.endCallCleanup();
          setCallTokens(null);
          toast.error("Failed to start call");
          return false;
        }

        tauriUtils.showWindow("main");
        return true;
      } catch (error) {
        const isTrialEnded = (error as { error?: string } | null)?.error === "trial-ended";
        toast.error(isTrialEnded ? "Trial has expired, contact us if you want to extend it" : "Error joining call");
        return false;
      }
    },
    [endCall, joinCallRequest, setCallTokens],
  );
}
