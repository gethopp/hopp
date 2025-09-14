import "@/services/sentry";
import "../../App.css";
import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { Button } from "@/components/ui/button";
import { useDisableNativeContextMenu } from "@/lib/hooks";
import trayGif from "@/assets/tray.gif";
import { tauriUtils } from "../window-utils";

const appWindow = getCurrentWebviewWindow();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <InstructionalWindow />
  </React.StrictMode>,
);

function InstructionalWindow() {
  useDisableNativeContextMenu();

  const handleClose = async () => {
    await tauriUtils.hideTrayIconInstruction();
    await appWindow.close();
  };

  return (
    <div className="h-full w-full overflow-hidden dark bg-slate-900 rounded-lg shadow-xl" tabIndex={0}>
      <div
        data-tauri-drag-region
        className="title-panel h-[32px] top-0 left-0 titlebar w-full bg-slate-900 flex flex-row justify-end pr-4"
      />
      <div className="flex flex-col gap-2 p-4">
        <h4 className="h4 text-left text-white">Hopp lives in your menubar</h4>

        <div className="flex flex-row gap-3 items-center bg-slate-800 rounded-md">
          <div className="w-40 h-32 bg-slate-700 bg-opacity-60 rounded-sm flex items-center justify-center shrink-0">
            <img src={trayGif} alt="Click tray icon" className="w-full h-full object-contain rounded-sm" />
          </div>

          <div className="flex-1 text-center">
            <p className="text-white text-s font-medium mb-3">Click the tray icon to open</p>
            <Button variant="default" onClick={handleClose} className="text-xs h-7 px-3">
              Don't show this again
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
