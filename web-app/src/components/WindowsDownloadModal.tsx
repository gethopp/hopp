import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
  DialogFooter,
  DialogClose,
} from "@/components/ui/dialog";
import windowsSmartVideo from "../assets/wmv3.mp4";

type Props = {
  onDownload: () => void;
  disabled?: boolean;
  triggerClassName?: string;
};
// allows download and modal popup at same time
export function WindowsDownloadDialog({ onDownload, disabled, triggerClassName }: Props) {
  return (
    <Dialog>
      <DialogTrigger asChild>
        {/* Dashboard Button  */}
        <Button variant="outline" className={triggerClassName} disabled={disabled} onClick={onDownload}>
          Download for Windows
        </Button>
      </DialogTrigger>

      {/* Modal  */}

      <DialogContent className="sm:max-w-[660px]">
        <DialogHeader>
          <DialogTitle>Download started</DialogTitle>
          <DialogDescription>
            The Hopp Installer is downloading in the background. If the SmartScreen warning appears, follow the steps
            below.
          </DialogDescription>
        </DialogHeader>

        <div className="grid gap-4 py-4">
          <div className="flex justify-center">
            <video
              src={windowsSmartVideo}
              className="w-[80%] rounded-lg border border-gray-400 shadow-sm object-contain"
              autoPlay
              loop
              muted
              playsInline
            />
          </div>

          <ol className="list-decimal space-y-1 pl-5 text-sm">
            <li>
              When “Windows protected your PC” appears, click <b>More info</b>.
            </li>
            <li>
              Click <b>Run anyway</b>.
            </li>
            <li>Follow additional prompts to start the installation.</li>
          </ol>
        </div>

        <DialogFooter>
          <DialogClose asChild>
            <Button variant="outline">Close</Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
