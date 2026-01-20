import { useParams, useNavigate, Link } from "react-router-dom";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { useHoppStore } from "@/store/store";
import { BACKEND_URLS } from "@/constants";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { AlertTriangle } from "lucide-react";

type SessionStatus = "loading" | "valid" | "ended" | "not_found" | "forbidden" | "error";

/**
 * TODO(@konsalex):.
 * We should hit the rooms API list to get this, with a filtering by session ID.
 * Currently the "api/auth/room/:ID" endpoint returns the non-temp rooms and the room GET gets tokens for the room back.
 *
 * Flow:
 * 1. User is redirected here from clicking "Join" in Slack
 * 2. If not authenticated, they'll be redirected to login (via router guard)
 * 3. We validate the session exists before attempting deep-link
 * 4. We attempt to open the Hopp desktop app via deep-link with just the session ID
 * 5. The desktop app fetches its own tokens using the session ID
 * 6. If the app doesn't open, user can stay in browser or retry
 */
export function SlackJoin() {
  const { sessionId } = useParams<{ sessionId: string }>();
  const navigate = useNavigate();
  const authToken = useHoppStore((state) => state.authToken);

  const [sessionStatus, setSessionStatus] = useState<SessionStatus>("loading");
  const [deepLinkAttempted, setDeepLinkAttempted] = useState(false);

  // Build the deep-link URL - just pass the session ID, app will fetch tokens
  const buildDeepLinkUrl = (id: string) => {
    const deepLinkParams = new URLSearchParams({ sessionId: id });
    return `hopp:///join-session?${deepLinkParams.toString()}`;
  };

  // First, validate the session exists
  useEffect(() => {
    if (!sessionId || !authToken) return;

    const checkSession = async () => {
      try {
        // Try to get tokens for this session - this validates it exists
        const response = await fetch(`${BACKEND_URLS.BASE}/api/auth/room/${sessionId}`, {
          headers: {
            Authorization: `Bearer ${authToken}`,
          },
        });

        if (response.ok) {
          setSessionStatus("valid");
        } else if (response.status === 404) {
          // Session not found - it has ended or never existed
          setSessionStatus("ended");
        } else if (response.status === 403) {
          // User is not authorized to join this session (wrong team)
          setSessionStatus("forbidden");
        } else {
          setSessionStatus("error");
        }
      } catch {
        setSessionStatus("error");
      }
    };

    checkSession();
  }, [sessionId, authToken]);

  // Attempt to open the desktop app via deep-link (only if session is valid)
  useEffect(() => {
    if (sessionStatus !== "valid" || sessionId == undefined || deepLinkAttempted) return;

    const deepLinkUrl = buildDeepLinkUrl(sessionId);
    console.info("Attempting deep-link:", deepLinkUrl);

    // Open the app using window.open (same approach as authentication flow)
    window.open(deepLinkUrl, "_blank");
    setDeepLinkAttempted(true);
  }, [sessionId, sessionStatus, deepLinkAttempted]);

  // Handle retry - open deep-link again
  const handleRetryDeepLink = () => {
    if (!sessionId) return;
    const deepLinkUrl = buildDeepLinkUrl(sessionId);
    window.open(deepLinkUrl, "_blank");
  };

  // Handle joining in browser instead
  const handleJoinInBrowser = () => {
    navigate(`/room/${sessionId}`);
  };

  if (!sessionId) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-center">
          <h1 className="text-2xl font-semibold mb-2">Invalid Session</h1>
          <p className="text-gray-600 mb-4">No session ID was provided.</p>
          <Button onClick={() => navigate("/dashboard")}>Go to Dashboard</Button>
        </div>
      </div>
    );
  }

  if (!authToken) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-center">
          <h1 className="text-2xl font-semibold mb-2">Authentication Required</h1>
          <p className="text-gray-600 mb-4">Please log in to join this pairing session.</p>
          <Button onClick={() => navigate(`/login?redirect=/slack/join/${sessionId}`)}>Log In</Button>
        </div>
      </div>
    );
  }

  // Session has ended or doesn't exist
  if (sessionStatus === "ended") {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-center max-w-md">
          <div className="text-4xl mb-4">⏱️</div>
          <h1 className="text-2xl font-semibold mb-2">Session Has Ended</h1>
          <p className="text-gray-600 mb-4">
            This pairing session is no longer active. The session may have ended or been closed by the host.
          </p>
          <Alert className="mb-4 text-left">
            <AlertTriangle className="h-4 w-4" />
            <AlertTitle>What happened?</AlertTitle>
            <AlertDescription>
              Hopp sessions automatically end when all participants leave. Ask the host to start a new session with{" "}
              <code className="bg-muted px-1 rounded">/hopp</code> in Slack.
            </AlertDescription>
          </Alert>
          <Button onClick={() => navigate("/dashboard")}>Go to Dashboard</Button>
        </div>
      </div>
    );
  }

  // User is not authorized to join this session (wrong team)
  if (sessionStatus === "forbidden") {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-center max-w-md">
          <div className="text-4xl mb-4">🔒</div>
          <h1 className="text-2xl font-semibold mb-2">Access Denied</h1>
          <p className="text-gray-600 mb-4">You don't have access to this pairing session.</p>
          <Alert className="mb-4 text-left">
            <AlertTriangle className="h-4 w-4" />
            <AlertTitle>Why can't I join?</AlertTitle>
            <AlertDescription>
              This session was created by a different team. You can only join sessions started by members of your own
              team.
            </AlertDescription>
          </Alert>
          <Button onClick={() => navigate("/dashboard")}>Go to Dashboard</Button>
        </div>
      </div>
    );
  }

  // Error checking session
  if (sessionStatus === "error") {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-center max-w-md">
          <div className="text-4xl mb-4">❌</div>
          <h1 className="text-2xl font-semibold mb-2">Unable to Join Session</h1>
          <p className="text-gray-600 mb-4">
            We couldn't connect to this session. Please try again or report a bug to{" "}
            <a className="link" href="https://github.com/gethopp/hopp/issues" target="_blank" rel="noreferrer">
              Hopp's GitHub repository
            </a>
            .
          </p>
          <div className="flex gap-2 justify-center">
            <Button onClick={() => window.location.reload()}>Try Again</Button>
            <Button variant="outline" onClick={() => navigate("/dashboard")}>
              Go to Dashboard
            </Button>
          </div>
        </div>
      </div>
    );
  }

  // Still checking session status
  if (sessionStatus === "loading") {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-center">
          <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-gray-900 mx-auto mb-4" />
          <h1 className="text-2xl font-semibold mb-2">Checking Session...</h1>
          <p className="text-gray-600">Verifying the session is still active...</p>
        </div>
      </div>
    );
  }

  if (deepLinkAttempted) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-center max-w-md">
          <div className="text-4xl mb-4">🏀</div>
          <h1 className="text-2xl font-semibold mb-2">Open in Hopp App</h1>
          <p className="text-gray-600 mb-4">
            We tried to open the Hopp desktop app. If it didn't open, you can try again or join directly in your
            browser.
          </p>
          <div className="flex gap-2 justify-center flex-wrap">
            <Button onClick={handleRetryDeepLink}>Open in Hopp App</Button>
            <Button variant="outline" onClick={handleJoinInBrowser}>
              Join in Browser
            </Button>
          </div>
          <p className="text-xs text-gray-400 mt-4">
            Don't have the app?{" "}
            <Link to="/dashboard" className="underline hover:text-gray-600">
              Download Hopp
            </Link>
          </p>
        </div>
      </div>
    );
  }

  // Initial loading state before deep-link attempt
  return (
    <div className="flex items-center justify-center h-full">
      <div className="text-center">
        <div className="animate-spin rounded-full h-8 w-8 border-b-2 border-gray-900 mx-auto mb-4" />
        <h1 className="text-2xl font-semibold mb-2">Preparing Session...</h1>
        <p className="text-gray-600">Getting ready to open Hopp...</p>
      </div>
    </div>
  );
}
