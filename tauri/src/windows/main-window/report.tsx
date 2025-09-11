import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { invoke } from "@tauri-apps/api/core";
import { open as openInBrowser } from "@tauri-apps/plugin-shell";

const OWNER = "gethopp";
const REPO = "hopp";
const TEMPLATE = "bug_report.yml";

const deactivateHiding = async (value: boolean) => {
  await invoke("set_deactivate_hiding", { deactivate: value });
};

export function Report() {
  const [isSubmitting, setIsSubmitting] = useState(false);

  useEffect(() => {
    deactivateHiding(true);
    return () => {
      deactivateHiding(false);
    };
  }, []);

  const handleOpenTemplate = async () => {
    setIsSubmitting(true);
    try {
      const base = `https://github.com/${OWNER}/${REPO}/issues/new`;
      const url = `${base}?template=${encodeURIComponent(TEMPLATE)}`;

      try {
        await openInBrowser(url);
      } catch {
        window.open(url, "_blank", "noopener");
      }
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <div className="flex flex-col p-6 max-w-2xl mx-auto">
      <h1 className="text-2xl font-semibold mb-2">Report an Issue</h1>
      <p className="text-sm text-muted-foreground mb-6">
        This opens a GitHub bug report form in your browser. Fill it out and attach any files there.
      </p>

      <Button onClick={handleOpenTemplate} className="w-full" disabled={isSubmitting}>
        {isSubmitting ? "Opening GitHub…" : "Open Bug Report Form"}
      </Button>

      <div className="mt-4 text-xs text-muted-foreground">
        Learn more in the official project{" "}
        <a href="https://docs.gethopp.app/" target="_blank" rel="noreferrer" className="underline hover:no-underline">
          documentation
        </a>
        .
      </div>
    </div>
  );
}

export default Report;
