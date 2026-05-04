import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import useStore from "@/store/store";
import { socketService } from "@/services/socket";
import { Button } from "@/components/ui/button";
import { useEffect, useRef, useState } from "react";
import { Textarea } from "@/components/ui/textarea";
import { soundUtils } from "@/lib/sound_utils";
import { validateAndSetAuthToken } from "@/lib/authUtils";
import { invoke } from "@tauri-apps/api/core";
import { useQuery } from "@tanstack/react-query";
import { typedInvoke } from "@/core_payloads";

export const Debug = () => {
  const { callTokens, setCallTokens, authToken } = useStore();
  const [isPlaying, setIsPlaying] = useState(false);
  const [trayNotification, setTrayNotification] = useState(false);
  const [noiseCancellation, setNoiseCancellation] = useState(true);
  const soundRef = useRef(soundUtils.createPlayer("incoming-call"));

  const { refetch, data, isLoading, isFetching } = useQuery({
    queryKey: ["list_cameras"],
    queryFn: () => typedInvoke("list_webcams"),
  });

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
    typedInvoke("get_noise_cancellation").then(setNoiseCancellation);
  }, []);

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
          <Label htmlFor="callToken">Call Tokens</Label>
          <span className="muted">A field that you hopefully will never need to use 🫡</span>
          <Textarea
            placeholder="Call Token"
            value={JSON.stringify(callTokens, null, 2) || ""}
            onChange={(e) => {
              setCallTokens({
                ...JSON.parse(e.target.value),
                timeStarted: new Date(),
                hasAudioEnabled: false,
                hasCameraEnabled: false,
                isRemoteControlEnabled: true,
                participants: [],
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
      </div>

      <div className="flex flex-col gap-3 my-4 p-3 border rounded-lg bg-gray-50 dark:bg-gray-800">
        <Label className="text-sm font-medium">Core Process</Label>
        <Button onClick={() => refetch()} size="sm">
          {isLoading || isFetching ? "Loading..." : "List cameras"}
        </Button>
        {data && data.map((camera) => <div key={camera.id}>{camera.name}</div>)}
        <Button onClick={() => typedInvoke("open_stats_window")} size="sm">
          Open Stats Window
        </Button>
      </div>

      <div className="flex flex-col gap-3 my-4 p-3 border rounded-lg bg-gray-50 dark:bg-gray-800">
        <Label className="text-sm font-medium">App Settings</Label>
        <div className="flex items-center justify-between">
          <div className="flex flex-col gap-0.5">
            <Label htmlFor="noise-cancellation" className="text-sm">
              Noise Cancellation
            </Label>
            <span className="text-xs text-muted-foreground">Noise suppression on microphone input</span>
          </div>
          <Switch
            id="noise-cancellation"
            checked={noiseCancellation}
            onCheckedChange={(checked) => {
              setNoiseCancellation(checked);
              typedInvoke("set_noise_cancellation", { enabled: checked });
            }}
          />
        </div>
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
