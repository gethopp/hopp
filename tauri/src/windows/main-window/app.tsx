import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { onOpenUrl } from "@tauri-apps/plugin-deep-link";
import { checkForUpdates, downloadAndRelaunch } from "@/update.ts";
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
import { TWebSocketMessage } from "@/payloads";
import { Participants } from "@/components/ui/participants";
import { CallCenter } from "@/components/ui/call-center";
import { listen } from "@tauri-apps/api/event";
import Invite from "./invite";
import { sounds } from "@/constants/sounds";
import { useDisableNativeContextMenu } from "@/lib/hooks";
import { validateAndSetAuthToken } from "@/lib/authUtils";
import { Rooms } from "./tabs/Rooms";
import { LiveKitRoom } from "@livekit/components-react";
import { ConditionalWrap } from "@/components/conditional-wrapper";

function App() {
  const {
    tab,
    authToken,
    callTokens,
    teammates,
    needsUpdate,
    updateInProgress,
    setCallTokens,
    setNeedsUpdate,
    setUpdateInProgress,
    setUser,
    setTab,
    setTeammates,
    setAuthToken,
  } = useStore();

  const coreProcessCrashedRef = useRef(false);
  useDisableNativeContextMenu();

  const { useQuery } = useAPI();

  const [incomingCallerId, setIncomingCallerId] = useState<string | null>(null);
  const [livekitUrl, setLivekitUrl] = useState<string>("");

  const { error: userError } = useQuery("get", "/api/auth/user", undefined, {
    enabled: !!authToken,
    refetchInterval: 30_000,
    retry: true,
    queryHash: `user-${authToken}`,
    select: (data) => {
      setUser(data);
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

  // Get LiveKit server URL and send to Tauri backend
  const { data: livekitUrlData } = useQuery("get", "/api/auth/livekit/server-url", undefined, {
    enabled: !!authToken,
    retry: true,
    queryHash: `livekit-url-${authToken}`,
  });

  // Send LiveKit URL to Tauri backend when it's fetched
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
  }, [livekitUrlData]);

  // Load stored token on app start
  useEffect(() => {
    (async () => {
      if (!isTauri()) return;
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
          try {
            const urlObj = new URL(url);
            if (urlObj.protocol === "hopp:" && urlObj.pathname === "/authenticate") {
              const params = new URLSearchParams(urlObj.search);
              const token = params.get("token");
              if (token) {
                if (token) {
                  await validateAndSetAuthToken(token);
                  await tauriUtils.showWindow("main");
                }
              }
            }
          } catch (err) {
            console.error("Failed to parse deep link URL:", err);
          }
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

  // Check for updates
  useEffect(() => {
    if (!isTauri()) return;

    const checkUpdates = async () => {
      try {
        const needUpdate = await checkForUpdates();
        setNeedsUpdate(needUpdate !== null);
      } catch (err) {
        console.error("Failed to check for updates:", err);
      }
    };

    checkUpdates();

    const interval = setInterval(checkUpdates, 60 * 60 * 1000);

    return () => clearInterval(interval);
  }, []);

  const handleReject = () => {
    if (!incomingCallerId) return;
    sounds.incomingCall.stop();
    socketService.send({
      type: "call_reject",
      payload: {
        caller_id: incomingCallerId,
      },
    });
  };

  // Generic socket event listeners
  // Need to remove from here and add them to a shared file
  // to be cleaner and easier to manage
  useEffect(() => {
    socketService.on("incoming_call", (data: TWebSocketMessage) => {
      if (data.type === "incoming_call") {
        setIncomingCallerId(data.payload.caller_id);

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
        setCallTokens(null);
        // Close screen share window
        tauriUtils.endCallCleanup();
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
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    const setupCoreProcessCrashedListener = async () => {
      const unlistenFn = await listen("core_process_crashed", (data) => {
        if (coreProcessCrashedRef.current) return;

        console.debug("Core process crashed");
        coreProcessCrashedRef.current = true;

        tauriUtils.showWindow("main");
        toast.error(`${data.payload as string}, please restart and submit a bug report`, {
          duration: 30_000,
          position: "top-center",
        });
      });

      return unlistenFn;
    };

    // Update auth token when it changes in the backend
    const setupChangeTokenListener = async () => {
      const unlistenFn = await listen("token_changed", (event) => {
        const token = event.payload as string;
        if (token) {
          setAuthToken(token);
        }
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

    return () => {
      if (unlisten) unlisten();
      if (unlistenChangeToken) unlistenChangeToken();
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
      const unlistenFn = await listen("ping", () => {});

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
    <div className="container flex flex-row bg-white">
      {/* Action Sidebar */}
      <Sidebar />
      <ConditionalWrap
        condition={!!callTokens}
        wrap={(children) => (
          <LiveKitRoom key={callTokens?.audioToken} token={callTokens?.audioToken} serverUrl={livekitUrl}>
            {children}
          </LiveKitRoom>
        )}
      >
        <ScrollArea type="scroll" className="h-100% overflow-y-scroll overflow-x-hidden w-[350px] relative h-full">
          {callTokens && (
            <div className="sticky top-0 z-10 bg-white">
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
                <Button
                  variant={
                    needsUpdate ?
                      updateInProgress ?
                        "loading"
                      : "default"
                    : "hidden"
                  }
                  isLoading={updateInProgress}
                  disabled={!!callTokens}
                  onClick={() => {
                    downloadAndRelaunch();
                    setUpdateInProgress(true);
                    handleReject();
                  }}
                >
                  Update and restart
                </Button>
              </div>
            </>
          )}
          {tab === "report-issue" && <Report />}
        </ScrollArea>
      </ConditionalWrap>
    </div>
  );
}

export default App;
