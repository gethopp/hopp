import "@/services/sentry";
import "../../App.css";
import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { SharingScreen } from "@/components/SharingScreen/SharingScreen";
import { SharingProvider, useSharingContext } from "./context";
import { ScreenSharingControls } from "@/components/SharingScreen/Controls";
import { Toaster } from "react-hot-toast";
import { useDisableNativeContextMenu } from "@/lib/hooks";
import { tauriUtils } from "../window-utils";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

const appWindow = getCurrentWebviewWindow();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <SharingProvider>
      <Window />
    </SharingProvider>
  </React.StrictMode>,
);

function Window() {
  useDisableNativeContextMenu();
  const { setParentKeyTrap, setVideoToken, videoToken } = useSharingContext();
  const [livekitUrl, setLivekitUrl] = useState<string>("");
  const [port, setPort] = useState<number>(0);

  useEffect(() => {
    const videoTokenFromUrl = tauriUtils.getVideoTokenParam();

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

    async function getPort() {
      const port = await tauriUtils.getStreamWsPort();
      setPort(port);
    }
    getPort();

    enableDock();
  }, []);

  return (
    <div
      className="h-full bg-slate-900 overflow-hidden text-white"
      tabIndex={0}
      ref={(ref) => ref && setParentKeyTrap(ref)}
    >
      <Toaster position="bottom-center" />
      <div data-tauri-drag-region className="title-panel h-[32px] top-0 left-0 titlebar w-full bg-slate-900 px-4">
        <ScreenSharingControls />
      </div>
      <div className="content px-1 pb-0.5 pt-[10px] overflow-hidden">
        <SharingScreen serverURL={""} token={""} port={port}/>
      </div>
    </div>
  );
}
