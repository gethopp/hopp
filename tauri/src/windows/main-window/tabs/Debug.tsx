import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import useStore from "@/store/store";
import { socketService } from "@/services/socket";
import { Button } from "@/components/ui/button";
import { useEffect, useRef, useState } from "react";
import { Textarea } from "@/components/ui/textarea";
import { soundUtils } from "@/lib/sound_utils";
import { validateAndSetAuthToken } from "@/lib/authUtils";
import { URLS } from "@/constants";
import { tauriUtils } from "@/windows/window-utils";
import { usePostHog } from "posthog-js/react";
import { invoke } from "@tauri-apps/api/core";

export const Debug = () => {
  const { callTokens, setCallTokens, updateCallTokens, authToken, customServerUrl, setCustomServerUrl } = useStore();
  const [isPlaying, setIsPlaying] = useState(false);
  const [localServerUrl, setLocalServerUrl] = useState<string>(customServerUrl || "");
  const [trayNotification, setTrayNotification] = useState(false);
  const soundRef = useRef(soundUtils.createPlayer("incoming-call"));
  const posthog = usePostHog();

  const handleSetTrayNotification = async (enabled: boolean) => {
    try {
      await invoke("set_tray_notification", { enabled });
      setTrayNotification(enabled);
      console.log(`Tray notification set to: ${enabled}`);
    } catch (error) {
      console.error("Failed to set tray notification:", error);
    }
  };

  useEffect(() => {
    return () => {
      try {
        soundRef.current.stop?.();
      } catch (error) {
        console.error("Error stopping sound:", error);
      }
    };
  }, []);

  const toggleSound = async () => {
    console.log("Toggling sound");
    if (isPlaying) {
      soundRef.current.stop();
      setIsPlaying(false);
      return;
    }

    try {
      soundRef.current.play();
      setIsPlaying(true);
    } catch (error) {
      console.error("Error playing sound:", error);
    }
  };

  return (
    <div className="flex flex-col p-2">
      <h4>Debug screen</h4>
      <div className="flex flex-col gap-5">
        <div className="mt-3 mb-0">
          <span className="muted">Something in the app broke for you to be here 😅</span>
          <br />
          <span className="muted">Be sure to ping us so we can fix any bug 🐛</span>
        </div>
        <div className="grid w-full max-w-sm items-center gap-1.5">
          <Label htmlFor="authToken">Auth Token</Label>
          <span className="muted">Paste you authentication token that you copied from the web application.</span>
          <Input
            type="text"
            placeholder="Auth Token"
            value={authToken || ""}
            onChange={async (e) => {
              const newToken = e.target.value;
              await validateAndSetAuthToken(newToken);
            }}
          />
        </div>

        <div className="grid w-full max-w-sm items-center gap-1.5">
          <Label htmlFor="customServerUrl">Custom Backend URL</Label>
          <span className="muted">
            Override the default backend URL. Leave empty to use default ({URLS.API_BASE_URL}).
          </span>
          <Input
            type="text"
            placeholder={URLS.API_BASE_URL}
            value={localServerUrl}
            onChange={async (e) => {
              const newUrl = e.target.value;
              setLocalServerUrl(newUrl);
              const urlToSet = newUrl.trim() || null;
              setCustomServerUrl(urlToSet);
              await tauriUtils.setHoppServerUrl(urlToSet);
              posthog.capture("custom_backend_url_changed");
            }}
          />
        </div>

        <div className="grid w-full max-w-sm items-center gap-1.5">
          <Label htmlFor="callToken">Call Tokens</Label>
          <span className="muted">A field that you hopefully will never need to use 🫡</span>
          <Textarea
            placeholder="Call Token"
            value={JSON.stringify(callTokens, null, 2) || ""}
            onChange={(e) => {
              setCallTokens({
                ...JSON.parse(e.target.value),
                timeStarted: new Date(),
                // hasAudioEnabled: true,
                hasAudioEnabled: false,
                hasCameraEnabled: false,
                isRemoteControlEnabled: true,
                cameraTrackId: null,
              });
            }}
          />
        </div>
      </div>
      <div className="flex flex-col gap-3 my-4">
        <Button
          onClick={() =>
            socketService.send({
              type: "ping",
              payload: {
                message: "ping",
              },
            })
          }
        >
          Ping websocket
        </Button>
        <Button onClick={toggleSound}>{isPlaying ? "Stop call sound" : "Play call sound"}</Button>
        <Button
          onClick={() => {
            updateCallTokens({
              krispToggle: !(callTokens?.krispToggle ?? true),
            });
          }}
          variant={callTokens?.krispToggle === false ? "destructive" : "default"}
        >
          Krisp: {callTokens?.krispToggle === false ? "Disabled" : "Enabled"}
        </Button>
        <Button
          onClick={() => {
            updateCallTokens({
              av1Enabled: !(callTokens?.av1Enabled ?? false),
            });
          }}
          disabled={!(callTokens?.controllerSupportsAv1 && !callTokens?.isRoomCall)}
          variant={callTokens?.av1Enabled ? "default" : "destructive"}
        >
          AV1: {callTokens?.av1Enabled ? "Enabled" : "Disabled"}
        </Button>
      </div>

      <div className="flex flex-col gap-3 my-4 p-3 border rounded-lg bg-gray-50 dark:bg-gray-800">
        <Label className="text-sm font-medium">Tray Icon Test (macOS)</Label>
        <span className="text-xs text-muted-foreground">
          Test tray icon switching. The icon should automatically change with system theme.
        </span>
        <div className="flex gap-2">
          <Button
            onClick={() => handleSetTrayNotification(false)}
            variant={!trayNotification ? "default" : "outline"}
            size="sm"
          >
            Default Icon
          </Button>
          <Button
            onClick={() => handleSetTrayNotification(true)}
            variant={trayNotification ? "default" : "outline"}
            size="sm"
          >
            Notification Icon
          </Button>
        </div>
        <span className="text-xs text-muted-foreground">
          Current state: {trayNotification ? "Notification" : "Default"}
        </span>
      </div>
    </div>
  );
};
