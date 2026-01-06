import { HiOutlineCursorClick } from "react-icons/hi";
import { useSharingContext } from "@/windows/screensharing/context";
import { TooltipContent, TooltipTrigger, Tooltip, TooltipProvider } from "../ui/tooltip";
import { BiSolidJoystick } from "react-icons/bi";
import useStore from "@/store/store";
import { SegmentedControl } from "../ui/segmented-control";
import { CustomIcons } from "../ui/icons";
import { cn } from "@/lib/utils";
import { PiScribbleLoopBold } from "react-icons/pi";
import { HiOutlineCog6Tooth } from "react-icons/hi2";
import { TbLineDashed } from "react-icons/tb";
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger } from "../ui/dropdown-menu";
import { useEffect, useRef } from "react";
import { tauriUtils } from "@/windows/window-utils";
import { TDrawingMode, PDrawingMode } from "@/payloads";

type ScreenSharingControlsProps = {
  className?: string;
};

export function ScreenSharingControls({ className }: ScreenSharingControlsProps = {}) {
  const { setIsSharingKeyEvents, setIsSharingMouse, drawingMode, setDrawingMode } = useSharingContext();
  const isInitialMount = useRef(true);
  const cachedDrawingModeRef = useRef<TDrawingMode | null>(null);

  // Load cached drawing mode on mount
  useEffect(() => {
    const loadCachedDrawingMode = async () => {
      try {
        const cachedMode = await tauriUtils.getLastDrawingMode();
        if (cachedMode) {
          const parsed = PDrawingMode.safeParse(JSON.parse(cachedMode));
          if (parsed.success && parsed.data.type !== "Disabled") {
            // Store the cached mode but don't set it yet - it will be used when switching to drawing
            // This is stored in a ref so it's available in handleRemoteControlChange
            cachedDrawingModeRef.current = parsed.data;
          }
        }
      } catch (error) {
        console.error("Failed to load cached drawing mode:", error);
      }
    };
    loadCachedDrawingMode();
  }, []);

  // Save drawing mode whenever it changes (but not on initial mount and not when Disabled)
  useEffect(() => {
    if (isInitialMount.current) {
      isInitialMount.current = false;
      return;
    }

    if (drawingMode.type !== "Disabled") {
      // Update the cached ref immediately
      cachedDrawingModeRef.current = drawingMode;

      const saveDrawingMode = async () => {
        try {
          await tauriUtils.setLastDrawingMode(JSON.stringify(drawingMode));
        } catch (error) {
          console.error("Failed to save drawing mode:", error);
        }
      };
      saveDrawingMode();
    }
  }, [drawingMode]);

  // Derive remoteControlStatus from drawingMode
  const remoteControlStatus = drawingMode.type === "Disabled" ? "controlling" : "drawing";

  const handleRemoteControlChange = (value: string) => {
    if (value === "controlling") {
      setIsSharingMouse(true);
      setIsSharingKeyEvents(true);
      setDrawingMode({ type: "Disabled" });
    } else if (value === "drawing") {
      setIsSharingMouse(false);
      setIsSharingKeyEvents(false);
      // Use cached mode if available, otherwise default to Draw mode
      if (drawingMode.type === "Disabled") {
        const modeToUse = cachedDrawingModeRef.current || { type: "Draw", settings: { permanent: false } };
        setDrawingMode(modeToUse);
      }
    }
  };

  const handleDrawingModeTypeChange = (value: string) => {
    if (value === "drawing") {
      setDrawingMode({
        type: "Draw",
        settings: {
          permanent: drawingMode.type === "Draw" ? drawingMode.settings.permanent : false,
        },
      });
    } else if (value === "clickAnimation") {
      setDrawingMode({ type: "ClickAnimation" });
    }
  };

  const handlePermanentModeChange = (checked: boolean) => {
    if (drawingMode.type === "Draw") {
      setDrawingMode({
        type: "Draw",
        settings: { permanent: checked },
      });
    }
  };

  // Derive UI state from drawingMode
  const drawingModeType =
    drawingMode.type === "Draw" ? "drawing"
    : drawingMode.type === "ClickAnimation" ? "clickAnimation"
    : "drawing";
  const isPermanentMode = drawingMode.type === "Draw" ? drawingMode.settings.permanent : false;

  return (
    <TooltipProvider>
      <div className={cn("w-full pt-2 flex flex-row items-center relative pointer-events-none", className)}>
        <div className="w-full flex justify-center">
          <div className="flex flex-row gap-1 items-center">
            <SegmentedControl
              items={[
                {
                  id: "controlling",
                  content: <HiOutlineCursorClick className="size-3" />,
                  tooltipContent: "Remote control",
                },
                {
                  id: "drawing",
                  content: <PiScribbleLoopBold className="size-3" />,
                  tooltipContent: "Drawing",
                },
              ]}
              value={remoteControlStatus}
              onValueChange={handleRemoteControlChange}
              className="pointer-events-auto"
            />
            {remoteControlStatus === "drawing" && (
              <DropdownMenu>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <DropdownMenuTrigger asChild>
                      <button
                        type="button"
                        className={cn(
                          "pointer-events-auto h-[28px] w-[28px] flex items-center justify-center rounded-lg",
                          "bg-gray-500/80 dark:bg-zinc-600 text-white hover:bg-gray-600/80 dark:hover:bg-zinc-700",
                          "transition-colors outline-none focus-visible:ring-2 focus-visible:ring-slate-400 focus-visible:ring-offset-2",
                        )}
                      >
                        <HiOutlineCog6Tooth className="size-3" />
                      </button>
                    </DropdownMenuTrigger>
                  </TooltipTrigger>
                  <TooltipContent>Drawing settings</TooltipContent>
                </Tooltip>
                <DropdownMenuContent
                  align="end"
                  className="w-fit min-w-0 p-0.5 bg-gray-500 dark:bg-zinc-600 border-gray-400/50 dark:border-zinc-500/50 text-white"
                >
                  <div className="flex flex-col gap-0.5">
                    <SegmentedControl
                      items={[
                        {
                          id: "drawing",
                          content: <PiScribbleLoopBold className="size-3" />,
                          tooltipContent: "Drawing",
                        },
                        {
                          id: "clickAnimation",
                          content: <CustomIcons.PointerClick className="size-3" />,
                          tooltipContent: "Click Animation",
                        },
                      ]}
                      value={drawingModeType}
                      onValueChange={handleDrawingModeTypeChange}
                      className="pointer-events-auto"
                    />
                    {drawingModeType === "drawing" && (
                      <>
                        <div className="h-px bg-gray-400/50 dark:bg-zinc-500/50" />
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <button
                              type="button"
                              onClick={() => handlePermanentModeChange(!isPermanentMode)}
                              className={cn(
                                "pointer-events-auto h-[28px] w-full px-2 flex items-center justify-center rounded-md",
                                "text-white transition-colors outline-none",
                                isPermanentMode ?
                                  "bg-slate-300/50 dark:bg-slate-300/50 hover:bg-slate-400/50 dark:hover:bg-slate-400/50"
                                : "bg-gray-500 dark:bg-zinc-600 hover:bg-gray-600 dark:hover:bg-zinc-700",
                              )}
                            >
                              <TbLineDashed className="size-4" />
                            </button>
                          </TooltipTrigger>
                          <TooltipContent>Permanent Mode</TooltipContent>
                        </Tooltip>
                      </>
                    )}
                  </div>
                </DropdownMenuContent>
              </DropdownMenu>
            )}
          </div>
        </div>
      </div>
    </TooltipProvider>
  );
}

export function RemoteControlDisabledIndicator() {
  const isRemoteControlEnabled = useStore((state) => state.callTokens?.isRemoteControlEnabled);

  if (isRemoteControlEnabled !== false) {
    return null;
  }

  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger>
          <div className="flex flex-row gap-1 items-center muted border text-white bg-gray-500/80 dark:border-slate-600 dark:text-white dark:bg-slate-700 px-1.5 py-0.5 rounded-md rounded-tr-xl">
            <BiSolidJoystick className="size-4" /> Remote control is disabled
          </div>
        </TooltipTrigger>
        <TooltipContent>
          <div>Ask the sharer to enable remote control.</div>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
