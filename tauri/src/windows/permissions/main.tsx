import "@/services/sentry";
import "../../App.css";
import React, { useState, useEffect } from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { toast, Toaster } from "react-hot-toast";
import { Button } from "@/components/ui/button";
import { useDisableNativeContextMenu } from "@/lib/hooks";
import { tauriUtils } from "../window-utils";
import { PiMicrophoneDuotone, PiMouseDuotone, PiMonitorArrowUpDuotone, PiCheckCircleDuotone, PiCameraDuotone } from "react-icons/pi";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Window />
  </React.StrictMode>,
);

interface PermissionStatus {
  mic: boolean;
  screenShare: boolean;
  accessibility: boolean;
  camera: boolean;
}

function Window() {
  useDisableNativeContextMenu();
  const [permissions, setPermissions] = useState<PermissionStatus>({
    mic: false,
    screenShare: false,
    accessibility: false,
    camera: false,
  });
  const [loading, setLoading] = useState<string | null>(null);

  // Get initial permission values from backend
  useEffect(() => {
    const fetchPermissions = async () => {
      const micPermission = await getMicPermission();
      const screenSharePermission = await getScreenSharePermission();
      const accessibilityPermission = await getAccessibilityPermission();
      const cameraPermission = await getCameraPermission();

      setPermissions({
        mic: micPermission,
        screenShare: screenSharePermission,
        accessibility: accessibilityPermission,
        camera: cameraPermission,
      });
    };

    fetchPermissions();
  }, []);

  const getMicPermission = async (): Promise<boolean> => {
    return tauriUtils.getMicPermission();
  };

  const getScreenSharePermission = async (): Promise<boolean> => {
    return tauriUtils.getScreenSharePermission();
  };

  const getAccessibilityPermission = async (): Promise<boolean> => {
    return tauriUtils.getControlPermission();
  };

  const getCameraPermission = async (): Promise<boolean> => {
    return tauriUtils.getCameraPermission();
  };

  const requestMicrophonePermission = async (): Promise<boolean> => {
    try {
      // Request microphone access using browser API
      const stream = await navigator.mediaDevices.getUserMedia({
        audio: true,
      });

      // Stop the microphone stream immediately after getting permission
      stream.getTracks().forEach((track) => {
        track.stop();
      });
      return true;
    } catch (error) {
      console.error("Browser microphone permission denied:", error);
      return false;
    }
  };

  const requestCameraPermission = async (): Promise<boolean> => {
    try {
      // Request camera access using browser API
      const stream = await navigator.mediaDevices.getUserMedia({
        video: true,
      });

      // Stop the camera stream immediately after getting permission
      stream.getTracks().forEach((track) => {
        track.stop();
      });
      return true;
    } catch (error) {
      console.error("Browser camera permission denied:", error);
      return false;
    }
  };

  const handlePermissionRequest = async (permission: keyof PermissionStatus) => {
    setLoading(permission);

    try {
      console.log(`Requesting ${permission} permission...`);

      let success = false;

      switch (permission) {
        case "mic":
          success = await requestMicrophonePermission();
          if (!success) {
            await tauriUtils.openMicrophoneSettings();
          }

          // Keep polling until permission is granted
          let micCheckCount = 0;
          while (!success && micCheckCount < 15) {
            await new Promise((resolve) => setTimeout(resolve, 1000)); // Wait 1 second
            success = await getMicPermission();
            micCheckCount++;
          }
          break;
        case "screenShare":
          success = await tauriUtils.triggerScreenSharePermission();
          if (!success) {
            // We sleep for 2 seconds to allow the user to click the open
            // setting prompt the first time. If the permission has not been granted
            // the prompt won't be shown and this is why we have to open the settings
            // manually.
            await new Promise((resolve) => setTimeout(resolve, 2000));
            await tauriUtils.openScreenShareSettings();
          }

          // Keep polling until permission is granted
          // Instead of checking the permission we do the trigger because on our
          // testing the permission wasn't updated in CGPreflightScreenCaptureAccess
          // immediately and it expected a restart (probably)
          let screenShareCheckCount = 0;
          while (!success && screenShareCheckCount < 5) {
            await new Promise((resolve) => setTimeout(resolve, 3000)); // Wait 3 seconds
            success = await tauriUtils.triggerScreenSharePermission();
            screenShareCheckCount++;
          }
          break;
        case "accessibility":
          await tauriUtils.openAccessibilitySettings();
          // Keep polling until permission is granted
          let accessibilityCheckCount = 0;
          while (!success && accessibilityCheckCount < 15) {
            await new Promise((resolve) => setTimeout(resolve, 1000)); // Wait 1 second
            success = await getAccessibilityPermission();
            accessibilityCheckCount++;
          }
          break;
        case "camera":
          success = await requestCameraPermission();
          if (!success) {
            await tauriUtils.openCameraSettings();
          }

          // Keep polling until permission is granted
          let cameraCheckCount = 0;
          while (!success && cameraCheckCount < 15) {
            await new Promise((resolve) => setTimeout(resolve, 1000)); // Wait 1 second
            success = await getCameraPermission();
            cameraCheckCount++;
          }
          break;
      }

      if (success) {
        setPermissions((prev) => ({
          ...prev,
          [permission]: true,
        }));
        toast.success(`${getPermissionTitle(permission)} granted!`);
      } else {
        toast.error(`Failed to request ${getPermissionTitle(permission)}`);
      }
    } catch (error) {
      console.error(`Failed to request ${permission} permission:`, error);
      toast.error(`Failed to request ${getPermissionTitle(permission)}`);
    } finally {
      setLoading(null);
    }
  };

  const getPermissionIcon = (permission: keyof PermissionStatus) => {
    const icons = {
      mic: <PiMicrophoneDuotone className="size-6" />,
      camera: <PiCameraDuotone className="size-6" />,
      screenShare: <PiMonitorArrowUpDuotone className="size-6" />,
      accessibility: <PiMouseDuotone className="size-6" />,
    };
    return icons[permission];
  };

  const getPermissionTitle = (permission: keyof PermissionStatus) => {
    const titles = {
      mic: "Microphone Access",
      camera: "Camera Access",
      screenShare: "Screen Sharing Access",
      accessibility: "Remote Control Access",
    };
    return titles[permission];
  };

  const getPermissionDescription = (permission: keyof PermissionStatus) => {
    const descriptions = {
      mic: "Allow access to your microphone for your teammates to hear you. If for any reason the system settings open automatically, please go to Privacy & Security, Microphone and allow Hopp to access your microphone.",
      camera: "Allow access to your camera for your teammates to see you. If for any reason the system settings open automatically, please go to Privacy & Security, Camera and allow Hopp to access your camera. If you don't have a camera, you can ignore this permission.",
      screenShare:
        "Allow screen sharing. If for any reason the system settings open automatically, please go to Privacy & Security, Screen & System Audio Recording and allow Hopp to share your screen.",
      accessibility:
        "Allow remote control for a seamless remote pair programming experience. If for any reason the system settings open automatically, please go to Privacy & Security, Accessibility and allow Hopp to control your computer.",
    };
    return descriptions[permission];
  };

  const allPermissionsGranted = Object.values(permissions).every(Boolean);

  // Close window when all permissions are granted (with delay)
  useEffect(() => {
    if (allPermissionsGranted) {
      const timer = setTimeout(async () => {
        try {
          const currentWindow = getCurrentWebviewWindow();
          await currentWindow.close();
        } catch (error) {
          console.error("Failed to close permissions window:", error);
        }
      }, 2000);

      return () => clearTimeout(timer);
    }
  }, [allPermissionsGranted]);

  return (
    <div className="h-full overflow-hidden dark" tabIndex={0}>
      <Toaster position="top-center" />
      <div
        data-tauri-drag-region
        className="title-panel h-[28px] top-0 left-0 titlebar w-full bg-slate-900 flex flex-row justify-end pr-4"
      ></div>

      <div className="flex flex-col items-start gap-4 px-4 py-6 mt-2">
        <div className="w-full">
          <h1 className="h3 text-slate-100">Grant permissions</h1>
          <p className="text-slate-400 text-sm leading-relaxed">
            We need the following permissions to have been granted for Hopp to work properly. You can change these
            settings later in your system preferences.
          </p>
        </div>

        {allPermissionsGranted && (
          <div className="w-full p-3 bg-green-900 bg-opacity-30 border border-green-700 rounded-md">
            <div className="flex items-center gap-2 text-green-400 text-sm font-medium">
              <PiCheckCircleDuotone className="size-4" />
              All permissions granted! You can now use all features of Hopp.
            </div>
          </div>
        )}
      </div>

      <div className="content px-4 pb-4 overflow-auto flex flex-col gap-4">
        {(["mic", "camera", "accessibility", "screenShare"] as Array<keyof PermissionStatus>).map((permission) => (
          <div
            key={permission}
            className="flex items-center justify-between group p-4 rounded-md transition-all duration-300 hover:bg-slate-800 border border-slate-700"
          >
            <div className="flex items-center gap-4 flex-1">
              <div className="w-12 h-12 rounded-full bg-slate-700 flex items-center justify-center text-2xl">
                {getPermissionIcon(permission)}
              </div>

              <div className="flex-1">
                <h4 className="h4 font-semibold text-slate-100 mb-1">{getPermissionTitle(permission)}</h4>
                <p className="text-sm mt-0 text-slate-400 leading-relaxed">{getPermissionDescription(permission)}</p>
              </div>
            </div>
            <div className="ml-4">
              {permissions[permission] ?
                <div className="flex flex-row items-center gap-1 text-green-400 font-medium text-md">
                  <PiCheckCircleDuotone className="size-4" />
                  <span>Granted</span>
                </div>
                : <Button
                  onClick={() => handlePermissionRequest(permission)}
                  disabled={loading === permission}
                  variant="default"
                  size="sm"
                  className="transition-all duration-300"
                >
                  {loading === permission ? "Waiting for permission..." : "Request Permission"}
                </Button>
              }
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
