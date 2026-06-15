import { useEffect, useRef } from "react";
import { differenceInSeconds, fromUnixTime } from "date-fns";
import { Button } from "@/components/ui/button";
import { onOpenUrl } from "@tauri-apps/plugin-deep-link";
import useStore from "../../store/store";
import { tauriUtils } from "@/windows/window-utils.ts";
import { Sidebar } from "@/components/sidebar/Sidebar";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Debug } from "./tabs/Debug";
import { Login } from "./login";
import { Report } from "./report";
import { useAPI } from "@/services/query";
import { isTauri } from "@tauri-apps/api/core";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { HiOutlineExclamationCircle } from "react-icons/hi2";
import toast from "react-hot-toast";
import { CallBanner } from "@/components/ui/call-banner";
import { socketService } from "@/services/socket";
import { TRejectCallMessage, TIncomingCallMessage, TWebSocketMessage, TPresenceAckMessage } from "@/payloads";
import { Participants } from "@/components/ui/participants";
import { CallCenter } from "@/components/ui/call-center";
import { listen } from "@tauri-apps/api/event";
import Invite from "./invite";
import { sounds } from "@/constants/sounds";
import { useDisableNativeContextMenu } from "@/lib/hooks";
import { processDeepLinkUrl } from "@/lib/deepLinkUtils";
import { Rooms } from "./tabs/Rooms";
import { pollUpdates } from "@/lib/auto-update";

function App() {
  const {
    tab,
    authToken,
    callTokens,
    teammates,
    updateInProgress,
    setCallTokens,
    setNeedsUpdate,
    setUser,
    setTab,
    setTeammates,
    setAuthToken,
    setLivekitUrl,
    setIncomingCallCallerId,
    setCallsPresence,
  } = useStore();

  const coreProcessCrashedRef = useRef(false);
  useDisableNativeContextMenu();

  const { useQuery } = useAPI();

  const sentryMetadataRef = useRef<boolean>(false);

  const { error: userError } = useQuery("get", "/api/auth/user", undefined, {
    enabled: !!authToken,
    refetchInterval: 30_000,
    retry: true,
    queryHash: `user-${authToken}`,
    select: (data) => {
      setUser(data);
      if (!sentryMetadataRef.current) {
        tauriUtils.setSentryMetadata(data.id);
        sentryMetadataRef.current = true;
      }
      return data;
    },
  });

  // Get current user's teammates
  const { error: teammatesError, refetch: refetchTeammates } = useQuery("get", "/api/auth/teammates", undefined, {
    enabled: !!authToken,
    refetchInterval: 10_000,
    refetchIntervalInBackground: true,
    retry: true,
    queryHash: `teammates-${authToken}`,
    select: (data) => {
      setTeammates(data);
      return data;
    },
  });

  // Poll call presence every 10 seconds
  const { refetch: refetchCallsPresence } = useQuery("get", "/api/auth/calls/presence", undefined, {
    enabled: !!authToken,
    refetchInterval: 10_000,
    refetchIntervalInBackground: true,
    retry: true,
    queryHash: `calls-presence-${authToken}`,
    select: (data) => {
      setCallsPresence(data.presence);
      return data.presence;
    },
  });

  // Get LiveKit server URL and send to Tauri backend
  const { data: livekitUrlData } = useQuery("get", "/api/auth/livekit/server-url", undefined, {
    enabled: !!authToken,
    retry: true,
    queryHash: `livekit-url-${authToken}`,
  });

  // Send LiveKit URL to Tauri backend and store when fetched
  useEffect(() => {
    const sendLivekitUrlToBackend = async () => {
      if (livekitUrlData?.url) {
        console.log("livekitUrlData", livekitUrlData);
        try {
          await tauriUtils.setLivekitUrl(livekitUrlData.url);
          setLivekitUrl(livekitUrlData.url);
          console.debug("LiveKit URL sent to Tauri backend:", livekitUrlData.url);
        } catch (err) {
          console.error("Failed to send LiveKit URL to Tauri backend:", err);
        }
      }
    };

    sendLivekitUrlToBackend();
  }, [livekitUrlData, setLivekitUrl]);

  // Load stored token and custom server URL on app start
  useEffect(() => {
    (async () => {
      if (!isTauri()) return;
      // Load custom server URL first (before auth token, since auth uses the URL)
      const customUrl = await tauriUtils.loadCustomServerUrl();
      if (customUrl) {
        useStore.getState().setCustomServerUrl(customUrl);
      }
      // Then load auth token
      const token = await tauriUtils.getStoredToken();
      if (token) {
        setAuthToken(token);
      }
    })();
  }, []);

  // Deep link handling
  useEffect(() => {
    if (!isTauri()) return;
    // Focus to open the window is its closed
    const setupDeepLinkListener = async () => {
      const unlistenFn = await onOpenUrl(async (urls: string[]) => {
        console.log("Received deep link request:", urls);
        const url = urls[0];
        if (url) {
          // Use the centralized deep-link handler
          await processDeepLinkUrl(url);
        }
      });

      return unlistenFn;
    };

    let unlisten: (() => void) | undefined;
    setupDeepLinkListener().then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Check for updates (and auto-install when idle on macOS)
  useEffect(() => {
    if (!isTauri()) return;

    pollUpdates(setNeedsUpdate);
    const interval = setInterval(() => pollUpdates(setNeedsUpdate), 15 * 60 * 1000);

    return () => clearInterval(interval);
  }, [setNeedsUpdate]);

  // Auto-navigate to the call page on call join, and away from it on call end.
  const prevInCallRef = useRef(false);
  useEffect(() => {
    const inCall = !!callTokens;
    if (inCall && !prevInCallRef.current) {
      setTab("call");
    } else if (!inCall && prevInCallRef.current && tab === "call") {
      setTab("user-list");
    }
    prevInCallRef.current = inCall;
  }, [callTokens, tab, setTab]);

  const handleReject = (isInCall?: boolean) => {
    const { incomingCallCallerId } = useStore.getState();
    if (!incomingCallCallerId && !isInCall) return;
    if (!isInCall) {
      sounds.incomingCall.stop();
    }
    socketService.send({
      type: "call_reject",
      payload: {
        caller_id: incomingCallCallerId,
        reject_reason: "rejected",
      },
    } as TRejectCallMessage);
    setIncomingCallCallerId(null);
  };

  const handleInCallRejection = (callerID: string) => {
    socketService.send({
      type: "call_reject",
      payload: {
        caller_id: callerID,
        reject_reason: "in-call",
      },
    } as TRejectCallMessage);
  };

  // Generic socket event listeners
  // Need to remove from here and add them to a shared file
  // to be cleaner and easier to manage
  useEffect(() => {
    socketService.on("incoming_call", (data: TWebSocketMessage) => {
      if (data.type !== "incoming_call") {
        return;
      }

      const MAX_CALL_AGE_S = 60;
      const incomingMsg = data as TIncomingCallMessage;
      const initiatedAt = incomingMsg.payload.initiated_at;
      if (initiatedAt != null) {
        const ageSeconds = differenceInSeconds(new Date(), fromUnixTime(initiatedAt));
        if (ageSeconds > MAX_CALL_AGE_S) {
          console.warn(`Dropping stale incoming call (age: ${ageSeconds}s)`);
          return;
        }
      }

      // Check that there is no ongoing call
      // If there is, reject the call
      const { callTokens } = useStore.getState();
      if (callTokens) {
        const incomingCallerId = (data as TIncomingCallMessage).payload.caller_id;

        // Special case: If we're receiving a call from the same participant we're already in a call with,
        // close the current call and allow the new call to proceed
        // This is an edge case, if we do not get a termination signal but not
        // sure when its happening yet.
        if (callTokens.participant === incomingCallerId) {
          console.log("Received call from current participant - closing current call and accepting new one");
          setCallTokens(null);
          tauriUtils.endCallCleanup();
          // Continue to show the call banner below
        } else {
          // Different participant - reject as usual
          handleInCallRejection(incomingCallerId);
          return;
        }
      }

      if (data.type === "incoming_call") {
        setIncomingCallCallerId(data.payload.caller_id);

        /* Reject call if update in progress */
        if (updateInProgress) {
          handleReject();
          return;
        }

        // Open current tauri window
        // and create call banner
        toast((t) => <CallBanner callerId={data.payload.caller_id} toastId={t.id} />, {
          position: "bottom-center",
          id: "call-banner",
          duration: Infinity,
          className: "ml-12",
          removeDelay: 100,
          style: {
            padding: "2px",
          },
        });
        tauriUtils.showWindow("main");
      }
    });

    socketService.on("call_end", (data: TWebSocketMessage) => {
      if (data.type === "call_end") {
        // Get call info before clearing tokens
        toast.dismiss("call-banner");
        setIncomingCallCallerId(null);

        const { callTokens: currentCallTokens, user } = useStore.getState();
        const participantId = currentCallTokens?.participant || "";
        const roomId = currentCallTokens?.room?.id || data.payload.call_id || "";
        const teamId = user?.team_id?.toString() || "";

        setCallTokens(null);
        // Close screen share window
        tauriUtils.endCallCleanup();

        // Show feedback window if not disabled
        if (participantId && teamId) {
          tauriUtils.showFeedbackWindowIfEnabled(teamId, roomId, participantId);
        }
      }
    });

    socketService.on("teammate_online", (data: TWebSocketMessage) => {
      if (data.type === "teammate_online") {
        const { teammates } = useStore.getState();
        for (const teammate of teammates || []) {
          if (teammate.id === data.payload.teammate_id && !teammate.is_active) {
            refetchTeammates();
          }
        }
      }
    });

    socketService.on("presence_changed", (data: TWebSocketMessage) => {
      if (data.type === "presence_changed") {
        refetchCallsPresence();
      }
    });

    socketService.on("presence_check", (data: TWebSocketMessage) => {
      if (data.type !== "presence_check") {
        return;
      }
      const room = data.payload.room;
      const { callTokens } = useStore.getState();
      // in_call answers the validation ping. For named room calls we additionally
      // verify the room matches; ad-hoc 1:1 rooms aren't stored client-side, so
      // the boolean (callTokens !== null) covers them.
      let inCall = callTokens !== null;
      if (inCall && callTokens?.room?.id) {
        inCall = callTokens.room.id === room;
      }
      socketService.send({
        type: "presence_ack",
        payload: { room, in_call: inCall },
      } as TPresenceAckMessage);
    });
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    const setupCoreProcessCrashedListener = async () => {
      const unlistenFn = await listen("core_process_crashed", (data) => {
        if (coreProcessCrashedRef.current) return;

        console.debug("Core process crashed");
        coreProcessCrashedRef.current = true;

        tauriUtils.showWindow("main");
        toast.error(
          (t) => (
            <div className="flex flex-row items-center gap-2">
              <div className="text-sm">{`${data.payload as string}`}</div>
              <Button
                size="sm"
                onClick={() => {
                  toast.dismiss(t.id);
                  coreProcessCrashedRef.current = false;
                }}
              >
                Dismiss
              </Button>
            </div>
          ),
          {
            duration: 60_000,
            position: "top-center",
          },
        );
      });

      return unlistenFn;
    };

    // Update auth token when it changes in the backend
    const setupChangeTokenListener = async () => {
      const unlistenFn = await listen("token_changed", (event) => {
        const token = event.payload as string;
        if (token) {
          setAuthToken(token);
        } else {
          // Token was deleted (e.g., when changing server URL)
          // Clear auth-related state but preserve customServerUrl
          setAuthToken(null);
          setUser(null);
          setTeammates(null);
          setCallTokens(null);
        }
      });

      return unlistenFn;
    };

    // Update custom server URL when it changes in the backend
    const setupServerUrlChangedListener = async () => {
      const unlistenFn = await listen<string | null>("hopp_server_url_changed", (event) => {
        useStore.getState().setCustomServerUrl(event.payload);
      });

      return unlistenFn;
    };

    let unlisten: (() => void) | undefined;
    setupCoreProcessCrashedListener().then((fn) => {
      unlisten = fn;
    });

    let unlistenChangeToken: (() => void) | undefined;
    setupChangeTokenListener().then((fn) => {
      unlistenChangeToken = fn;
    });

    let unlistenServerUrl: (() => void) | undefined;
    setupServerUrlChangedListener().then((fn) => {
      unlistenServerUrl = fn;
    });

    return () => {
      if (unlisten) unlisten();
      if (unlistenChangeToken) unlistenChangeToken();
      if (unlistenServerUrl) unlistenServerUrl();
    };
  }, []);

  /*
   * This is a hack for keeping the frontend alive and
   * continue to receive web socket messages, if we don't do that
   * the frontend goes to sleep and stops receiving web sockets messages
   * which means that an incoming call might be missed.
   * Worst case scenario the ring won't be heard for the first 30 seconds.
   */
  useEffect(() => {
    if (!isTauri()) return;
    const setupCoreProcessCrashedListener = async () => {
      const unlistenFn = await listen("ping", () => { });

      return unlistenFn;
    };

    let unlisten: (() => void) | undefined;
    setupCoreProcessCrashedListener().then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  // Avoid showing login tab if user is already logged in
  if (authToken && tab === "login") {
    setTab("user-list");
  }

  return (
    <div className="container flex flex-row bg-white" id="app-body">
      {/* Action Sidebar */}
      <Sidebar />
      <ScrollArea type="scroll" className="h-100% overflow-y-scroll overflow-x-hidden w-[350px] relative h-full">
        {callTokens && (
          <div className={tab === "call" ? "" : "hidden"}>
            <CallCenter />
          </div>
        )}
        <div className="w-full h-auto mt-2">
          {userError && (
            <Alert variant="destructive" className="py-2 w-[90%] mx-auto">
              <HiOutlineExclamationCircle className="h-4 w-4" />
              <AlertTitle>Issue</AlertTitle>
              <AlertDescription>{userError?.message}</AlertDescription>
            </Alert>
          )}
          {teammatesError && (
            <Alert variant="destructive" className="py-2 w-[90%] mx-auto">
              <HiOutlineExclamationCircle className="h-4 w-4" />
              <AlertTitle>Issue</AlertTitle>
              <AlertDescription>{teammatesError?.message}</AlertDescription>
            </Alert>
          )}
        </div>
        {tab === "debug" && <Debug />}
        {tab === "invite" && <Invite />}
        {tab === "login" && <Login />}
        {tab === "rooms" && <Rooms />}
        {tab === "user-list" && (
          <>
            <div className="flex flex-col items-start gap-1.5 p-2">
              <Participants teammates={teammates || []} />
            </div>
          </>
        )}
        {tab === "report-issue" && <Report />}
      </ScrollArea>
    </div>
  );
}

export default App;
