import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useEffect } from "react";
import hotkeys from "hotkeys-js";
import useStore, { ParticipantRole } from "@/store/store";
import { useLocalParticipant, useRoomContext, useTracks } from "@livekit/components-react";
import { Track } from "livekit-client";
import { tauriUtils } from "@/windows/window-utils";

const appWindow = getCurrentWebviewWindow();

export const useResizeListener = (callback: () => void) => {
  useEffect(() => {
    // Run only once hook
    // Hacky way to initialise the callbacks with a Promise inside a hook
    const setupResizeListener = async () => {
      const unlisten = await appWindow.onResized(callback);
      return unlisten;
    };

    let unlisten: (() => void) | undefined;

    setupResizeListener().then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, [callback]);
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
 * Hook to listen for screen share events and update participant roles accordingly.
 * This hook handles the logic for detecting when the local participant or remote participants
 * start/stop screen sharing and updates the role in the store.
 *
 * Used as a hook to void holding this logic in the component.
 */
export const useScreenShareListener = () => {
  const { callTokens, setCallTokens, user } = useStore();
  const tracks = useTracks([Track.Source.ScreenShare]);
  const room = useRoomContext();

  useEffect(() => {
    if (!callTokens || !callTokens.videoToken) return;

    // To find if we actually share the screen, we cannot rely on
    // localParticipant, as we share from the backend (diff participant conceptually).
    // To find out if we share, we need to check the identity
    // Example ID:
    // room:92f1bdd8-5b98-45a7-ab08-0bd96e29f2d1:0195013f-20b5-719d-ac6b-f4beed3ba2ea:audio
    let localIsSharing = false;
    for (const participant of room.remoteParticipants) {
      for (const track of participant[1].getTrackPublications()) {
        if (track.source === Track.Source.ScreenShare && participant[1].identity.includes(user?.id || "")) {
          localIsSharing = true;
          break;
        }
      }
    }

    // Check if any remote participant is sharing
    let remoteIsSharing = false;
    for (const participant of room.remoteParticipants) {
      for (const track of participant[1].getTrackPublications()) {
        if (track.source === Track.Source.ScreenShare && !participant[1].identity.includes(user?.id || "")) {
          remoteIsSharing = true;
          break;
        }
      }
      if (remoteIsSharing) break;
    }

    if (localIsSharing && remoteIsSharing) {
      console.error("Both local and remote participants are sharing, edge case, or transient period");
      return;
    }

    let newRole: ParticipantRole;
    if (localIsSharing) {
      newRole = ParticipantRole.SHARER;
    } else if (remoteIsSharing) {
      newRole = ParticipantRole.CONTROLLER;
    } else {
      newRole = ParticipantRole.NONE;
    }

    if (callTokens.role !== newRole) {
      if (callTokens.role === ParticipantRole.CONTROLLER && !remoteIsSharing) {
        tauriUtils.closeScreenShareWindow();
        tauriUtils.setDockIconVisible(false);
      }

      if (newRole === ParticipantRole.CONTROLLER) {
        tauriUtils.createScreenShareWindow(callTokens.videoToken, false);
      }

      setCallTokens({
        ...callTokens,
        role: newRole,
      });
    }
  }, [tracks, room.remoteParticipants, callTokens, setCallTokens]);
};
