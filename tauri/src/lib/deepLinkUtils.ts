import toast from "react-hot-toast";
import useStore, { ParticipantRole } from "@/store/store";
import { tauriUtils } from "@/windows/window-utils";
import { Constants } from "@/constants";
import { validateAndSetAuthToken } from "./authUtils";
import { endCallAndWait } from "./hooks";
import { requestOutgoingCall, startCallWithTokens } from "./outgoingCall";
import { socketService } from "@/services/socket";
import { sounds } from "@/constants/sounds";
import type { components } from "@/openapi";

/** How long to wait for the websocket to connect when the app was just launched. */
const SOCKET_CONNECT_TIMEOUT_MS = 10_000;

/**
 * Processes a hopp:// deep-link URL and handles the appropriate action.
 *
 * Supported deep-link formats:
 * - hopp:///authenticate?token=xxx - Authenticate user with token
 * - hopp:///join-session?sessionId=xxx - Join a Slack pairing session
 * - hopp:///call/{userId} - Ring a teammate to start a call
 * - hopp:///join-call/{userId} - Join a teammate's ongoing call
 * - hopp:///room/{roomId} - Join a named room
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
    const segments = pathname.split("/").filter(Boolean);
    const action = segments[0];

    // Handle /authenticate deep-link
    if (action === "authenticate") {
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
    if (action === "join-session") {
      const sessionId = params.get("sessionId");
      if (sessionId) {
        return await handleJoinSessionDeepLink(sessionId);
      }
      console.warn("Missing sessionId in join-session deep-link");
      return false;
    }

    // Handle /call/{userId} deep-link - ring a teammate to start a call
    if (action === "call") {
      const userId = segments[1] ?? params.get("userId");
      if (userId) {
        return await handleCallTeammateDeepLink(userId);
      }
      console.warn("Missing userId in call deep-link");
      return false;
    }

    // Handle /join-call/{userId} deep-link - join a teammate's ongoing call
    if (action === "join-call") {
      const userId = segments[1] ?? params.get("userId");
      if (userId) {
        return await handleJoinCallDeepLink(userId);
      }
      console.warn("Missing userId in join-call deep-link");
      return false;
    }

    // Handle /room/{roomId} deep-link - join a named room
    if (action === "room") {
      const roomId = segments[1] ?? params.get("roomId");
      if (roomId) {
        return await handleJoinRoomDeepLink(roomId);
      }
      console.warn("Missing roomId in room deep-link");
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
 * Ensures the user is authenticated before handling a call-related deep-link.
 * Deep-links can launch the app, racing the startup token load in `app.tsx`,
 * so the stored token and custom server URL are loaded from the Tauri backend
 * first. Shows a toast and navigates to the login tab when not authenticated.
 */
const ensureAuthenticated = async (action: string): Promise<boolean> => {
  const { authToken, customServerUrl, setAuthToken, setCustomServerUrl, setTab } = useStore.getState();
  if (authToken) return true;

  const customUrl = await tauriUtils.loadCustomServerUrl();
  if (customUrl && !customServerUrl) {
    setCustomServerUrl(customUrl);
  }

  const storedToken = await tauriUtils.getStoredToken();
  if (storedToken) {
    setAuthToken(storedToken);
    return true;
  }

  toast.error(`Please log in first to ${action}`);
  setTab("login");
  await tauriUtils.showWindow("main");
  return false;
};

/**
 * Waits for the backend websocket to connect. Deep-links can launch the app,
 * in which case the socket may still be connecting when the link is handled.
 */
const waitForSocketConnection = async (timeoutMs = SOCKET_CONNECT_TIMEOUT_MS): Promise<boolean> => {
  if (useStore.getState().socketConnected) return true;

  return await new Promise<boolean>((resolve) => {
    const timeoutId = setTimeout(() => {
      unsubscribe();
      resolve(false);
    }, timeoutMs);

    const unsubscribe = useStore.subscribe((state) => {
      if (state.socketConnected) {
        clearTimeout(timeoutId);
        unsubscribe();
        resolve(true);
      }
    });
  });
};

/**
 * Tears down the active call. Mirrors the cleanup performed by `useEndCall`,
 * minus the React-only bits (analytics hooks and the feedback window).
 */
const endCurrentCall = () => {
  const { callTokens, authToken, setCallTokens } = useStore.getState();
  if (!callTokens) return;

  // Notify backend that user left the room (fire and forget).
  // This is used for Slack rooms to remove the user from the Slack call.
  if (callTokens.room?.id && authToken) {
    fetch(`${Constants.backendUrl}/api/auth/room/${callTokens.room.id}/leave`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${authToken}`,
      },
    }).catch((e) => {
      console.error("leave room request failed:", e);
    });
  }

  socketService.send({
    type: "call_end",
    payload: {
      participant_id: callTokens.participant,
    },
  });

  if (callTokens.role === ParticipantRole.SHARER) {
    void tauriUtils.stopSharing();
  }
  void tauriUtils.endCallCleanup();
  setCallTokens(null);
};

/**
 * Handles hopp:///call/{userId} - rings a teammate to start a call, the same
 * way the "Call" button in the teammate list does.
 */
export const handleCallTeammateDeepLink = async (userId: string): Promise<boolean> => {
  if (!(await ensureAuthenticated("start a call"))) return false;

  const { callTokens, incomingCallCallerId, calling, teammates } = useStore.getState();
  const teammate = teammates?.find((t) => t.id === userId);
  const name = teammate ? `${teammate.first_name} ${teammate.last_name}`.trim() : "your teammate";

  if (callTokens) {
    toast.error("End your current call to start a new one");
    await tauriUtils.showWindow("main");
    return false;
  }
  if (incomingCallCallerId) {
    toast.error("Answer or reject the incoming call first");
    await tauriUtils.showWindow("main");
    return false;
  }
  if (calling) {
    toast.error("Already calling, please wait");
    return false;
  }
  if (teammate && !teammate.is_active) {
    sounds.unavailable.play();
    toast.error(`${name} is currently offline`);
    return false;
  }

  // The deep-link may have just launched the app, so the websocket may not be up yet.
  if (!(await waitForSocketConnection())) {
    toast.error("Not connected to the server yet, try again in a moment");
    return false;
  }

  // The shared outgoing-call controller owns the request: it rechecks
  // `calling` after the socket wait and is the only place that applies the
  // returned tokens via `startCallWithTokens`.
  return await requestOutgoingCall(userId, name);
};

/**
 * Handles hopp:///join-call/{userId} - joins the ongoing call of a teammate,
 * the same way the "Join" button in the teammate list does.
 */
export const handleJoinCallDeepLink = async (userId: string): Promise<boolean> => {
  if (!(await ensureAuthenticated("join a call"))) return false;

  const { authToken, callTokens, setTab } = useStore.getState();

  try {
    toast.loading("Joining call", { id: "join-call" });

    // Mirror the app's join flow: leave the current call before joining another one.
    if (callTokens) {
      await endCallAndWait(endCurrentCall);
    }

    const response = await fetch(`${Constants.backendUrl}/api/auth/call/join/${userId}`, {
      method: "POST",
      headers: {
        Authorization: `Bearer ${authToken}`,
      },
    });

    toast.dismiss("join-call");
    if (!response.ok) {
      if (response.status === 404) {
        toast.error("That teammate is not in a call");
      } else if (response.status === 401) {
        toast.error("Please log in again to join the call");
        setTab("login");
      } else if (response.status === 402) {
        const body = await response.json().catch(() => null);
        toast.error(
          body?.error === "trial-ended" ?
            "Trial has expired, contact us if you want to extend it"
          : "Payment required to join the call",
        );
      } else if (response.status === 403) {
        toast.error("You can only join calls with teammates");
      } else if (response.status === 409) {
        toast.error("The call ended while joining");
      } else {
        toast.error("Failed to join call");
      }
      console.error("Failed to join call:", response.status);
      return false;
    }

    const tokens: components["schemas"]["SessionTokensResponse"] = await response.json();
    return await startCallWithTokens(tokens);
  } catch (err) {
    console.error("Error joining call:", err);
    toast.dismiss("join-call");
    toast.error("Failed to join call");
    return false;
  }
};

/**
 * Looks up a room by ID in the user's room list. The room-tokens endpoint
 * returns tokens only, so the room name is resolved separately for the call UI.
 */
const fetchRoomById = async (
  roomId: string,
  authToken: string | null,
): Promise<components["schemas"]["Room"] | null> => {
  try {
    const response = await fetch(`${Constants.backendUrl}/api/auth/rooms`, {
      headers: {
        Authorization: `Bearer ${authToken}`,
      },
    });
    if (!response.ok) return null;
    const rooms: components["schemas"]["Room"][] = await response.json();
    return rooms.find((r) => r.id === roomId) ?? null;
  } catch {
    return null;
  }
};

/**
 * Handles hopp:///room/{roomId} - joins a named room, the same way the
 * "Join" button in the rooms list does.
 */
export const handleJoinRoomDeepLink = async (roomId: string): Promise<boolean> => {
  if (!(await ensureAuthenticated("join a room"))) return false;

  const { authToken, callTokens, setTab, user } = useStore.getState();

  // Already in this room - just surface the call UI.
  if (callTokens?.room?.id === roomId) {
    setTab("call");
    await tauriUtils.showWindow("main");
    return true;
  }

  try {
    toast.loading("Joining room", { id: "join-room" });

    if (callTokens) {
      await endCallAndWait(endCurrentCall);
    }

    const response = await fetch(`${Constants.backendUrl}/api/auth/room/${roomId}`, {
      headers: {
        Authorization: `Bearer ${authToken}`,
      },
    });

    if (!response.ok) {
      toast.dismiss("join-room");
      if (response.status === 404) {
        toast.error("Room not found");
      } else if (response.status === 401) {
        toast.error("Please log in again to join the room");
        setTab("login");
      } else if (response.status === 402) {
        const body = await response.json().catch(() => null);
        toast.error(
          body?.error === "trial-ended" ?
            "Trial has expired, contact us if you want to extend it"
          : "Payment required to join the room",
        );
      } else if (response.status === 403) {
        toast.error("You don't have access to this room");
      } else {
        toast.error("Failed to join room");
      }
      console.error("Failed to fetch room tokens:", response.status);
      return false;
    }

    const tokens: components["schemas"]["SessionTokensResponse"] = await response.json();

    // The tokens endpoint doesn't include the room details; look them up for
    // the call UI, falling back to a stub with the room ID when unavailable.
    const room = tokens.room ??
      (await fetchRoomById(roomId, authToken)) ?? {
        id: roomId,
        name: "Room",
        user_id: user?.id || "",
      };

    toast.dismiss("join-room");
    return await startCallWithTokens(tokens, room);
  } catch (err) {
    console.error("Error joining room:", err);
    toast.dismiss("join-room");
    toast.error("Failed to join room");
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
    const { startMic, startCamera } = await tauriUtils.getCallStartPreferences();
    setCallTokens({
      ...data,
      timeStarted: new Date(),
      hasAudioEnabled: startMic,
      hasCameraEnabled: startCamera,
      role: ParticipantRole.NONE,
      isRemoteControlEnabled: true,
      isRoomCall: true,
      participants: [],
      room: {
        id: sessionId,
        // TODO(@konsalex): Get the room name from the backend
        // Same issue with `SlackJoin.tsx` component
        name: "Slack Session",
        user_id: user?.id || "",
      },
      micLevel: 0,
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
