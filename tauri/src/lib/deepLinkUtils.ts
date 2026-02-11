import toast from "react-hot-toast";
import useStore, { ParticipantRole } from "@/store/store";
import { tauriUtils } from "@/windows/window-utils";
import { Constants } from "@/constants";
import { validateAndSetAuthToken } from "./authUtils";
import type { components } from "@/openapi";

/**
 * Processes a hopp:// deep-link URL and handles the appropriate action.
 *
 * Supported deep-link formats:
 * - hopp:///authenticate?token=xxx - Authenticate user with token
 * - hopp:///join-session?sessionId=xxx - Join a Slack pairing session
 *
 * @param url The hopp:// URL to process
 * @returns true if the URL was handled successfully, false otherwise
 */
export const processDeepLinkUrl = async (url: string): Promise<boolean> => {
  try {
    const urlObj = new URL(url);

    if (urlObj.protocol !== "hopp:") {
      console.warn("Not a hopp:// URL:", url);
      return false;
    }

    const pathname = urlObj.pathname;
    const params = new URLSearchParams(urlObj.search);

    // Handle /authenticate deep-link
    if (pathname === "/authenticate") {
      const token = params.get("token");
      if (token) {
        await validateAndSetAuthToken(token);
        await tauriUtils.showWindow("main");
        return true;
      }
      console.warn("Missing token in authenticate deep-link");
      return false;
    }

    // Handle /join-session deep-link
    if (pathname === "/join-session") {
      const sessionId = params.get("sessionId");
      if (sessionId) {
        return await handleJoinSessionDeepLink(sessionId);
      }
      console.warn("Missing sessionId in join-session deep-link");
      return false;
    }

    console.warn("Unknown deep-link path:", pathname);
    return false;
  } catch (err) {
    console.error("Failed to parse deep link URL:", err);
    toast.error("Failed to process deep link");
    return false;
  }
};

/**
 * Handles joining a Slack pairing session by fetching tokens and setting up the call.
 *
 * @param sessionId The session/room ID to join
 * @returns true if successfully joined, false otherwise
 */
export const handleJoinSessionDeepLink = async (sessionId: string): Promise<boolean> => {
  const { authToken, setCallTokens, setTab, user } = useStore.getState();

  if (!authToken) {
    toast.error("Please log in first to join the session");
    setTab("login");
    await tauriUtils.showWindow("main");
    return false;
  }

  try {
    toast.loading("Joining session", { id: "join-session" });

    // Fetch tokens for the session
    const response = await fetch(`${Constants.backendUrl}/api/auth/slack/session/${sessionId}/tokens`, {
      headers: {
        Authorization: `Bearer ${authToken}`,
      },
    });

    if (!response.ok) {
      toast.dismiss("join-session");
      if (response.status === 404) {
        toast.error("Session has ended or doesn't exist");
      } else if (response.status === 401) {
        toast.error("Please log in again to join the session");
        setTab("login");
      } else if (response.status === 402) {
        const body = await response.json().catch(() => null);
        if (body?.error === "trial-ended") {
          toast.error("Trial has expired, contact us if you want to extend it");
        } else {
          toast.error("Payment required to access this session");
        }
      } else if (response.status === 403) {
        toast.error("You don't have access to this session. It belongs to a different team.");
      } else {
        toast.error("Failed to join session");
      }
      console.error("Failed to fetch session tokens:", response.status);
      return false;
    }

    const data: components["schemas"]["SessionTokensResponse"] = await response.json();

    // Set up the call with the tokens
    setCallTokens({
      ...data,
      timeStarted: new Date(),
      hasAudioEnabled: true,
      hasCameraEnabled: false,
      role: ParticipantRole.NONE,
      isRemoteControlEnabled: true,
      isRoomCall: true,
      room: {
        id: sessionId,
        // TODO(@konsalex): Get the room name from the backend
        // Same issue with `SlackJoin.tsx` component
        name: "Slack Session",
        user_id: user?.id || "",
      },
    });

    // Switch to the rooms tab and show the window
    await tauriUtils.showWindow("main");
    setTab("rooms");

    toast.dismiss("join-session");
    return true;
  } catch (err) {
    console.error("Error joining session:", err);
    toast.dismiss("join-session");
    toast.error("Failed to join room");
    return false;
  }
};
