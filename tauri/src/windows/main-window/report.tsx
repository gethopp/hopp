import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { invoke } from "@tauri-apps/api/core";
import { open as openInBrowser } from "@tauri-apps/plugin-shell";
import { platform, version } from "@tauri-apps/plugin-os";
import { appVersion } from "@/windows/window-utils.ts";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { toast } from "react-hot-toast";
import { readTextFile } from "@tauri-apps/plugin-fs";
import { Clipboard } from "lucide-react";
import failGif from "@/assets/fail.gif";

const OWNER = "gethopp";
const REPO = "hopp";
const TEMPLATE = "bug_report.yml";

const getLogs = async (): Promise<string> => {
  try {
    const logPath = await invoke<string | null>("get_logs");
    if (!logPath) {
      return "Log file path could not be found";
    }
    const logContent = await readTextFile(logPath);
    if (!logContent.trim()) {
      return "The log file is empty";
    }
    return logContent;
  } catch (error) {
    return `An error occurred while trying to read the log file: ${error}`;
  }
};

const deactivateHiding = async (value: boolean) => {
  await invoke("set_deactivate_hiding", { deactivate: value });
};

export function Report() {
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [isCopying, setIsCopying] = useState(false);

  useEffect(() => {
    deactivateHiding(true);
    return () => {
      deactivateHiding(false);
    };
  }, []);

  const handleCopyLogs = async () => {
    setIsCopying(true);
    try {
      const logs = await getLogs();
      await writeText(logs);
      toast.success("App logs copied to clipboard!");
    } catch (error) {
      toast.error("Could not copy logs.");
    } finally {
      setIsCopying(false);
    }
  };

  const handleOpenTemplate = async () => {
    setIsSubmitting(true);
    try {
      const osPlatform = await platform();
      const osVersion = await version();

      const base = `https://github.com/${OWNER}/${REPO}/issues/new`;
      const params = new URLSearchParams({
        template: TEMPLATE,
        os: `${osPlatform} ${osVersion}`,
        ver: `${appVersion}`,
      });
      const url = `${base}?${params.toString()}`;

      try {
        await openInBrowser(url);
      } catch {
        window.open(url, "_blank", "noopener");
      }
    } catch (error) {
      console.error("Failed to open bug report form:", error);
      toast.error("Could not open bug report form.");
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <div className="flex flex-col p-6 max-w-2xl mx-auto">
      <h1 className="text-2xl font-semibold mb-2">Report an Issue</h1>
      <div className="mt-2 mb-4 flex justify-center">
        <img src={failGif} alt="Funny fail GIF" className="max-w-full h-auto rounded-lg" />
      </div>
      <p className="text-sm text-muted-foreground mb-4">
        This opens a GitHub bug report form in your browser. Use the clipboard icon to copy logs.
      </p>

      <div className="flex w-full items-center space-x-2">
        <Button onClick={handleOpenTemplate} className="w-full" disabled={isSubmitting}>
          {isSubmitting ? "Opening GitHub…" : "Open Bug Report Form"}
        </Button>
        <Button className="shrink-0" onClick={handleCopyLogs} variant="outline" size="icon" disabled={isCopying}>
          <Clipboard className="size-4" />
        </Button>
      </div>

      <div className="mt-4 text-xs text-muted-foreground">
        Learn more in the official project{" "}
        <a href="https://docs.gethopp.app/" target="_blank" rel="noreferrer" className="underline hover-no-underline">
          documentation
        </a>
        .
      </div>
    </div>
  );
}

export default Report;
