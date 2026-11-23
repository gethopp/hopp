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
 * A manual reject cannot realistically arrive this fast: the request has to
 * reach the callee and a human has to see the banner and dismiss it. Rejects
 * without a server-side reason arriving inside this window are in-flight
 * leftovers from a previously canceled attempt — the payload only carries the
 * caller id, so they cannot be matched to a callee directly.
 */
const STALE_REJECT_WINDOW_MS = 1_500;

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

  // Issuing these tokens added us to the call's room server-side (accept and
  // join endpoints do this before returning them). Refusals below owe the
  // backend a `call_end` so a refused start cannot leave stale room
  // membership or a peer waiting alone in the room — but only while no other
  // call owns the slot locally, since the backend removes us from our
  // *current* room and that could already be the replacement call's.
  const releasePeer = () => {
    const { callTokens } = useStore.getState();
    if (callTokens && callTokens.audioToken !== tokens.audioToken) return;
    socketService.send({ type: "call_end", payload: { participant_id: tokens.participant } });
  };

  sounds.callAccepted.play();

  let startMic: boolean, startCamera: boolean;
  try {
    ({ startMic, startCamera } = await tauriUtils.getCallStartPreferences());
  } catch (e) {
    console.error("Failed to read call start preferences:", e);
    releasePeer();
    toast.error("Failed to start call");
    return false;
  }

  // Another call may have claimed the slot while preferences were loading —
  // only the first claimant applies tokens (no await between check and set).
  if (useStore.getState().callTokens !== null) {
    releasePeer();
    return false;
  }

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

  // The peer may have ended the call while core was getting ready — only
  // start core if this attempt still owns the call state.
  if (useStore.getState().callTokens?.audioToken !== tokens.audioToken) {
    releasePeer();
    return false;
  }

  try {
    await tauriUtils.callStarted(tokens.audioToken, tokens.videoToken);
  } catch (e) {
    console.error("CallStart failed:", e);
    releasePeer();
    const current = useStore.getState().callTokens;
    // Only touch the store while this attempt still owns the call — a
    // replacement call must not be torn down by our rollback.
    if (current?.audioToken === tokens.audioToken) {
      setCallTokens(null);
    }
    // Undo the native half-start only while no other call owns the slot —
    // endCallCleanup would otherwise tear down the replacement call.
    if (!current || current.audioToken === tokens.audioToken) {
      try {
        await tauriUtils.endCallCleanup();
      } catch (cleanupError) {
        console.error("Call cleanup failed after CallStart error:", cleanupError);
      }
    }
    toast.error("Failed to start call");
    return false;
  }

  // The call may have ended while core was starting up.
  const current = useStore.getState().callTokens;
  if (current?.audioToken !== tokens.audioToken) {
    releasePeer();
    // Only clean up the zombie native call when the slot is empty — a
    // replacement call owns core now.
    if (current === null) {
      try {
        await tauriUtils.endCallCleanup();
      } catch (e) {
        console.error("Call cleanup failed after call ended during start:", e);
      }
    }
    return false;
  }

  try {
    await tauriUtils.showWindow("main");
  } catch (e) {
    // Window presentation is best-effort; the call itself is already live.
    console.warn("Failed to show main window after call start:", e);
  }
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
  // was still connecting or a call started over HTTP in the meantime.
  const { calling, callTokens, incomingCallCallerId } = useStore.getState();
  if (callTokens) {
    toast.error("End your current call to start a new one");
    return Promise.resolve(false);
  }
  if (incomingCallCallerId) {
    toast.error("Answer or reject the incoming call first");
    return Promise.resolve(false);
  }
  if (calling) {
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
    // Tokens are only honored after a fresh `call_accept` for this attempt —
    // the backend always publishes the accept before minting tokens, so
    // leftover tokens from a canceled attempt are dropped.
    let sawAccept = false;
    const requestSentAt = Date.now();
    let timeoutId: ReturnType<typeof setTimeout> | undefined;
    let unsubscribeStore: (() => void) | undefined;

    const teardown = () => {
      if (timeoutId) clearTimeout(timeoutId);
      unsubscribeStore?.();
      socketService.removeHandler(handlerId);
      sounds.ringing.stop();
    };

    const release = () => {
      // No-op once a newer operation owns the slot — a stale release must not
      // clear a replacement request's `calling` reservation.
      if (activeCall !== op) return;
      activeCall = null;
      if (useStore.getState().calling === userId) {
        useStore.getState().setCalling(null);
      }
    };

    const finish = (result: boolean) => {
      teardown();
      release();
      resolve(result);
    };

    const settle = (tokens: TCallTokensMessage["payload"] | null) => {
      if (settled) return;
      settled = true;

      // Late tokens for a canceled call, or tokens arriving after another
      // call already owns the slot, must not start it.
      if (activeCall !== op || useStore.getState().calling !== userId || useStore.getState().callTokens !== null) {
        finish(false);
        return;
      }

      if (!tokens) {
        finish(false);
        return;
      }

      // The wait is over: stop listening, but keep the `calling` reservation
      // until startup finishes so a second request cannot interleave during
      // the native awaits.
      teardown();
      void (async () => {
        let ok = false;
        try {
          ok = await startCallWithTokens(tokens);
        } catch (e) {
          console.error("Outgoing call startup failed:", e);
        }
        // Released mid-start means the op was canceled externally (e.g. the
        // End action) — the peer already got `call_end`, so unwind the call
        // that just materialized locally.
        const released = useStore.getState().calling !== userId;
        if (ok && released && useStore.getState().callTokens?.audioToken === tokens.audioToken) {
          useStore.getState().setCallTokens(null);
          try {
            await tauriUtils.endCallCleanup();
          } catch (e) {
            console.error("Call cleanup failed after cancel during start:", e);
          }
        }
        release();
        resolve(ok && !released);
      })();
    };

    socketService.on(handlerId, (data: TWebSocketMessage) => {
      if (activeCall !== op) return;
      switch (data.type) {
        case "call_tokens":
          // For the caller-side message `participant` holds the callee id.
          // Tokens without a preceding accept belong to a canceled attempt.
          if (data.payload.participant === userId && sawAccept) settle(data.payload);
          break;
        case "call_reject": {
          const reason = data.payload.reject_reason;
          const isManual = !reason || reason === "rejected";
          if (isManual && Date.now() - requestSentAt < STALE_REJECT_WINDOW_MS) break;
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
          sawAccept = true;
          toast.success(`${name} accepted your call`, { duration: 1500 });
          break;
        case "error":
          // The server could not set up the call (e.g. failed to create the
          // pending record or mint tokens) — release the slot now instead of
          // waiting out the full timeout.
          toast.error(data.payload.error, { duration: 2500 });
          sounds.unavailable.play();
          settle(null);
          break;
      }
    });

    // Ends the operation when `calling` is released externally, e.g. by the
    // End action through `cancelOutgoingCall` or a state reset. Also ends it
    // when another call takes ownership (`callTokens` set by an answered
    // incoming call or a room join) — we settle locally only, because a
    // `call_end` sent now would pull us out of that active call.
    unsubscribeStore = useStore.subscribe((state) => {
      if (state.calling !== userId || state.callTokens !== null) settle(null);
    });

    socketService.send({
      type: "call_request",
      payload: { callee_id: userId },
    } as TCallRequestMessage);

    // 5 secs more from auto-reject from callee's timeout.
    timeoutId = setTimeout(() => {
      // A `call_end` removes us from our *active* call server-side, so it is
      // only safe while no call has materialized.
      if (!useStore.getState().callTokens) {
        socketService.send({
          type: "call_end",
          payload: { participant_id: userId },
        });
      }
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
  // `call_end` pulls us out of our active call server-side, so skip it when a
  // different call already owns the slot — that ring simply dies locally. A
  // call to this same userId that is mid-startup does get notified.
  const { callTokens } = useStore.getState();
  if (!callTokens || callTokens.participant === userId) {
    socketService.send({
      type: "call_end",
      payload: { participant_id: userId },
    });
  }
  useStore.getState().setCalling(null);
};
