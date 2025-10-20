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
import { RiExpandDiagonalLine, RiCollapseDiagonalLine } from "react-icons/ri";
import { WindowActions } from "@/components/ui/window-buttons";
import { CustomIcons } from "@/components/ui/icons";
import { Button } from "@/components/ui/button";
import useStore from "@/store/store";
import clsx from "clsx";
import ListenToRemoteAudio from "@/components/ui/listen-to-remote-audio";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <CameraWindow />
  </React.StrictMode>,
);

const EXPANSION_FACTOR = 1.3;

async function CameraWindowSize({
  numOfTracks,
  expansionFactor = 1,
}: {
  numOfTracks: number;
  expansionFactor?: number;
}) {
  let trackLength = numOfTracks;

  // All values are in pixels
  const FlexGap = 4; // 0.25rem
  const VideoCardHeight = 140 * expansionFactor;
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
  console.log(
    `Tracks ${trackLength}`,
    `Expansion factor ${expansionFactor}`,
    `Factor ${factor}`,
    `Total height ${totalHeight}`,
  );
  appWindow.setSize(new PhysicalSize(Math.floor(160 * expansionFactor * factor), Math.floor(totalHeight * factor)));
}

function ConsumerComponent({
  hideSelf,
  setHideSelf,
  isExpanded,
}: {
  hideSelf: boolean;
  setHideSelf: (value: boolean) => void;
  isExpanded: boolean;
}) {
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
    CameraWindowSize({ numOfTracks: visibleTracks.length, expansionFactor: isExpanded ? EXPANSION_FACTOR : 1 });
  }, [visibleTracks, isExpanded]);

  const factor = isExpanded ? EXPANSION_FACTOR : 1;

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
          const isSelfTrack = callTokens?.cameraTrackId === track?.publication?.trackSid;

          return (
            <div className="relative overflow-hidden rounded-lg group" key={track.sid}>
              <VideoTrack
                trackRef={track}
                className="rounded-lg object-cover overflow-hidden"
                style={{
                  aspectRatio: "1/1",
                  width: `${Math.floor(140 * factor)}px`,
                  height: `${Math.floor(140 * factor)}px`,
                  minHeight: `${Math.floor(140 * factor)}px`,
                  minWidth: `${Math.floor(140 * factor)}px`,
                  maxHeight: `${Math.floor(140 * factor)}px`,
                  maxWidth: `${Math.floor(140 * factor)}px`,
                  border:
                    track?.participant?.isSpeaking ?
                      "1px solid rgba(157, 253, 49, 0.8)"
                    : "1px solid rgba(0, 0, 0, 0.1)",
                  transform: isSelfTrack ? "scaleX(-1)" : undefined,
                }}
              />
              {isSelfTrack && (
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
        <ListenToRemoteAudio />
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
  const [isSelfHidden, setIsSelfHidden] = useState(false);
  const [livekitUrl, setLivekitUrl] = useState<string>("");
  const [isExpanded, setIsExpanded] = useState(false);

  useEffect(() => {
    // Set correct window size
    CameraWindowSize({ numOfTracks: 0, expansionFactor: isExpanded ? EXPANSION_FACTOR : 1 });

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
        <CustomIcons.Drag
          className={clsx(
            "absolute left-1/2 -translate-x-1/2 pointer-events-none",
            isSelfHidden ? "left-[33%] -translate-x-[33%]" : "left-1/2 -translate-x-1/2",
          )}
        />
        {/* <div className="pointer-events-none ml-auto font-medium text-white/80 text-[12px]">+2 more users</div> */}
        <div className="ml-auto flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon-sm"
            className="text-white/80 hover:text-white hover:bg-white/10"
            onClick={() => setIsExpanded(!isExpanded)}
            title={isExpanded ? "Collapse window" : "Expand window"}
          >
            {isExpanded ?
              <RiCollapseDiagonalLine className="size-4" />
            : <RiExpandDiagonalLine className="size-4" />}
          </Button>
          {isSelfHidden && (
            <Button
              variant="ghost"
              size="icon-sm"
              className="text-white/80 hover:text-white hover:bg-white/10"
              onClick={() => setIsSelfHidden(false)}
              title="Show self"
            >
              <HiOutlineEye className="w-4 h-4" />
            </Button>
          )}
        </div>
      </div>
      <Toaster position="bottom-center" />
      {cameraToken && livekitUrl && (
        <LiveKitRoom token={cameraToken} serverUrl={livekitUrl}>
          <ConsumerComponent hideSelf={isSelfHidden} setHideSelf={setIsSelfHidden} isExpanded={isExpanded} />
        </LiveKitRoom>
      )}
    </div>
  );
}
