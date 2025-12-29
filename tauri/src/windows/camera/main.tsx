import "@/services/sentry";
import "../../App.css";
import React, { useEffect, useMemo, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { Toaster } from "react-hot-toast";
import { useDisableNativeContextMenu, useInboundCameraBandwidthMonitor, useSystemTheme } from "@/lib/hooks";
import { tauriUtils } from "../window-utils";
import { LiveKitRoom, useTracks, VideoTrack } from "@livekit/components-react";
import { Track, VideoQuality } from "livekit-client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { PhysicalSize, LogicalPosition, currentMonitor } from "@tauri-apps/api/window";
import { CgSpinner } from "react-icons/cg";
import { HiOutlineEye, HiOutlineEyeSlash } from "react-icons/hi2";
import { VscChromeMinimize } from "react-icons/vsc";
import { WindowActions } from "@/components/ui/window-buttons";
import { CustomIcons } from "@/components/ui/icons";
import { Button } from "@/components/ui/button";
import useStore from "@/store/store";
import clsx from "clsx";
import ListenToRemoteAudio from "@/components/ui/listen-to-remote-audio";
import { IoGridOutline } from "react-icons/io5";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <CameraWindow />
  </React.StrictMode>,
);

// Define the three size modes
type SizeMode = "small" | "medium" | "big";

// Size configurations for each mode
const SIZE_CONFIG = {
  small: {
    videoSize: 140, // All 216p quality
    quality: VideoQuality.LOW, // 216p
    label: "Small (216p)",
  },
  medium: {
    videoSize: 280, // All 360p quality
    quality: VideoQuality.MEDIUM, // 360p
    label: "Medium (360p)",
  },
  big: {
    videoSize: 560, // All 720p quality
    quality: VideoQuality.HIGH, // 720p
    label: "Big (720p)",
  },
} as const;

type GridShape = {
  cols: number;
  rows: number;
};

function getGridShape(trackCount: number): GridShape {
  const safeCount = Math.max(trackCount, 1);
  const cols = Math.min(2, safeCount);
  const rows = Math.max(1, Math.ceil(safeCount / 2));
  return { cols, rows };
}

function clampVideoSizeToMonitor({
  baseSize,
  rows,
  headerHeight,
  padding,
  gap,
  logicalMonitorHeight,
  modeLabel,
}: {
  baseSize: number;
  rows: number;
  headerHeight: number;
  padding: number;
  gap: number;
  logicalMonitorHeight: number;
  modeLabel: SizeMode;
}): number {
  const maxAvailableHeight = logicalMonitorHeight * 0.9;
  const calculatedHeight = headerHeight + rows * baseSize + (rows - 1) * gap + padding * 2;

  if (calculatedHeight <= maxAvailableHeight) {
    return baseSize;
  }

  const availableForVideos = maxAvailableHeight - headerHeight - (rows - 1) * gap - padding * 2;
  const resizedSize = Math.floor(availableForVideos / rows);

  console.log(
    `${modeLabel} mode - Scaling down from ${baseSize}px to ${resizedSize}px to fit screen`,
    `Monitor height: ${logicalMonitorHeight}px, Max available: ${maxAvailableHeight}px`,
  );

  return resizedSize;
}

async function CameraWindowSize({
  numOfTracks,
  sizeMode,
}: {
  numOfTracks: number;
  sizeMode: SizeMode;
}): Promise<number> {
  let trackLength = numOfTracks;

  // All values are in pixels
  const FlexGap = 4; // 0.25rem
  const HeaderHeight = 36;
  const VideoCardPadding = 14; // 0.875rem

  if (trackLength === 0) {
    // Give initial size of one only card
    // and a loading placeholder will be added
    trackLength = 1;
  }

  const appWindow = getCurrentWebviewWindow();
  const factor = await appWindow.scaleFactor();

  let totalHeight: number;
  let totalWidth: number;
  let actualVideoSize: number;
  const isGridMode = sizeMode !== "small";

  if (isGridMode) {
    // For medium and big modes: grid layout with 2 columns max (or 1 column if only 1 track)
    let videoSize: number = SIZE_CONFIG[sizeMode].videoSize;

    if (trackLength === 1) {
      // Single track: use full width (same as small/medium modes)
      actualVideoSize = videoSize;
      totalWidth = videoSize + VideoCardPadding * 2;
      totalHeight = HeaderHeight + videoSize + VideoCardPadding * 2;
      console.log(
        `${sizeMode} mode (Single) - Tracks: 1, Video size: ${videoSize}px`,
        `Height: ${totalHeight}px, Width: ${totalWidth}px`,
      );
    } else {
      // Multiple tracks: use grid with up to 2 columns
      const { cols, rows } = getGridShape(trackLength);
      const monitor = rows > 1 ? await currentMonitor() : null;

      if (monitor) {
        const logicalMonitorHeight = monitor.size.height / factor;
        videoSize = clampVideoSizeToMonitor({
          baseSize: videoSize,
          rows,
          headerHeight: HeaderHeight,
          padding: VideoCardPadding,
          gap: FlexGap,
          logicalMonitorHeight,
          modeLabel: sizeMode,
        });
      }

      actualVideoSize = videoSize;

      // Width: columns * videoSize + gaps between + padding
      totalWidth = cols * videoSize + (cols - 1) * FlexGap + VideoCardPadding * 2;

      // Height: header + rows * videoSize + gaps between rows + padding
      totalHeight = HeaderHeight + rows * videoSize + (rows - 1) * FlexGap + VideoCardPadding * 2;

      console.log(
        `${sizeMode} mode (Grid) - Tracks: ${trackLength}, Cols: ${cols}, Rows: ${rows}, Video size: ${videoSize}px`,
        `Height: ${totalHeight}px, Width: ${totalWidth}px`,
      );
    }
  } else {
    // For small mode: vertical stack with all same size
    const videoSize = SIZE_CONFIG.small.videoSize;
    actualVideoSize = videoSize;
    totalHeight = HeaderHeight + videoSize * trackLength + FlexGap * (trackLength - 1) + VideoCardPadding * 2;
    totalWidth = videoSize + VideoCardPadding * 2;

    console.log(
      `${sizeMode} mode - Tracks: ${trackLength}, Video size: ${videoSize}px`,
      `Height: ${totalHeight}px, Width: ${totalWidth}px`,
    );
  }

  appWindow.setSize(new PhysicalSize(Math.floor(totalWidth * factor), Math.floor(totalHeight * factor)));
  return actualVideoSize;
}

function ConsumerComponent({
  hideSelf,
  setHideSelf,
  sizeMode,
}: {
  hideSelf: boolean;
  setHideSelf: (value: boolean) => void;
  sizeMode: SizeMode;
}) {
  //useInboundCameraBandwidthMonitor();
  const { callTokens } = useStore();
  const [actualVideoSize, setActualVideoSize] = useState<number>(SIZE_CONFIG[sizeMode].videoSize);

  const tracks = useTracks([Track.Source.Camera], {
    onlySubscribed: true,
  });

  const selfTrackSid = callTokens?.cameraTrackId;
  const selfTrack = useMemo(() => {
    if (!selfTrackSid) {
      return undefined;
    }
    return tracks.find((track) => track?.publication?.trackSid === selfTrackSid);
  }, [tracks, selfTrackSid]);

  const visibleTracks = useMemo(() => {
    return tracks.filter((track) => {
      const isSelfTrack = callTokens?.cameraTrackId === track?.publication?.trackSid;
      return !(hideSelf && isSelfTrack);
    });
  }, [tracks, hideSelf, callTokens?.cameraTrackId]);
  const visibleTrackCount = visibleTracks.length;

  useEffect(() => {
    const publication = selfTrack?.publication as { setVideoQuality?: (quality: VideoQuality) => void } | undefined;
    if (!publication?.setVideoQuality) {
      return;
    }

    // Limit self track quality to MEDIUM to save bandwidth
    let targetQuality = SIZE_CONFIG[sizeMode].quality;
    if (targetQuality === VideoQuality.HIGH) {
      targetQuality = VideoQuality.MEDIUM;
    }

    if (hideSelf) {
      targetQuality = VideoQuality.LOW;
    }
    publication.setVideoQuality(targetQuality);
  }, [selfTrack, hideSelf, sizeMode]);

  useEffect(() => {
    // Set window size appropriately and get the actual video size used
    CameraWindowSize({ numOfTracks: visibleTrackCount, sizeMode }).then((size) => {
      setActualVideoSize(size);
    });
  }, [visibleTrackCount, sizeMode]);

  // Get quality based on mode, but use the actual calculated video size
  const videoSize = actualVideoSize;
  const quality = SIZE_CONFIG[sizeMode].quality;

  // For medium and big modes, use grid layout (2 per row) when there are 2+ tracks
  const isGridLayout = sizeMode !== "small" && visibleTracks.length >= 2;

  return (
    <div className="content px-2 py-4">
      <div
        className={clsx(
          "gap-1 h-full",
          isGridLayout ? "grid grid-cols-2 items-start justify-center" : "flex flex-col items-center justify-center",
        )}
      >
        {tracks.length === 0 && (
          <div
            style={{
              aspectRatio: "1/1",
              width: `${videoSize}px`,
              height: `${videoSize}px`,
              minHeight: `${videoSize}px`,
              minWidth: `${videoSize}px`,
              maxHeight: `${videoSize}px`,
              maxWidth: `${videoSize}px`,
            }}
            className="flex flex-col rounded-lg items-center justify-center border border-slate-400/30 dark:border-slate-600/20 bg-slate-400/30 dark:bg-slate-600/30"
          >
            <CgSpinner className="animate-spin" />
            <span className="text-sm text-black/80 dark:text-white/80">Loading</span>
          </div>
        )}
        {visibleTracks.map((track) => (
          <VideoTrackComponent
            key={track?.publication?.trackSid}
            track={track}
            size={videoSize}
            quality={quality}
            callTokens={callTokens}
            setHideSelf={setHideSelf}
          />
        ))}
        <ListenToRemoteAudio />
      </div>
    </div>
  );
}

// Helper component to render a video track with specified size and quality
function VideoTrackComponent({
  track,
  size,
  quality,
  callTokens,
  setHideSelf,
}: {
  track: any;
  size: number;
  quality: VideoQuality;
  callTokens: any;
  setHideSelf: (value: boolean) => void;
}) {
  //useInboundCameraBandwidthMonitor();
  const isSelfTrack = callTokens?.cameraTrackId === track?.publication?.trackSid;
  const sid = track?.publication?.trackSid;

  // Set the desired video quality for this track
  useEffect(() => {
    if (track?.publication) {
      track.publication.setVideoQuality(quality);
      console.log(`Set video quality to ${quality} for track ${sid}`);
    }
  }, [track, quality, sid]);

  return (
    <div className="relative overflow-hidden rounded-lg group" key={sid}>
      <VideoTrack
        trackRef={track}
        className="rounded-lg object-cover overflow-hidden"
        style={{
          aspectRatio: "1/1",
          width: `${size}px`,
          height: `${size}px`,
          minHeight: `${size}px`,
          minWidth: `${size}px`,
          maxHeight: `${size}px`,
          maxWidth: `${size}px`,
          border: track?.participant?.isSpeaking ? "1px solid rgba(157, 253, 49, 0.8)" : "1px solid rgba(0, 0, 0, 0.1)",
          transform: isSelfTrack ? "scaleX(-1)" : undefined,
        }}
        onSubscriptionStatusChanged={(status) => {
          console.log(`Track ${sid} subscription status:`, status);
        }}
      />
      {isSelfTrack && (
        <div className="absolute inset-0 bg-black/50 opacity-0 group-hover:opacity-100 transition-opacity duration-200 flex items-center justify-center rounded-lg">
          <Button
            variant="secondary"
            size="icon-sm"
            className="bg-white/20 hover:bg-white/30 text-white border-white/20"
            onClick={() => setHideSelf(true)}
          >
            <HiOutlineEyeSlash className="w-4 h-4" />
          </Button>
        </div>
      )}
    </div>
  );
}

// Size Mode Selector Component
function SizeModeSelector({
  currentMode,
  onModeChange,
}: {
  currentMode: SizeMode;
  onModeChange: (mode: SizeMode) => void;
}) {
  const [isOpen, setIsOpen] = useState(false);

  const modes: Array<{ mode: SizeMode; label: string; description: string }> = [
    { mode: "small", label: "Small", description: "All 216p" },
    { mode: "medium", label: "Medium", description: "All 360p" },
    { mode: "big", label: "Big", description: "Speaker 720p" },
  ];

  return (
    <div className="relative">
      <Button
        variant="ghost"
        size="icon-sm"
        className="text-black/80 dark:text-white/80 hover:text-black dark:hover:text-white hover:bg-black/10 dark:hover:bg-white/10"
        onClick={() => setIsOpen(!isOpen)}
      >
        <IoGridOutline className="size-4" />
      </Button>

      {isOpen && (
        <>
          {/* Backdrop */}
          <div className="fixed inset-0 z-40" onClick={() => setIsOpen(false)} />

          {/* Modal - positioned below the button */}
          <div className="absolute right-0 top-full mt-1 z-50 bg-white/40 dark:bg-gray-500/40 backdrop-blur-sm rounded-md shadow-lg border border-black/10 dark:border-white/10 p-1.5 flex gap-1 w-max">
            {modes.map(({ mode }) => (
              <button
                key={mode}
                onClick={() => {
                  onModeChange(mode);
                  setIsOpen(false);
                }}
                className={clsx(
                  "p-1.5 rounded transition-all hover:bg-black/20 dark:hover:bg-white/20 shrink-0",
                  currentMode === mode ?
                    "bg-white/40 dark:bg-white/20 ring-1 ring-black/40 dark:ring-white/40"
                  : "bg-transparent",
                )}
                title={
                  mode === "small" ? "Small (216p)"
                  : mode === "medium" ?
                    "Medium (360p)"
                  : "Big (720p Grid)"
                }
              >
                {/* Visual representation */}
                <div className="flex items-center justify-center">
                  {mode === "small" && (
                    // Small: 3 equal small squares vertically
                    <div className="flex flex-col gap-0.5">
                      <div className="w-2.5 h-2.5 bg-white/60 dark:bg-white/60 rounded-sm" />
                      <div className="w-2.5 h-2.5 bg-white/60 dark:bg-white/60 rounded-sm" />
                      <div className="w-2.5 h-2.5 bg-white/60 dark:bg-white/60 rounded-sm" />
                    </div>
                  )}
                  {mode === "medium" && (
                    // Medium: grid layout with slightly smaller squares
                    <div className="grid grid-cols-2 gap-0.5">
                      <div className="w-3 h-3 bg-white/60 dark:bg-white/60 rounded-sm" />
                      <div className="w-3 h-3 bg-white/60 dark:bg-white/60 rounded-sm" />
                      <div className="w-3 h-3 bg-white/60 dark:bg-white/60 rounded-sm" />
                      <div className="w-3 h-3 bg-white/60 dark:bg-white/60 rounded-sm" />
                    </div>
                  )}
                  {mode === "big" && (
                    // Big: Grid with 2 columns
                    <div className="grid grid-cols-2 gap-0.5">
                      <div className="w-4 h-4 bg-white/60 dark:bg-white/60 rounded-sm" />
                      <div className="w-4 h-4 bg-white/60 dark:bg-white/60 rounded-sm" />
                      <div className="w-4 h-4 bg-white/60 dark:bg-white/60 rounded-sm" />
                      <div className="w-4 h-4 bg-white/60 dark:bg-white/60 rounded-sm" />
                    </div>
                  )}
                </div>
              </button>
            ))}
          </div>
        </>
      )}
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
  useSystemTheme();
  const [cameraToken, setCameraToken] = useState<string | null>(null);
  const [isSelfHidden, setIsSelfHidden] = useState(false);
  const [livekitUrl, setLivekitUrl] = useState<string>("");
  const [sizeMode, setSizeMode] = useState<SizeMode>("small");
  const initialSizeModeRef = useRef<SizeMode>(sizeMode);

  useEffect(() => {
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

    CameraWindowSize({ numOfTracks: 0, sizeMode: initialSizeModeRef.current }).catch((err) => {
      console.error("Error setting initial camera window size:", err);
    });
  }, []);

  return (
    <div className="h-full min-h-full overflow-hidden bg-[#ECECEC] dark:bg-[#323232] text-black dark:text-white rounded-[12px]">
      <div
        data-tauri-drag-region
        className="h-[36px] min-w-full bg-black/10 dark:bg-white/10 rounded-t-[12px] titlebar w-full flex flex-row items-center justify-start px-3 relative overflow-visible"
      >
        <WindowActions.Empty onClick={() => putWindowCorner()} className=" justify-self-start">
          <CustomIcons.Corner />
        </WindowActions.Empty>
        {/* <div className="pointer-events-none ml-auto font-medium text-black/80 dark:text-white/80 text-[12px]">+2 more users</div> */}
        <div className="ml-auto flex items-center gap-1">
          <Button
            variant="ghost"
            size="icon-sm"
            className="text-black/80 dark:text-white/80 hover:text-black dark:hover:text-white hover:bg-black/10 dark:hover:bg-white/10"
            onClick={() => getCurrentWebviewWindow().minimize()}
          >
            <VscChromeMinimize className="size-4" />
          </Button>
          <SizeModeSelector currentMode={sizeMode} onModeChange={setSizeMode} />
          {isSelfHidden && (
            <Button
              variant="ghost"
              size="icon-sm"
              className="text-black/80 dark:text-white/80 hover:text-black dark:hover:text-white hover:bg-black/10 dark:hover:bg-white/10"
              onClick={() => setIsSelfHidden(false)}
            >
              <HiOutlineEye className="w-4 h-4" />
            </Button>
          )}
        </div>
      </div>
      <Toaster position="bottom-center" />
      {cameraToken && livekitUrl && (
        <LiveKitRoom token={cameraToken} serverUrl={livekitUrl}>
          <ConsumerComponent hideSelf={isSelfHidden} setHideSelf={setIsSelfHidden} sizeMode={sizeMode} />
        </LiveKitRoom>
      )}
    </div>
  );
}
