import "@/services/sentry";
import "../../App.css";
import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { Toaster } from "react-hot-toast";
import { useDisableNativeContextMenu } from "@/lib/hooks";
import { tauriUtils } from "../window-utils";
import { LiveKitRoom, useTracks, VideoTrack } from "@livekit/components-react";
import { Track } from "livekit-client";
import { IoClose } from "react-icons/io5";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { PhysicalSize } from "@tauri-apps/api/window";
import { CgSpinner } from "react-icons/cg";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <CameraWindow />
  </React.StrictMode>,
);

async function CameraWindowSize({ numOfTracks }: { numOfTracks: number }) {
  let trackLength = numOfTracks;

  // All values are in pixels
  const FlexGap = 4; // 0.25rem
  const VideoCardHeight = 140;
  const HeaderHeight = 36;
  const VideoCardPadding = 16; // 1rem

  if (trackLength === 0) {
    // Give initial size of one only card
    // and a loading placeholder will be added
    trackLength = 1;
  }

  const totalHeight = HeaderHeight + VideoCardHeight * trackLength + FlexGap * (trackLength - 1) + VideoCardPadding * 2;

  const appWindow = getCurrentWebviewWindow();
  const factor = await appWindow.scaleFactor();
  console.log("Scale factor: ", factor);
  console.log("CameraWindowSize: ", totalHeight * factor);
  appWindow.setSize(new PhysicalSize(160 * factor, totalHeight * factor));
}

function ConsumerComponent() {
  const tracks = useTracks([Track.Source.Camera], {
    onlySubscribed: true,
  });

  useEffect(() => {
    // Set window size appropriately
    CameraWindowSize({ numOfTracks: tracks.length });
  }, [tracks]);

  return (
    <div className="content px-2 py-4">
      <div className="flex flex-col gap-1 items-center justify-center h-full">
        {tracks.length === 0 && (
          <div
            style={{
              aspectRatio: "1/1",
              width: "140px",
              height: "140px",
              minHeight: "140px",
              minWidth: "140px",
              maxHeight: "140px",
              maxWidth: "140px",
            }}
            className="flex flex-col rounded-lg items-center justify-center border border-slate-600/20 bg-slate-600/30"
          >
            <CgSpinner className="animate-spin" />
            <span className="text-sm text-white/80">Loading</span>
          </div>
        )}
        {tracks.map((track) => {
          return (
            <VideoTrack
              trackRef={track}
              className="rounded-lg object-cover"
              style={{
                aspectRatio: "1/1",
                width: "140px",
                height: "140px",
                minHeight: "140px",
                minWidth: "140px",
                maxHeight: "140px",
                maxWidth: "140px",
                border:
                  track?.participant?.isSpeaking ? "1px solid rgba(157, 253, 49, 0.8)" : "1px solid rgba(0, 0, 0, 0.1)",
              }}
            />
          );
        })}
      </div>
    </div>
  );
}

function CameraWindow() {
  useDisableNativeContextMenu();
  const [cameraToken, setCameraToken] = useState<string | null>(null);

  const [livekitUrl, setLivekitUrl] = useState<string>("");

  useEffect(() => {
    // Set correct window size
    CameraWindowSize({ numOfTracks: 0 });

    const cameraTokenFromUrl = tauriUtils.getTokenParam("cameraToken");

    if (cameraTokenFromUrl) {
      setCameraToken(cameraTokenFromUrl);
    }

    const getLivekitUrl = async () => {
      const url = await tauriUtils.getLivekitUrl();
      setLivekitUrl(url);
    };
    getLivekitUrl();

    async function enableDock() {
      await tauriUtils.setDockIconVisible(true);
    }

    enableDock();
  }, []);

  return (
    <div className="h-full min-h-full overflow-hidden bg-transparent text-white">
      <div
        data-tauri-drag-region
        className="h-[36px] min-w-full bg-gray-500/40 rounded-none titlebar w-full flex flex-row flex-between items-center px-3"
      >
        <div className="pointer-events-none size-[14px] rounded-full bg-white/50 text-gray-600 flex items-center justify-center">
          <IoClose />
        </div>
        <div className="pointer-events-none ml-auto font-medium text-white/80 text-[12px]">+2 more users</div>
      </div>
      <Toaster position="bottom-center" />
      <LiveKitRoom token={cameraToken ?? undefined} serverUrl={livekitUrl}>
        <ConsumerComponent />
      </LiveKitRoom>
    </div>
  );
}
