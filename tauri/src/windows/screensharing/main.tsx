import "@/services/sentry";
import "../../App.css";
import React, { useCallback, useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { SharingScreen } from "@/components/SharingScreen/SharingScreen";
import { SharingProvider, useSharingContext } from "./context";
import { ScreenSharingControls } from "@/components/SharingScreen/Controls";
import { Toaster } from "react-hot-toast";
import { useDisableNativeContextMenu } from "@/lib/hooks";
import { cn } from "@/lib/utils";
import { tauriUtils } from "../window-utils";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { PhysicalSize } from "@tauri-apps/api/window";
import { setWindowToMaxStreamSize } from "@/components/SharingScreen/utils";
import { LuMaximize2, LuMinus, LuX } from "react-icons/lu";

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
}: {
  onClick?: () => void;
  disabled?: boolean;
  children: React.ReactNode;
  label: string;
}) => {
  return (
    <button
      type="button"
      data-tauri-drag-region="no-drag"
      onClick={disabled ? undefined : onClick}
      disabled={disabled}
      aria-label={label}
      className={cn(
        "group relative w-[26px] h-[26px] rounded-md border border-white/20 bg-white/10 text-white pointer-events-auto shadow-[0_3px_10px_rgba(0,0,0,0.35)] backdrop-blur-md transition-all",
        disabled ?
          "opacity-35 cursor-not-allowed"
        : "hover:-translate-y-[0.5px] hover:border-white/40 hover:bg-white/20 active:translate-y-0",
      )}
    >
      <span className="absolute inset-[1px] rounded-[7px] bg-white/15 opacity-0 group-hover:opacity-100 transition-opacity" />
      <span className="relative flex items-center justify-center text-xs">{children}</span>
    </button>
  );
};

function Window() {
  useDisableNativeContextMenu();
  const { setParentKeyTrap, setVideoToken, videoToken, streamDimensions } = useSharingContext();
  const [livekitUrl, setLivekitUrl] = useState<string>("");
  const previousSizeRef = useRef<{ width: number; height: number } | null>(null);
  const [isMaximized, setIsMaximized] = useState(false);

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

    tauriUtils.styleScreenshareWindow();
  }, []);

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
      const size = await appWindow.innerSize();
      previousSizeRef.current = { width: size.width, height: size.height };
      await setWindowToMaxStreamSize(streamDimensions.width, streamDimensions.height);
      setIsMaximized(true);
    } else if (previousSizeRef.current) {
      await appWindow.setSize(new PhysicalSize(previousSizeRef.current.width, previousSizeRef.current.height));
      previousSizeRef.current = null;
      setIsMaximized(false);
    }
  }, [streamDimensions, isMaximized]);

  const fullscreenDisabled = !streamDimensions;

  return (
    <div
      className="h-full w-full bg-slate-900 text-white rounded-[18px] border border-slate-800/80 shadow-[0_18px_35px_rgba(0,0,0,0.45)] overflow-hidden"
      tabIndex={0}
      ref={(ref) => ref && setParentKeyTrap(ref)}
    >
      <Toaster position="bottom-center" />
      <div
        data-tauri-drag-region
        className="title-panel flex items-center h-[40px] px-3 titlebar w-full bg-slate-900/95 border-b border-slate-800"
      >
        <div className="flex items-center gap-2 min-w-[120px]" data-tauri-drag-region="no-drag">
          <TitlebarButton onClick={handleClose} label="Close window">
            <LuX className="w-4 h-4" />
          </TitlebarButton>
          <TitlebarButton onClick={handleMinimize} label="Minimize window">
            <LuMinus className="w-4 h-4" />
          </TitlebarButton>
          <TitlebarButton
            onClick={fullscreenDisabled ? undefined : handleFullscreen}
            disabled={fullscreenDisabled}
            label="Fit window to stream"
          >
            <LuMaximize2 className="w-4 h-4" />
          </TitlebarButton>
        </div>
        <div data-tauri-drag-region="no-drag" className="flex-1 flex justify-center pointer-events-none">
          <ScreenSharingControls className="pt-0" />
        </div>
        <div className="min-w-[120px]" aria-hidden="true" />
      </div>
      <div className="content px-1 pb-0.5 pt-[10px] overflow-hidden">
        {videoToken && <SharingScreen serverURL={livekitUrl} token={videoToken} />}
      </div>
    </div>
  );
}
