import "@/services/sentry";
import "../../App.css";
import React, { useCallback, useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { SharingScreen } from "@/components/SharingScreen/SharingScreen";
import { SharingProvider, useSharingContext } from "./context";
import {
  ScreenSharingControls,
  RemoteControlDisabledIndicator,
  DrawingSettingsButton,
} from "@/components/SharingScreen/Controls";
import { Toaster } from "react-hot-toast";
import { useDisableNativeContextMenu, useResizeListener, useSystemTheme } from "@/lib/hooks";
import { cn } from "@/lib/utils";
import { tauriUtils } from "../window-utils";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { PhysicalSize, PhysicalPosition, currentMonitor } from "@tauri-apps/api/window";
import { setWindowToMaxStreamSize } from "@/components/SharingScreen/utils";
import { LuMaximize2, LuMinimize2, LuMinus, LuX } from "react-icons/lu";

const appWindow = getCurrentWebviewWindow();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <SharingProvider>
      <Window />
    </SharingProvider>
  </React.StrictMode>,
);

const TitlebarButton = ({
  onClick,
  disabled,
  children,
  label,
  className,
}: {
  onClick?: () => void;
  disabled?: boolean;
  children: React.ReactNode;
  label: string;
  className?: string;
}) => {
  return (
    <button
      type="button"
      data-tauri-drag-region="no-drag"
      onClick={disabled ? undefined : onClick}
      disabled={disabled}
      aria-label={label}
      className={cn(
        "group relative size-[16px] border border-black/20 dark:border-white/20 bg-black/10 dark:bg-white/10 text-black/50 dark:text-slate-700/80 pointer-events-auto backdrop-blur-md rounded-full",
        disabled ? "opacity-35 cursor-not-allowed" : "active:translate-y-0",
        className,
      )}
    >
      <span className="hidden group-hover:flex relative items-center justify-center text-xs">{children}</span>
    </button>
  );
};

function Window() {
  useDisableNativeContextMenu();
  useSystemTheme(); // Listen for system theme changes
  const { setParentKeyTrap, setVideoToken, videoToken, streamDimensions } = useSharingContext();
  const [livekitUrl, setLivekitUrl] = useState<string>("");
  const previousSizeRef = useRef<{ width: number; height: number; x: number; y: number } | null>(null);
  const [isMaximized, setIsMaximized] = useState(false);
  const isProgrammaticResizeRef = useRef(false);

  useEffect(() => {
    const videoTokenFromUrl = tauriUtils.getTokenParam("videoToken");

    if (videoTokenFromUrl) {
      setVideoToken(videoTokenFromUrl);
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

  // Detect manual window resizing and reset isMaximized state
  const handleWindowResize = useCallback(() => {
    // Ignore resize events during programmatic resizing
    if (isProgrammaticResizeRef.current) {
      return;
    }

    // If window is marked as maximized but user manually resized, reset the state
    if (isMaximized) {
      setIsMaximized(false);
      previousSizeRef.current = null;
    }
  }, [isMaximized]);

  useResizeListener(handleWindowResize);

  const handleClose = useCallback(() => {
    appWindow.close();
  }, []);

  const handleMinimize = useCallback(async () => {
    const minimized = await appWindow.isMinimized();
    if (minimized) {
      await appWindow.show();
      await appWindow.unminimize();
      await appWindow.setFocus();
    } else {
      await appWindow.minimize();
    }
  }, []);

  const handleFullscreen = useCallback(async () => {
    if (!streamDimensions) {
      return;
    }

    if (!isMaximized) {
      isProgrammaticResizeRef.current = true;
      const size = await appWindow.innerSize();
      const position = await appWindow.innerPosition();
      previousSizeRef.current = {
        width: size.width,
        height: size.height,
        x: position.x,
        y: position.y,
      };

      // Get monitor info for centering
      const monitor = await currentMonitor();
      if (!monitor) {
        await setWindowToMaxStreamSize(streamDimensions.width, streamDimensions.height);
        setIsMaximized(true);
        isProgrammaticResizeRef.current = false;
        return;
      }

      // Calculate window size using 92% of screen height
      const factor = await appWindow.scaleFactor();
      const streamExtraOffset = 50 * factor;
      const aspectRatio = streamDimensions.width / streamDimensions.height;

      // Use 92% of monitor height
      const maxHeight = Math.floor(monitor.size.height * 0.87);
      const maxWidth = Math.floor(monitor.size.width);

      // Calculate width based on aspect ratio, ensuring it fits within screen bounds
      let finalWidth: number;
      let finalHeight: number;

      if (maxHeight * aspectRatio <= maxWidth) {
        // Height is the limiting factor
        finalHeight = Math.floor(maxHeight + streamExtraOffset);
        finalWidth = Math.floor(maxHeight * aspectRatio);
      } else {
        // Width is the limiting factor
        finalWidth = maxWidth;
        finalHeight = Math.floor(maxWidth / aspectRatio + streamExtraOffset);
      }

      // Set window size
      await appWindow.setSize(new PhysicalSize(finalWidth, finalHeight));

      // Center the window on the monitor
      const centerX = Math.floor((monitor.size.width - finalWidth) / 2) + monitor.position.x;
      const centerY = Math.floor((monitor.size.height - finalHeight) / 2) + monitor.position.y;
      await appWindow.setPosition(new PhysicalPosition(centerX, centerY));

      setIsMaximized(true);
      // Reset flag after a short delay to allow resize event to fire
      setTimeout(() => {
        isProgrammaticResizeRef.current = false;
      }, 100);
    } else if (previousSizeRef.current) {
      isProgrammaticResizeRef.current = true;
      await appWindow.setSize(new PhysicalSize(previousSizeRef.current.width, previousSizeRef.current.height));
      await appWindow.setPosition(new PhysicalPosition(previousSizeRef.current.x, previousSizeRef.current.y));
      previousSizeRef.current = null;
      setIsMaximized(false);
      // Reset flag after a short delay to allow resize event to fire
      setTimeout(() => {
        isProgrammaticResizeRef.current = false;
      }, 100);
    }
  }, [streamDimensions, isMaximized]);

  const hasAutoMaximizedRef = useRef(false);
  useEffect(() => {
    if (streamDimensions && !isMaximized && !hasAutoMaximizedRef.current) {
      hasAutoMaximizedRef.current = true;
      handleFullscreen();
    }
  }, [streamDimensions, isMaximized, handleFullscreen]);

  const fullscreenDisabled = !streamDimensions;

  return (
    <div
      className="h-full w-full bg-[#ECECEC] dark:bg-[#323232] text-black dark:text-white rounded-[12px] shadow-[0_18px_35px_rgba(0,0,0,0.45)] overflow-hidden group"
      tabIndex={0}
      ref={(ref) => ref && setParentKeyTrap(ref)}
    >
      <Toaster position="bottom-center" />
      <div
        data-tauri-drag-region
        className="title-panel grid grid-cols-[1fr_auto_1fr] items-center h-[40px] px-3 titlebar w-full border-b border-slate-700/20 dark:border-slate-200/20"
      >
        <div className="flex items-center justify-start gap-2" data-tauri-drag-region="no-drag">
          <TitlebarButton
            onClick={handleClose}
            label="Close window"
            className="group-hover:bg-red-500 dark:group-hover:bg-red-500"
          >
            <LuX className="size-[10px] stroke-[3px]" />
          </TitlebarButton>
          <TitlebarButton
            onClick={handleMinimize}
            label="Minimize window"
            className="group-hover:bg-yellow-500 dark:group-hover:bg-yellow-500"
          >
            <LuMinus className="size-[10px] stroke-[3px]" />
          </TitlebarButton>
          <TitlebarButton
            onClick={fullscreenDisabled ? undefined : handleFullscreen}
            disabled={fullscreenDisabled}
            label={isMaximized ? "Restore window size" : "Fit window to stream"}
            className="group-hover:bg-green-500 dark:group-hover:bg-green-500"
          >
            {isMaximized ?
              <LuMinimize2 className="size-[10px] stroke-[3px]" />
            : <LuMaximize2 className="size-[10px] stroke-[3px]" />}
          </TitlebarButton>
        </div>
        <div data-tauri-drag-region="no-drag" className="flex items-center justify-center pointer-events-none">
          <ScreenSharingControls className="pt-0" />
        </div>
        <div className="flex items-center justify-end gap-2" data-tauri-drag-region="no-drag">
          <RemoteControlDisabledIndicator />
          <DrawingSettingsButton />
        </div>
      </div>
      <div className="content px-1 pb-0.5 pt-[10px] overflow-hidden">
        {videoToken && <SharingScreen serverURL={livekitUrl} token={videoToken} />}
      </div>
    </div>
  );
}
