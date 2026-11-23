import toast from "react-hot-toast";
import useStore, { ParticipantRole } from "@/store/store";
import { tauriUtils } from "@/windows/window-utils";
import { socketService } from "@/services/socket";
import { sounds } from "@/constants/sounds";
import type { components } from "@/openapi";
import type { TCallRequestMessage, TCallTokensMessage, TWebSocketMessage } from "@/payloads";

/** How long to wait for the callee to answer before giving up. */
const CALL_RESPONSE_TIMEOUT_MS = 65_000;
/** How long to wait for the LiveKit server URL to be applied to core. */
const LIVEKIT_URL_TIMEOUT_MS = 10_000;

/**
 * The one outgoing-call operation in flight. Each operation gets a unique id
 * so stale callbacks can verify they still own the call before touching
 * shared state.
 */
let activeCall: { id: number; userId: string } | null = null;
let callSeq = 0;

/**
 * Waits until the LiveKit server URL has been applied to core. The URL query
 * in `app.tsx` only runs after `authToken` is set, so a cold-start deep-link
 * can reach call-start before core has a `room_service`, which rejects
 * `CallStart` with `RoomServiceNotFound`. Returns immediately when the URL is
 * already applied.
 */
const waitForLivekitUrl = async (timeoutMs = LIVEKIT_URL_TIMEOUT_MS): Promise<boolean> => {
  if (useStore.getState().livekitUrl) return true;

  return await new Promise<boolean>((resolve) => {
    const timeoutId = setTimeout(() => {
      unsubscribe();
      resolve(false);
    }, timeoutMs);

    const unsubscribe = useStore.subscribe((state) => {
      if (state.livekitUrl) {
        clearTimeout(timeoutId);
        unsubscribe();
        resolve(true);
      }
    });
  });
};

/**
 * Applies fetched session tokens to the store and starts the call in core.
 * Rolls back the call state and notifies the backend when core fails to start.
 */
export const startCallWithTokens = async (
  tokens: components["schemas"]["SessionTokensResponse"],
  room?: components["schemas"]["Room"] | null,
): Promise<boolean> => {
  const { setCallTokens } = useStore.getState();
  const callRoom = room ?? tokens.room ?? undefined;

  sounds.callAccepted.play();
  const { startMic, startCamera } = await tauriUtils.getCallStartPreferences();
  setCallTokens({
    ...tokens,
    isRoomCall: !!callRoom,
    timeStarted: new Date(),
    hasAudioEnabled: startMic,
    hasCameraEnabled: startCamera,
    role: ParticipantRole.NONE,
    isRemoteControlEnabled: true,
    room: callRoom,
    participants: [],
    isInitialisingCall: true,
    micLevel: 0,
  });

  if (!(await waitForLivekitUrl())) {
    console.warn("LiveKit server URL was not applied to core in time");
  }

  try {
    await tauriUtils.callStarted(tokens.audioToken, tokens.videoToken);
  } catch {
    socketService.send({ type: "call_end", payload: { participant_id: tokens.participant } });
    await tauriUtils.endCallCleanup();
    setCallTokens(null);
    toast.error("Failed to start call");
    return false;
  }

  await tauriUtils.showWindow("main");
  return true;
};

/**
 * Rings a teammate and waits for the callee to answer. This is the single
 * owner of an outgoing call: it registers the socket handler and timeout for
 * the operation, and is the only place that applies the returned tokens via
 * `startCallWithTokens`. UI components display `calling` from the store and
 * cancel through `cancelOutgoingCall` instead of handling call responses
 * themselves.
 *
 * Resolves true when the call started, false when it was rejected, timed out,
 * or canceled.
 */
export const requestOutgoingCall = (userId: string, name: string): Promise<boolean> => {
  // Reserve the outgoing-call slot atomically: no awaits between the busy
  // check and setCalling, so a second request cannot slip in while the socket
  // was still connecting.
  if (useStore.getState().calling) {
    toast.error("Already calling, please wait");
    return Promise.resolve(false);
  }

  const op = { id: ++callSeq, userId };
  activeCall = op;
  const handlerId = `outgoing-call-${op.id}`;

  useStore.getState().setCalling(userId);
  sounds.ringing.play();
  toast.success(`Calling ${name}...`);

  return new Promise<boolean>((resolve) => {
    let settled = false;
    let timeoutId: ReturnType<typeof setTimeout> | undefined;
    let unsubscribeCalling: (() => void) | undefined;

    const teardown = () => {
      if (timeoutId) clearTimeout(timeoutId);
      unsubscribeCalling?.();
      socketService.removeHandler(handlerId);
      sounds.ringing.stop();
      if (activeCall === op) activeCall = null;
    };

    const finish = (result: boolean) => {
      teardown();
      // Release `calling` only while this operation still owns it, so a stale
      // settle cannot clear a newer request's reservation.
      if (useStore.getState().calling === userId) {
        useStore.getState().setCalling(null);
      }
      resolve(result);
    };

    const settle = (tokens: TCallTokensMessage["payload"] | null) => {
      if (settled) return;
      settled = true;

      // Late tokens for a canceled call must not start it.
      if (activeCall !== op || useStore.getState().calling !== userId) {
        finish(false);
        return;
      }

      if (!tokens) {
        finish(false);
        return;
      }

      // The wait is over: release `calling`, then start the call in core as
      // the operation's single owner.
      teardown();
      if (useStore.getState().calling === userId) {
        useStore.getState().setCalling(null);
      }
      void startCallWithTokens(tokens).then(resolve);
    };

    socketService.on(handlerId, (data: TWebSocketMessage) => {
      if (activeCall !== op) return;
      switch (data.type) {
        case "call_tokens":
          // For the caller-side message `participant` holds the callee id.
          if (data.payload.participant === userId) settle(data.payload);
          break;
        case "call_reject": {
          const reason = data.payload.reject_reason;
          if (reason === "in-call") {
            toast.error(`${name} is already in a call`, { duration: 2500 });
          } else if (reason === "trial-ended") {
            toast.error(`Trial has expired, contact us if you want to extend it`, { duration: 2500 });
          } else if (reason === "already-calling") {
            toast.error(`Already calling ${name}, please wait`, { duration: 2500 });
          } else {
            toast.error(`${name} rejected your call`, { duration: 2500 });
          }
          sounds.unavailable.play();
          settle(null);
          break;
        }
        case "callee_offline":
          if (data.payload.callee_id === userId) {
            toast.error(`${name} appears to be offline`, { duration: 2500 });
            sounds.unavailable.play();
            settle(null);
          }
          break;
        case "call_accept":
          toast.success(`${name} accepted your call`, { duration: 1500 });
          break;
      }
    });

    // Ends the operation when `calling` is released externally, e.g. by the
    // End action through `cancelOutgoingCall` or a state reset.
    unsubscribeCalling = useStore.subscribe((state) => {
      if (state.calling !== userId) settle(null);
    });

    socketService.send({
      type: "call_request",
      payload: { callee_id: userId },
    } as TCallRequestMessage);

    // 5 secs more from auto-reject from callee's timeout.
    timeoutId = setTimeout(() => {
      socketService.send({
        type: "call_end",
        payload: { participant_id: userId },
      });
      toast.error(`${name} didn't answer`, { duration: 1500 });
      settle(null);
    }, CALL_RESPONSE_TIMEOUT_MS);
  });
};

/**
 * Cancels the outgoing call to `userId`, e.g. from the End action in the
 * teammate list. Clearing `calling` settles the pending operation, which
 * removes its handler and timeout.
 */
export const cancelOutgoingCall = (userId: string) => {
  if (useStore.getState().calling !== userId) return;
  socketService.send({
    type: "call_end",
    payload: { participant_id: userId },
  });
  useStore.getState().setCalling(null);
};
