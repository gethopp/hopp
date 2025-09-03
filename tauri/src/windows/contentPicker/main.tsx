import "@/services/sentry";
import "../../App.css";
import React, { useCallback, useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke, isTauri } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { AspectRatio } from "@/components/ui/aspect-ratio";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { toast, Toaster } from "react-hot-toast";
import useStore, { ParticipantRole } from "@/store/store";
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { HiOutlineExclamationCircle } from "react-icons/hi2";
import { useDisableNativeContextMenu } from "@/lib/hooks";
import { tauriUtils } from "../window-utils";

const appWindow = getCurrentWebviewWindow();

type ResolutionKey = "1080p" | "2K" | "1440p" | "2160p" | "4K";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Window />
  </React.StrictMode>,
);

interface CaptureContent {
  content: {
    content_type: "Display" | { Window: { display_id: number } };
    id: number;
  };
  base64: string;
  title: string;
}

async function getContent(setContent: React.Dispatch<React.SetStateAction<CaptureContent[]>>) {
  const message: CaptureContent[] = await invoke("get_available_content");
  console.log(message);
  setContent(message);
}

async function screenshare(content: CaptureContent["content"], resolution: ResolutionKey, videoToken: string) {
  const resolutionMap: Record<ResolutionKey, { width: number; height: number }> = {
    "1080p": { width: 1920, height: 1080 },
    "2K": { width: 2048, height: 1080 },
    "1440p": { width: 2560, height: 1440 },
    "2160p": { width: 3840, height: 2160 },
    "4K": { width: 4096, height: 2160 },
  };

  const message: boolean = await invoke("screenshare", {
    content: content,
    token: videoToken,
    resolution: resolutionMap[resolution],
  });
  return message;
}

function Window() {
  useDisableNativeContextMenu();
  const [content, setContent] = useState<CaptureContent[]>([]);
  const [hasFetched, setHasFetched] = useState(false);
  const [hasEmptyContentFromBackend, setHasEmptyContentFromBackend] = useState(false);
  const videoToken = tauriUtils.getVideoTokenParam();

  useEffect(() => {
    if (!hasFetched) {
      getContent((newContent) => {
        setContent(newContent);
        setHasEmptyContentFromBackend(newContent.length === 0);
      });
      setHasFetched(true);
    }
  }, [hasFetched]);

  const handleItemClick = async (content: CaptureContent["content"]) => {
    // TODO make this faster
    try {
      if (videoToken == null || videoToken == "") {
        toast.error("No video token found");
        return;
      }
      const success = await screenshare(content, resolution, videoToken);
      if (success) {
        await appWindow.close();
      } else {
        toast.error(
          (t) => (
            <div className="flex flex-row items-center gap-2">
              Screenshare failed
              <Button variant="default" className="ml-4" size="sm" onClick={() => toast.dismiss(t.id)}>
                Dismiss
              </Button>
            </div>
          ),
          { duration: 5000 },
        );
        return;
      }
    } catch (error) {
      console.error(error);
      toast.error("Failed to screenshare");
    }
  };

  const [resolution, setResolution] = useState<ResolutionKey>("1440p");
  const updateResolution = (value: string) => {
    setResolution(value as ResolutionKey);
  };

  return (
    <div className="h-full overflow-hidden dark" tabIndex={0}>
      <Toaster position="top-center" />
      <div
        data-tauri-drag-region
        className="title-panel h-[28px] top-0 left-0 titlebar w-full bg-slate-900 flex flex-row justify-end pr-4"
      ></div>
      <div className="flex flex-col items-start gap-2 px-4 py-2 mt-2">
        <span className="mr-2 small">Choose resolution:</span>
        <Select onValueChange={updateResolution} value={resolution}>
          <SelectTrigger className="w-[180px]">
            <SelectValue placeholder="Select resolution" />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="1080p">1080p</SelectItem>
            <SelectItem value="2K">2K</SelectItem>
            <SelectItem value="1440p">1440p</SelectItem>
            <SelectItem value="2160p">2160p</SelectItem>
            <SelectItem value="4K">4K</SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div className="content px-4 pb-4 pt-[10px] overflow-auto grid grid-cols-2 gap-4">
        {hasEmptyContentFromBackend ?
          <div className="col-span-2 flex justify-center">
            <Alert variant="destructive" className="w-full max-w-md">
              <HiOutlineExclamationCircle className="h-4 w-4" />
              <AlertTitle>No Content Available</AlertTitle>
              <AlertDescription>
                No screens or windows are available for sharing. Please make sure you have granted screen recording
                permissions and have content open to share.
              </AlertDescription>
            </Alert>
          </div>
        : content.map((item) => (
            <div
              key={item.content.id}
              className="flex flex-col group items-start gap-3 cursor-pointer transition-all duration-300 hover:bg-slate-500 p-2 rounded-md"
              onClick={() => handleItemClick(item.content)}
            >
              <AspectRatio ratio={16 / 9}>
                <img
                  src={item.base64}
                  alt={`Content ${item.content.id}`}
                  className="w-full max-h-full object-contain rounded-md group-hover:scale-[100.5%] transition-all duration-300 overflow-hidden bg-slate-600 bg-opacity-40"
                />
              </AspectRatio>
              <span className="text-center small ml-0.5">{`${item.title}`}</span>
            </div>
          ))
        }
      </div>
    </div>
  );
}
