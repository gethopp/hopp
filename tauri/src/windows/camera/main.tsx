import "@/services/sentry";
import "../../App.css";
import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { Toaster } from "react-hot-toast";
import { useDisableNativeContextMenu } from "@/lib/hooks";
import { tauriUtils } from "../window-utils";
import { LiveKitRoom, useTracks, VideoTrack } from "@livekit/components-react";
import { Track } from "livekit-client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { PhysicalSize, LogicalPosition, currentMonitor } from "@tauri-apps/api/window";
import { CgSpinner } from "react-icons/cg";
import { HiOutlineEye, HiOutlineEyeSlash } from "react-icons/hi2";
import { WindowActions } from "@/components/ui/window-buttons";
import { CustomIcons } from "@/components/ui/icons";
import { Button } from "@/components/ui/button";
import useStore from "@/store/store";

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
  const VideoCardPadding = 14; // 0.875rem

  if (trackLength === 0) {
    // Give initial size of one only card
    // and a loading placeholder will be added
    trackLength = 1;
  }

  const totalHeight = HeaderHeight + VideoCardHeight * trackLength + FlexGap * (trackLength - 1) + VideoCardPadding * 2;

  const appWindow = getCurrentWebviewWindow();
  const factor = await appWindow.scaleFactor();
  appWindow.setSize(new PhysicalSize(160 * factor, totalHeight * factor));
}

function ConsumerComponent({ hideSelf, setHideSelf }: { hideSelf: boolean; setHideSelf: (value: boolean) => void }) {
  const { callTokens } = useStore();

  const tracks = useTracks([Track.Source.Camera], {
    onlySubscribed: true,
  });

  const visibleTracks = tracks.filter((track) => {
    const isSelfTrack = callTokens?.cameraTrackId === track?.publication?.trackSid;
    return !(hideSelf && isSelfTrack);
  });

  useEffect(() => {
    console.log("tracks ", tracks);
    // Set window size appropriately
    CameraWindowSize({ numOfTracks: visibleTracks.length });
  }, [visibleTracks]);

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
        {visibleTracks.map((track) => {
          return (
            <div className="relative overflow-hidden rounded-lg group" key={track.sid}>
              <VideoTrack
                trackRef={track}
                className="rounded-lg object-cover overflow-hidden"
                style={{
                  aspectRatio: "1/1",
                  width: "140px",
                  height: "140px",
                  minHeight: "140px",
                  minWidth: "140px",
                  maxHeight: "140px",
                  maxWidth: "140px",
                  border:
                    track?.participant?.isSpeaking ?
                      "1px solid rgba(157, 253, 49, 0.8)"
                    : "1px solid rgba(0, 0, 0, 0.1)",
                }}
              />
              {callTokens?.cameraTrackId === track?.publication?.trackSid && (
                <div className="absolute inset-0 bg-black/50 opacity-0 group-hover:opacity-100 transition-opacity duration-200 flex items-center justify-center rounded-lg">
                  <Button
                    variant="secondary"
                    size="icon-sm"
                    className="bg-white/20 hover:bg-white/30 text-white border-white/20"
                    title="Hide participant"
                    onClick={() => setHideSelf(true)}
                  >
                    <HiOutlineEyeSlash className="w-4 h-4" />
                  </Button>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

const putWindowCorner = async () => {
  const appWindow = getCurrentWebviewWindow();

  try {
    // Get the current monitor information
    const monitor = await currentMonitor();
    if (!monitor) {
      console.error("Could not get current monitor information");
      return;
    }

    // Get the current window size in logical units
    const windowSize = await appWindow.outerSize();
    const scaleFactor = await appWindow.scaleFactor();

    const logicalMonitorWidth = monitor.size.width / scaleFactor;
    const logicalMonitorHeight = monitor.size.height / scaleFactor;
    const logicalAppWindowWidth = windowSize.width / scaleFactor;
    const rightGap = Math.floor(logicalMonitorWidth * 0.01);
    const topGap = Math.floor(logicalMonitorHeight * 0.08);

    // Calculate position for top-right corner with gap (in logical coordinates)
    const logicalMonitorX = monitor.position.x / scaleFactor;
    const logicalMonitorY = monitor.position.y / scaleFactor;

    const x = logicalMonitorX + logicalMonitorWidth - logicalAppWindowWidth - rightGap;
    const y = logicalMonitorY + topGap;

    // Set the window position
    await appWindow.setPosition(new LogicalPosition(x, y));
    console.log(`Positioned window at (${x}, ${y}) with ${rightGap}px gap from right edge`);
  } catch (error) {
    console.error("Failed to position window at corner:", error);
  }
};

function CameraWindow() {
  useDisableNativeContextMenu();
  const [cameraToken, setCameraToken] = useState<string | null>(null);
  const [hideSelf, setHideSelf] = useState(false);
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
        className="h-[36px] min-w-full bg-gray-500/40 rounded-none titlebar w-full flex flex-row items-center justify-start px-3 relative"
      >
        <WindowActions.Empty onClick={() => putWindowCorner()} className=" justify-self-start">
          <CustomIcons.Corner />
        </WindowActions.Empty>
        <CustomIcons.Drag className="absolute left-1/2 -translate-x-1/2 pointer-events-none" />
        {/* <div className="pointer-events-none ml-auto font-medium text-white/80 text-[12px]">+2 more users</div> */}
        {hideSelf && (
          <Button
            variant="ghost"
            size="icon-sm"
            className="ml-auto text-white/80 hover:text-white hover:bg-white/10"
            onClick={() => setHideSelf(false)}
            title="Show self"
          >
            <HiOutlineEye className="w-4 h-4" />
          </Button>
        )}
      </div>
      <Toaster position="bottom-center" />
      <LiveKitRoom token={cameraToken ?? undefined} serverUrl={livekitUrl}>
        <ConsumerComponent hideSelf={hideSelf} setHideSelf={setHideSelf} />
      </LiveKitRoom>
    </div>
  );
}
