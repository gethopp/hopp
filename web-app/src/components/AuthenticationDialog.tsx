import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { CustomIcons } from "@/components/ui/icons";
import { toast } from "react-hot-toast";

interface AuthenticationDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  appAuthToken: string | undefined;
}

export function AuthenticationDialog({ open, onOpenChange, appAuthToken }: AuthenticationDialogProps) {
  const handleOpenHopp = () => {
    if (!appAuthToken) {
      toast.error("Token not ready yet. Please wait or copy manually.");
      return;
    }

    try {
      const popupRef = window.open(`hopp:///authenticate?token=${appAuthToken}`, "_blank");
      if (!popupRef) {
        toast.error("Could not open the app. Please try the manual copy option below.");
      } else {
        toast.success("Opening Hopp app...", { duration: 1000 });
        onOpenChange(false);
      }
    } catch {
      toast.error("Could not open the app. Please try the manual copy option below.");
    }
  };

  const handleCopyToken = () => {
    if (appAuthToken) {
      navigator.clipboard.writeText(appAuthToken);
      toast.success("Authentication token copied");
    } else {
      toast.error("Token could not be copied, go to Settings page and copy manually");
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-md">
        <DialogHeader>
          <div className="flex flex-col items-center text-center gap-4">
            <div
              className="bg-primary/10 p-4 outline outline-[0.5px] outline-offset-1 outline-slate-200 rounded-lg"
              style={{
                background: "linear-gradient(180deg, #1E1E1E 0%, #242424 100%)",
                boxShadow: "inset 6px 0px 4px rgba(255, 255, 255, 0.05), inset 0px 1px 2px rgba(255, 255, 255, 0.20)",
              }}
            >
              <CustomIcons.GradientLock className="size-8" />
            </div>
            <DialogTitle className="text-2xl font-semibold text-foreground">Open Hopp app?</DialogTitle>
            <DialogDescription className="text-muted-foreground">
              This will launch the Hopp application on your computer
            </DialogDescription>
          </div>
        </DialogHeader>

        <div className="flex flex-col items-center gap-4">
          <Button size="lg" onClick={handleOpenHopp}>
            Open Hopp
          </Button>
          <p className="text-sm text-gray-500 text-center">This will launch the Hopp application on your computer</p>

          <div className="text-center text-xs text-muted-foreground">
            <p>
              If it doesn't work,{" "}
              <button
                onClick={handleCopyToken}
                className="font-medium text-primary hover:text-primary/80 underline cursor-pointer"
              >
                click copy auth token
              </button>{" "}
              and check{" "}
              <a
                className="font-medium text-primary hover:text-primary/80 underline"
                href="https://translucent-science-2ca.notion.site/How-to-authenticate-application-1f05bf4b0b4d809d8dacf9ee2ebb42f7?pvs=4"
                target="_blank"
                rel="noopener noreferrer"
              >
                our docs on how to manually authenticate the application
              </a>
            </p>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
