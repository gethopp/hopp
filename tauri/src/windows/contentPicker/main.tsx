import "@/services/sentry";
import "../../App.css";
import React, { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { AspectRatio } from "@/components/ui/aspect-ratio";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { toast, Toaster } from "react-hot-toast";
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { HiOutlineExclamationCircle } from "react-icons/hi2";
import { useDisableNativeContextMenu, useSystemTheme } from "@/lib/hooks";
import { tauriUtils } from "../window-utils";
import { CgSpinner } from "react-icons/cg";
import clsx from "clsx";

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

async function screenshare(
  content: CaptureContent["content"],
  resolution: ResolutionKey,
  accessibilityPermission: boolean,
) {
  const resolutionMap: Record<ResolutionKey, { width: number; height: number }> = {
    "1080p": { width: 1920, height: 1080 },
    "2K": { width: 2048, height: 1080 },
    "1440p": { width: 2560, height: 1440 },
    "2160p": { width: 3840, height: 2160 },
    "4K": { width: 4096, height: 2160 },
  };

  await invoke("screenshare", {
    content: content,
    resolution: resolutionMap[resolution],
    accessibilityPermission: accessibilityPermission,
  });
  return true;
}

const COLS = 2;

function Window() {
  useDisableNativeContextMenu();
  useSystemTheme();
  const [content, setContent] = useState<CaptureContent[]>([]);
  const [hasFetched, setHasFetched] = useState(false);
  const [hasEmptyContentFromBackend, setHasEmptyContentFromBackend] = useState(false);
  const [accessibilityPermission, setAccessibilityPermission] = useState(false);
  const hasClickedRef = useRef(false);
  const [hasClicked, setHasClicked] = useState(false);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const [usingKeyboard, setUsingKeyboard] = useState(false);
  const itemRefs = useRef<(HTMLDivElement | null)[]>([]);

  const fetchAccessibilityPermission = async () => {
    const permission = await tauriUtils.getControlPermission();
    setAccessibilityPermission(permission);
  };

  useEffect(() => {
    if (!hasFetched) {
      getContent((newContent) => {
        setContent(newContent);
        setHasEmptyContentFromBackend(newContent.length === 0);
      });
      setHasFetched(true);
    }

    fetchAccessibilityPermission();
  }, [hasFetched]);

  useEffect(() => {
    itemRefs.current = itemRefs.current.slice(0, content.length);
    if (content.length > 0 && !hasClicked) {
      setSelectedIndex(0);
      itemRefs.current[0]?.focus();
    }
  }, [content.length, hasClicked]);

  useEffect(() => {
    itemRefs.current[selectedIndex]?.focus();
  }, [selectedIndex]);

  const handleItemClick = async (content: CaptureContent["content"]) => {
    // TODO make this faster
    try {
      if (hasClickedRef.current) {
        return;
      }
      hasClickedRef.current = true;
      setHasClicked(true);
      const success = await screenshare(content, resolution, accessibilityPermission);
      if (success) {
        await appWindow.close();
      }
    } catch (error) {
      console.error(error);
      tauriUtils.showWindow("contentPicker");
      const errorMessage = typeof error === "string" ? error : "Failed to screenshare";
      toast.error(
        (t) => (
          <div className="flex flex-row items-center gap-2">
            <div className="text-sm">{errorMessage}</div>
            <Button size="sm" onClick={() => toast.dismiss(t.id)}>
              Dismiss
            </Button>
          </div>
        ),
        { duration: 10000 },
      );
    } finally {
      hasClickedRef.current = false;
      setHasClicked(false);
    }
  };

  const [resolution, setResolution] = useState<ResolutionKey>("4K");
  const updateResolution = (value: string) => {
    setResolution(value as ResolutionKey);
  };

  const handleGridKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (hasClicked || hasEmptyContentFromBackend || content.length === 0) return;

    switch (e.key) {
      case "ArrowRight":
        e.preventDefault();
        setUsingKeyboard(true);
        setSelectedIndex((i) => Math.min(i + 1, content.length - 1));
        break;
      case "ArrowLeft":
        e.preventDefault();
        setUsingKeyboard(true);
        setSelectedIndex((i) => Math.max(i - 1, 0));
        break;
      case "ArrowDown":
        e.preventDefault();
        setUsingKeyboard(true);
        setSelectedIndex((i) => Math.min(i + COLS, content.length - 1));
        break;
      case "ArrowUp":
        e.preventDefault();
        setUsingKeyboard(true);
        setSelectedIndex((i) => Math.max(i - COLS, 0));
        break;
      case "Enter": {
        e.preventDefault();
        const selected = content[selectedIndex];
        if (selected) {
          handleItemClick(selected.content);
        }
        break;
      }
    }
  };

  const grantAccessibilityPermission = () => {
    tauriUtils.openAccessibilitySettings();

    // Refetch permission status for 5 seconds
    const interval = setInterval(async () => {
      fetchAccessibilityPermission();
    }, 500); // Check every 500ms

    // Stop checking after 5 seconds regardless
    setTimeout(() => {
      clearInterval(interval);
    }, 10000);
  };

  return (
    <div
      className="h-screen overflow-hidden bg-[#ECECEC] dark:bg-[#323232] text-black dark:text-white rounded-[12px] flex flex-col gap-0"
      tabIndex={0}
    >
      <Toaster position="top-center" />
      <div
        data-tauri-drag-region
        className="title-panel h-[28px] top-0 left-0 titlebar w-full bg-transparent flex flex-row justify-end pr-4"
      ></div>
      {!accessibilityPermission && (
        <div className="flex flex-row items-center justify-center gap-2 px-4 py-2 mt-2">
          <span className="text-center text-base font-medium text-yellow-400">
            ⚠️ Accessibility permission is not granted, remote control will not work
          </span>
          <Button size="sm" onClick={grantAccessibilityPermission}>
            Grant permission
          </Button>
        </div>
      )}
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
      <div
        className={clsx("content px-4 pb-4 pt-[10px] overflow-auto gap-4 outline-none", {
          "h-full flex flex-col justify-center": hasClicked,
          "grid grid-cols-2 h-full items-start": !hasClicked,
        })}
        onKeyDown={handleGridKeyDown}
      >
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
        : hasClicked ?
          <div className="h-full w-full flex flex-col justify-center col-span-2">
            <div className="col-span-2 flex flex-row items-center justify-center gap-3">
              <span className="text-base text-black/80 dark:text-white/80">Starting screenshare...</span>
              <CgSpinner className="animate-spin text-black/80 dark:text-white/80 h-6 w-6" />
            </div>
          </div>
        : content.map((item, idx) => (
            <div
              key={item.content.id}
              ref={(el) => {
                itemRefs.current[idx] = el;
              }}
              tabIndex={0}
              className={clsx(
                "flex flex-col group items-start gap-3 cursor-pointer transition-all duration-300 focus:bg-slate-300 dark:focus:bg-slate-500 focus:outline-none p-2 rounded-md",
                !usingKeyboard && "hover:bg-slate-300 dark:hover:bg-slate-500",
              )}
              onClick={() => handleItemClick(item.content)}
              onFocus={() => setSelectedIndex(idx)}
              onMouseEnter={() => {
                if (!usingKeyboard) setSelectedIndex(idx);
              }}
              onMouseMove={() => {
                if (usingKeyboard) {
                  setUsingKeyboard(false);
                  setSelectedIndex(idx);
                }
              }}
            >
              <AspectRatio ratio={16 / 9}>
                <img
                  src={item.base64}
                  alt={`Content ${item.content.id}`}
                  className="w-full max-h-full object-contain rounded-md group-hover:scale-[100.5%] transition-all duration-300 overflow-hidden bg-slate-400/40 dark:bg-slate-600/40"
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
