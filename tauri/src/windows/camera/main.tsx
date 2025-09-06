import "@/services/sentry";
import "../../App.css";
import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { Toaster } from "react-hot-toast";
import { useDisableNativeContextMenu } from "@/lib/hooks";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { tauriUtils } from "../window-utils";
import { LiveKitRoom, useTracks, VideoTrack } from "@livekit/components-react";
import { Track } from "livekit-client";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
        <CameraWindow />
    </React.StrictMode>,
);

function ConsumerComponent() {
    const tracks = useTracks([Track.Source.Camera], {
        onlySubscribed: true,
    });

    return (
        <div className="content px-4 py-4">
            <div className="flex items-center justify-center h-full">
                <div className="text-center">
                    <h1 className="text-2xl font-semibold mb-2">Camera Window</h1>
                    {tracks.map((track) => {
                        return <VideoTrack trackRef={track} />
                    })}
                </div>
            </div>
        </div>
    );
}

function CameraWindow() {
    useDisableNativeContextMenu();
    const [cameraToken, setCameraToken] = useState<string | null>(null);

    const [livekitUrl, setLivekitUrl] = useState<string>("");

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
    }, []);

    return (
        <div data-tauri-drag-region className="h-full bg-slate-900 overflow-hidden text-white">
            <Toaster position="bottom-center" />
            <LiveKitRoom token={cameraToken ?? undefined} serverUrl={livekitUrl}>
                <ConsumerComponent />
            </LiveKitRoom>
        </div>
    );
}
