import { HiOutlineCursorClick } from "react-icons/hi";
import { useSharingContext } from "@/windows/screensharing/context";
import { TooltipContent, TooltipTrigger, Tooltip, TooltipProvider } from "../ui/tooltip";
import { BiSolidJoystick } from "react-icons/bi";
import { FaPencilAlt } from "react-icons/fa";
import useStore from "@/store/store";
import { SegmentedControl } from "../ui/segmented-control";
import { useState } from "react";
import { CustomIcons } from "../ui/icons";
import { cn } from "@/lib/utils";

type ScreenSharingControlsProps = {
  className?: string;
};

export function ScreenSharingControls({ className }: ScreenSharingControlsProps = {}) {
  const { setIsSharingKeyEvents, setIsSharingMouse, setIsDrawingMode } = useSharingContext();
  const [remoteControlStatus, setRemoteControlStatus] = useState<string>("controlling");

  const handleRemoteControlChange = (value: string) => {
    setRemoteControlStatus(value);
    if (value === "controlling") {
      setIsSharingMouse(true);
      setIsSharingKeyEvents(true);
      setIsDrawingMode(false);
    } else if (value === "drawing") {
      setIsSharingMouse(false);
      setIsSharingKeyEvents(false);
      setIsDrawingMode(true);
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
                  id: "controlling",
                  content: <HiOutlineCursorClick className="size-3" />,
                  tooltipContent: "Remote control",
                },
                {
                  id: "drawing",
                  content: <FaPencilAlt className="size-3.5 text-white" />,
                  tooltipContent: "Drawing",
                },
              ]}
              value={remoteControlStatus}
              onValueChange={handleRemoteControlChange}
              className="pointer-events-auto"
            />
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
