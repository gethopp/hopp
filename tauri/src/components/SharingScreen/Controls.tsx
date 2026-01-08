import { HiOutlineCursorClick } from "react-icons/hi";
import { useSharingContext } from "@/windows/screensharing/context";
import { TooltipContent, TooltipTrigger, Tooltip, TooltipProvider } from "../ui/tooltip";
import { BiSolidJoystick } from "react-icons/bi";
import useStore from "@/store/store";
import { SegmentedControl } from "../ui/segmented-control";
import { CustomIcons } from "../ui/icons";
import { cn } from "@/lib/utils";
import { PiScribbleLoopBold } from "react-icons/pi";
import { HiCog6Tooth } from "react-icons/hi2";
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger, DropdownMenuCheckboxItem } from "../ui/dropdown-menu";
import { useEffect, useRef } from "react";
import { tauriUtils } from "@/windows/window-utils";
import { TDrawingMode, PDrawingMode } from "@/payloads";

type ScreenSharingControlsProps = {
  className?: string;
};

export function ScreenSharingControls({ className }: ScreenSharingControlsProps = {}) {
  const { setIsSharingKeyEvents, setIsSharingMouse, drawingMode, setDrawingMode, triggerClearDrawings } =
    useSharingContext();
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
            // This is stored in a ref so it's available in handleModeChange
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

  // Derive current mode from drawingMode
  const currentMode =
    drawingMode.type === "Disabled" ? "pointer"
    : drawingMode.type === "Draw" ? "drawing"
    : "clickAnimation";

  const handleModeChange = (value: string) => {
    // Clear all drawings when leaving Draw mode (the scribble mode)
    if (drawingMode.type === "Draw" && value !== "drawing") {
      triggerClearDrawings();
    }

    if (value === "pointer") {
      setIsSharingMouse(true);
      setIsSharingKeyEvents(true);
      setDrawingMode({ type: "Disabled" });
    } else if (value === "drawing") {
      setIsSharingMouse(false);
      setIsSharingKeyEvents(false);
      // Use cached Draw mode settings if available
      const cachedSettings =
        cachedDrawingModeRef.current?.type === "Draw" ? cachedDrawingModeRef.current.settings : { permanent: false };
      setDrawingMode({ type: "Draw", settings: cachedSettings });
    } else if (value === "clickAnimation") {
      setIsSharingMouse(false);
      setIsSharingKeyEvents(false);
      setDrawingMode({ type: "ClickAnimation" });
    }
  };

  return (
    <TooltipProvider>
      <div className={cn("w-full pt-2 flex flex-row items-center relative pointer-events-none", className)}>
        <div className="w-full flex justify-center">
          <div className="flex flex-row gap-1 items-center">
            <SegmentedControl
              items={[
                {
                  id: "pointer",
                  content: <HiOutlineCursorClick className="size-3" />,
                  tooltipContent: "Remote control",
                },
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
              value={currentMode}
              onValueChange={handleModeChange}
              className="pointer-events-auto"
            />
          </div>
        </div>
      </div>
    </TooltipProvider>
  );
}

export function DrawingSettingsButton() {
  const { drawingMode, rightClickToClear, setRightClickToClear } = useSharingContext();
  const isDrawingMode = drawingMode.type === "Draw";

  if (!isDrawingMode) {
    return null;
  }

  return (
    <TooltipProvider>
      <DropdownMenu>
        <Tooltip>
          <TooltipTrigger asChild>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                className={cn(
                  "h-[28px] w-[28px] flex items-center justify-center rounded-lg",
                  "bg-gray-500/80 dark:bg-zinc-600 text-white hover:bg-gray-600/80 dark:hover:bg-zinc-700",
                  "transition-colors outline-none focus-visible:ring-2 focus-visible:ring-slate-400 focus-visible:ring-offset-2",
                )}
              >
                <HiCog6Tooth className="size-3" />
              </button>
            </DropdownMenuTrigger>
          </TooltipTrigger>
          <TooltipContent>Drawing settings</TooltipContent>
        </Tooltip>
        <DropdownMenuContent
          align="end"
          className="w-auto min-w-[180px] bg-white dark:bg-zinc-800 border-slate-200 dark:border-zinc-700"
        >
          <DropdownMenuCheckboxItem
            checked={rightClickToClear}
            onCheckedChange={setRightClickToClear}
            className="flex items-center justify-between gap-4"
          >
            <span>Click to remove drawing</span>
            <kbd className="ml-auto pointer-events-none inline-flex h-5 select-none items-center gap-1 rounded border border-slate-200 dark:border-zinc-600 bg-slate-100 dark:bg-zinc-700 px-1.5 font-mono text-[10px] font-medium text-slate-600 dark:text-slate-300">
              Right click
            </kbd>
          </DropdownMenuCheckboxItem>
        </DropdownMenuContent>
      </DropdownMenu>
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
