import { formatDistanceToNow } from "date-fns";
import { LuMicOff, LuVideo, LuVideoOff, LuScreenShare, LuScreenShareOff, LuWifiOff } from "react-icons/lu";
import { PiScribbleLoopBold } from "react-icons/pi";
import useStore, { CallState, ParticipantRole } from "@/store/store";
import { Separator } from "@/components/ui/separator";
import { ToggleIconButton } from "@/components/ui/toggle-icon-button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
  DropdownMenuCheckboxItem,
} from "@/components/ui/dropdown-menu";
import { useCallback, useEffect, useRef, useState } from "react";
import { Select, SelectContent, SelectItem, SelectTrigger } from "./select";
import { SelectPortal } from "@radix-ui/react-select";
import { Button } from "./button";
import { tauriUtils } from "@/windows/window-utils";
import { HoppAvatar } from "./hopp-avatar";
import { HiOutlineCursorClick, HiOutlineEye } from "react-icons/hi";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { CustomIcons } from "@/components/ui/icons";
import clsx from "clsx";
import { ChevronDownIcon } from "@radix-ui/react-icons";
import { MoreHorizontal } from "lucide-react";
import { HiOutlinePhoneXMark } from "react-icons/hi2";
import toast from "react-hot-toast";
import { useEndCall } from "@/lib/hooks";
import { typedInvoke } from "@/core_payloads";
import { useQuery } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";

const Colors = {
  deactivatedIcon: "text-slate-600",
  deactivatedText: "text-slate-500",
  mic: { text: "text-blue-600", icon: "text-blue-600", ring: "ring-blue-600" },
  camera: { text: "text-green-600", icon: "text-green-600", ring: "ring-green-600" },
  screen: { text: "text-yellow-600", icon: "text-yellow-600", ring: "ring-yellow-600" },
} as const;

export function CallCenter() {
  const { callTokens, teammates } = useStore();
  const callParticipant = teammates?.find((user) => user.id === callTokens?.participant);
  const [controllerCursorState, setControllerCursorState] = useState(true);
  const [accessibilityPermission, setAccessibilityPermission] = useState(true);

  const handleEndCall = useEndCall();
  const handleEndCallRef = useRef(handleEndCall);
  handleEndCallRef.current = handleEndCall;

  // Determine remote participant state from core events
  const remoteParticipantState = callTokens?.participants?.find(
    (p) => callParticipant && p.identity.includes(callParticipant.id),
  );
  const isRemoteDisconnected = remoteParticipantState ? !remoteParticipantState.connected : false;
  const isRemoteMuted = remoteParticipantState?.muted ?? false;

  const fetchAccessibilityPermission = async () => {
    const permission = await tauriUtils.getControlPermission();
    setAccessibilityPermission(permission);
    setControllerCursorState(permission);

    if (callTokens?.role === ParticipantRole.SHARER && (!permission || (permission && !accessibilityPermission))) {
      setTimeout(() => {
        tauriUtils.setControllerCursor(permission);
      }, 2000);
    }
  };

  useEffect(() => {
    fetchAccessibilityPermission();
  }, [callTokens?.role]);

  useEffect(() => {
    const unlisten = listen("core_call_ended", () => {
      console.log("core_call_ended event received");
      const { callTokens } = useStore.getState();
      if (!callTokens) return;
      handleEndCallRef.current();
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const unlisten = listen("core_room_connection_failed", () => {
      console.log("core_call_ended event received");
      const { callTokens } = useStore.getState();
      if (!callTokens) return;
      handleEndCallRef.current();
      toast.error("Failed to establish connection");
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  if (!callTokens) return null;

  return (
    <div className="flex flex-col items-center w-full max-w-sm mx-auto bg-white pt-4 mb-4">
      {/* Reconnecting Banner */}
      {callTokens.isReconnecting && (
        <div className="bg-amber-100 border border-amber-300 rounded-md px-3 py-2 mb-3 flex items-center justify-center gap-2">
          <div className="animate-spin h-4 w-4 border-2 border-amber-600 border-t-transparent rounded-full" />
          <span className="text-xs font-medium text-amber-800">Reconnecting...</span>
        </div>
      )}

      <div className="w-full">
        {/* Initial connecting banner */}
        {callTokens.isInitialisingCall && (
          <div className="mb-4 text-xs font-medium flex flex-row gap-2 items-center justify-center">
            <div className="animate-spin size-3 border-[1.5px] border-slate-400 border-t-transparent rounded-full" />
            Connecting to call
          </div>
        )}

        {/* Call Timer */}
        {!callTokens.isInitialisingCall && (
          <div className="w-full text-center mb-4">
            <span className="text-xs font-medium">Pairing</span>{" "}
            <span className="text-xs muted font-medium">
              started{" "}
              {formatDistanceToNow(callTokens.timeStarted, {
                addSuffix: true,
              })}
            </span>
          </div>
        )}
      </div>
      <div
        className={clsx("gap-2 px-3 flex-nowrap grid mb-4 min-w-full", {
          "grid-cols-6": callTokens?.isRoomCall,
          "grid-cols-12": !callTokens?.isRoomCall,
        })}
      >
        {!callTokens?.isRoomCall && (
          <div className="flex flex-col items-start mb-4 col-span-3 relative">
            <div className="relative mt-1">
              {callParticipant && (
                <>
                  <HoppAvatar
                    src={callParticipant?.avatar_url || undefined}
                    firstName={callParticipant?.first_name}
                    lastName={callParticipant?.last_name}
                    isMuted={isRemoteMuted}
                  />
                  {isRemoteDisconnected && (
                    <div className="absolute -top-1 -right-1 bg-red-500 rounded-full p-1 shadow-md border-2 border-white">
                      <LuWifiOff className="size-3 text-white" />
                    </div>
                  )}
                </>
              )}
            </div>
            <div className="flex flex-col items-start mt-2 w-full">
              <span className="text-sm text-left w-full">{callParticipant?.first_name}</span>
              <span className="text-sm text-left text-slate-500 w-full truncate">{callParticipant?.last_name}</span>
            </div>
          </div>
        )}
        <div className="flex flex-col gap-2 items-center col-span-9">
          <div className="flex flex-row gap-1 w-full">
            <MediaDevicesSettings />
          </div>
          <div className="flex flex-col gap-2 w-full">
            {callTokens?.role === ParticipantRole.CONTROLLER && (
              <Button
                className="w-full border-gray-500 text-gray-600 flex flex-row gap-2"
                variant="gradient-white"
                onClick={() => {
                  typedInvoke("open_screenshare_viewer");
                }}
              >
                <HiOutlineEye className="size-4" />
                Open shared window
              </Button>
            )}
            <div className="w-full flex flex-row gap-2">
              {callTokens?.role === ParticipantRole.SHARER && (
                <TooltipProvider>
                  <Tooltip delayDuration={100}>
                    <TooltipTrigger>
                      <ToggleIconButton
                        onClick={() => {
                          let controllerCursorTmp = !controllerCursorState;
                          tauriUtils.setControllerCursor(controllerCursorTmp);
                          setControllerCursorState(controllerCursorTmp);
                        }}
                        state={
                          controllerCursorState ? "active"
                          : !accessibilityPermission ?
                            "deactivated"
                          : "neutral"
                        }
                        size="unsized"
                        className="size-9"
                        disabled={!accessibilityPermission}
                      >
                        {controllerCursorState && (
                          <HiOutlineCursorClick
                            className={clsx("size-4", {
                              "text-red-800": !controllerCursorState,
                            })}
                          />
                        )}
                        {!controllerCursorState && (
                          <div className="relative">
                            <HiOutlineCursorClick className="size-4 text-red-800" />
                            <span className="absolute bottom-[-8px] right-[-7px] text-[10px]">🔒</span>
                          </div>
                        )}
                      </ToggleIconButton>
                    </TooltipTrigger>
                    <TooltipContent side="bottom">
                      {controllerCursorState ? "Disable" : "Enable"} remote control
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              )}
              {callTokens?.role === ParticipantRole.SHARER && <DrawingEnableButton />}
              <Button
                className="w-full border-red-500 text-red-600 flex flex-row gap-2"
                variant="gradient-white"
                onClick={handleEndCall}
              >
                <HiOutlinePhoneXMark className="size-4" />
                End call
              </Button>
            </div>
          </div>
        </div>
      </div>
      {/* Horizontal line */}
      <Separator className="w-full" />
    </div>
  );
}

function DrawingEnableButton() {
  const [drawingPermanent, setDrawingPermanent] = useState(false);
  const [dropdownOpen, setDropdownOpen] = useState(false);

  // Load drawing permanent preference on mount
  useEffect(() => {
    const loadPreference = async () => {
      try {
        const permanent = await tauriUtils.getSharerDrawPersist();
        setDrawingPermanent(permanent);
      } catch (error) {
        console.error("Failed to load drawing permanent preference:", error);
      }
    };
    loadPreference();
  }, []);

  const handlePermanentToggle = async (checked: boolean) => {
    setDrawingPermanent(checked);
    try {
      await tauriUtils.setSharerDrawPersist(checked);
    } catch (error) {
      console.error("Failed to save drawing permanent preference:", error);
    }
  };

  const handleEnableDrawing = async () => {
    try {
      await tauriUtils.enableDrawing(drawingPermanent);
    } catch (error) {
      console.error("Failed to enable drawing:", error);
      toast.error("Failed to enable drawing", { duration: 2500 });
    }
  };

  return (
    <div className="inline-flex -space-x-px rounded-lg shadow-xs">
      <TooltipProvider>
        <Tooltip delayDuration={100}>
          <TooltipTrigger asChild>
            <Button
              variant="outline"
              size="icon"
              onClick={handleEnableDrawing}
              className="rounded-none first:rounded-l-lg focus:z-10"
            >
              <PiScribbleLoopBold className="size-4" />
            </Button>
          </TooltipTrigger>
          <TooltipContent side="bottom">Enable drawing</TooltipContent>
        </Tooltip>
      </TooltipProvider>
      <DropdownMenu open={dropdownOpen} onOpenChange={setDropdownOpen}>
        <TooltipProvider>
          <Tooltip delayDuration={100}>
            <TooltipTrigger asChild>
              <DropdownMenuTrigger asChild>
                <Button
                  variant="outline"
                  size="icon"
                  className="rounded-none last:rounded-r-lg focus:z-10"
                  aria-label="Drawing options"
                >
                  <MoreHorizontal className="size-4" />
                </Button>
              </DropdownMenuTrigger>
            </TooltipTrigger>
            <TooltipContent side="bottom">Drawing options</TooltipContent>
          </Tooltip>
        </TooltipProvider>
        <DropdownMenuContent
          onCloseAutoFocus={(e) => e.preventDefault()}
          align="start"
          className="w-auto min-w-[200px]"
        >
          <DropdownMenuCheckboxItem checked={drawingPermanent} onCheckedChange={handlePermanentToggle}>
            <span>Persist until right click</span>
          </DropdownMenuCheckboxItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
  );
}

/**
 * MicrophoneIcon — uses typedInvoke to list/select microphones and toggle mute via core.
 * Audio level visualization is disabled (core does not yet send audio levels back).
 */
function MicrophoneIcon() {
  const { updateCallTokens, callTokens } = useStore();
  const hasAudioEnabled = callTokens?.hasAudioEnabled || false;

  const { data: microphoneDevices = [], refetch: refetchMics } = useQuery({
    queryKey: ["list_microphones"],
    enabled: !callTokens?.isInitialisingCall,
    queryFn: async () => typedInvoke("list_microphones"),
    select: (data) => data.sort((a, b) => a.name.localeCompare(b.name)),
  });

  const [activeMicId, setActiveMicId] = useState<string>("");

  useEffect(() => {
    if (!microphoneDevices.length) return;
    if (activeMicId) return;
    const resolve = async () => {
      const lastUsedMic = await tauriUtils.getLastUsedMic();
      if (lastUsedMic && microphoneDevices.find((d) => d.name === lastUsedMic)) {
        setActiveMicId(lastUsedMic);
        return;
      }
      const defaultDevice = microphoneDevices.find((d) => d.default) ?? microphoneDevices[0];
      if (defaultDevice) setActiveMicId(defaultDevice.name);
    };
    resolve();
  }, [microphoneDevices]);

  useEffect(() => {
    const unlisten = listen<string>("core_active_mic_changed", (event) => {
      setActiveMicId(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const handleMicToggle = useCallback(() => {
    const newState = !hasAudioEnabled;
    updateCallTokens({ hasAudioEnabled: newState });
    if (newState) {
      typedInvoke("unmute_mic");
    } else {
      typedInvoke("mute_mic");
    }
  }, [hasAudioEnabled, updateCallTokens]);

  const handleMicrophoneChange = useCallback((value: string) => {
    setActiveMicId(value);
    typedInvoke("select_microphone", { deviceName: value });
    tauriUtils.setLastUsedMic(value);
  }, []);

  const handleDropdownOpenChange = useCallback(
    (open: boolean) => {
      if (open) refetchMics();
    },
    [refetchMics],
  );

  return (
    <ToggleIconButton
      onClick={handleMicToggle}
      icon={
        <div className="relative flex items-center justify-center">
          {hasAudioEnabled ?
            <CustomIcons.MicWithLevel level={0} className={`size-4 ${Colors.mic.icon} relative z-10`} />
          : <LuMicOff className={`size-4 ${Colors.deactivatedIcon} relative z-10`} />}
        </div>
      }
      state={hasAudioEnabled ? "active" : "neutral"}
      size="unsized"
      className={clsx("flex-1 min-w-0", {
        [Colors.deactivatedText]: !hasAudioEnabled,
        [`${Colors.mic.text} ${Colors.mic.ring}`]: hasAudioEnabled,
      })}
      cornerIcon={
        <Select value={activeMicId} onValueChange={handleMicrophoneChange} onOpenChange={handleDropdownOpenChange}>
          <SelectTrigger
            iconClassName={clsx({
              [Colors.mic.text]: hasAudioEnabled,
              [Colors.deactivatedIcon]: !hasAudioEnabled,
            })}
            className="hover:outline-solid hover:outline-1 hover:outline-slate-300 focus:ring-0 focus-visible:ring-0 hover:bg-slate-200 size-4 rounded-xs p-0 border-0 shadow-none hover:shadow-xs"
          />
          <SelectPortal container={document.getElementsByClassName("container")[0]}>
            <SelectContent align="center">
              {microphoneDevices.map(
                (device) =>
                  device.name !== "" && (
                    <SelectItem key={device.name} value={device.name}>
                      <span className="text-xs truncate">{device.name}</span>
                    </SelectItem>
                  ),
              )}
            </SelectContent>
          </SelectPortal>
        </Select>
      }
    >
      {hasAudioEnabled ? "Mute me" : "Open mic"}
    </ToggleIconButton>
  );
}

function ScreenShareIcon({ callTokens }: { callTokens: CallState | null }) {
  const { setCallTokens } = useStore();

  const toggleScreenShare = useCallback(async () => {
    if (!callTokens) return;

    if (callTokens.role === ParticipantRole.NONE || callTokens.role === ParticipantRole.CONTROLLER) {
      tauriUtils.createContentPickerWindow();
    } else if (callTokens.role === ParticipantRole.SHARER) {
      setCallTokens({
        ...callTokens,
        role: ParticipantRole.NONE,
        isRemoteControlEnabled: true,
      });
      tauriUtils.stopSharing();
    }
  }, [callTokens, setCallTokens]);

  const changeScreenShare = useCallback(() => {
    if (!callTokens) return;
    tauriUtils.createContentPickerWindow();
  }, [callTokens]);

  return (
    <ToggleIconButton
      onClick={toggleScreenShare}
      icon={
        callTokens?.role === ParticipantRole.SHARER ?
          <LuScreenShareOff className={`size-4 ${Colors.screen.icon}`} />
        : <LuScreenShare className={`size-4 ${Colors.deactivatedIcon}`} />
      }
      state={callTokens?.role === ParticipantRole.SHARER ? "active" : "neutral"}
      size="unsized"
      className={clsx("flex-1 min-w-0", {
        [Colors.deactivatedText]: !(callTokens?.role === ParticipantRole.SHARER),
        [`${Colors.screen.text} ${Colors.screen.ring}`]: callTokens?.role === ParticipantRole.SHARER,
      })}
      cornerIcon={
        callTokens?.role === ParticipantRole.SHARER && (
          <button
            onClick={changeScreenShare}
            className="hover:outline-solid hover:outline-1 hover:outline-slate-300 focus:ring-0 focus-visible:ring-0 hover:bg-slate-200 size-4 rounded-sm p-0 border-0 shadow-none hover:shadow-xs"
          >
            <ChevronDownIcon
              className={clsx("size-4", {
                [Colors.screen.icon]: callTokens?.role === ParticipantRole.SHARER,
                [Colors.deactivatedIcon]: !(callTokens?.role === ParticipantRole.SHARER),
              })}
            />
          </button>
        )
      }
    >
      {callTokens?.role === ParticipantRole.SHARER ? "Stop sharing" : "Share screen"}
    </ToggleIconButton>
  );
}

function CameraIcon() {
  const { updateCallTokens, callTokens } = useStore();
  const cameraEnabled = callTokens?.hasCameraEnabled || false;

  const { data: cameraDevices = [], refetch: refetchCameras } = useQuery({
    queryKey: ["list_webcams"],
    queryFn: () => typedInvoke("list_webcams"),
    select: (data) => data.sort((a, b) => a.name.localeCompare(b.name)),
  });

  const [activeCamera, setActiveCamera] = useState<string>("");

  useEffect(() => {
    if (!cameraDevices.length) return;
    if (activeCamera) return;
    const resolve = async () => {
      const lastUsedCamera = await tauriUtils.getLastUsedCamera();
      if (lastUsedCamera && cameraDevices.find((d) => d.name === lastUsedCamera)) {
        setActiveCamera(lastUsedCamera);
        return;
      }
      const defaultDevice = cameraDevices.find((d) => d.default) ?? cameraDevices[0];
      if (defaultDevice) setActiveCamera(defaultDevice.name);
    };
    resolve();
  }, [cameraDevices]);

  useEffect(() => {
    const unlisten = listen<string>("core_active_camera_changed", (event) => {
      setActiveCamera(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const handleCameraToggle = useCallback(async () => {
    const newEnabled = !cameraEnabled;
    updateCallTokens({ hasCameraEnabled: newEnabled });

    if (newEnabled) {
      try {
        await typedInvoke("start_camera", { deviceName: activeCamera || undefined });
      } catch (error) {
        console.error("Failed to start camera:", error);
        toast.error("Failed to initialize camera", { duration: 2500 });
        updateCallTokens({ hasCameraEnabled: false });
      }
    } else {
      // TODO(@konsalex): This is an optimistic update, not sure how I feel about this.
      // We may need to make this sync somehow.
      typedInvoke("stop_camera");
    }
  }, [cameraEnabled, updateCallTokens, activeCamera]);

  const handleCameraChange = useCallback(
    async (deviceName: string) => {
      setActiveCamera(deviceName);
      tauriUtils.setLastUsedCamera(deviceName);
      if (cameraEnabled) {
        try {
          await typedInvoke("start_camera", { deviceName });
        } catch (error) {
          console.error("Failed to switch camera:", error);
        }
      }
    },
    [cameraEnabled],
  );

  const handleDropdownOpenChange = useCallback(
    (open: boolean) => {
      if (open) refetchCameras();
    },
    [refetchCameras],
  );

  return (
    <ToggleIconButton
      onClick={handleCameraToggle}
      icon={
        cameraEnabled ?
          <LuVideo className={`size-4 ${Colors.camera.icon}`} />
        : <LuVideoOff className={`size-4 ${Colors.deactivatedIcon}`} />
      }
      state={cameraEnabled ? "active" : "neutral"}
      size="unsized"
      className={clsx("flex-1 min-w-0", {
        [Colors.deactivatedText]: !cameraEnabled,
        [`${Colors.camera.text} ${Colors.camera.ring}`]: cameraEnabled,
      })}
      cornerIcon={
        <Select value={activeCamera} onValueChange={handleCameraChange} onOpenChange={handleDropdownOpenChange}>
          <SelectTrigger
            iconClassName={clsx({
              [Colors.camera.text]: cameraEnabled,
              [Colors.deactivatedIcon]: !cameraEnabled,
            })}
            className="hover:outline hover:outline-slate-300 focus:ring-0 focus-visible:ring-0 hover:bg-slate-200 size-4 rounded-sm p-0 border-0 shadow-none hover:shadow-xs"
          />
          <SelectPortal container={document.getElementsByClassName("container")[0]}>
            <SelectContent align="center">
              {cameraDevices.map(
                (device) =>
                  device.name !== "" && (
                    <SelectItem key={device.name} value={device.name}>
                      <span className="text-xs truncate">{device.name}</span>
                    </SelectItem>
                  ),
              )}
            </SelectContent>
          </SelectPortal>
        </Select>
      }
    >
      {cameraEnabled ? "Stop sharing" : "Share cam"}
    </ToggleIconButton>
  );
}

function MediaDevicesSettings() {
  const { callTokens } = useStore();

  return (
    <div className="flex flex-row gap-1 w-full">
      <MicrophoneIcon />
      <CameraIcon />
      <ScreenShareIcon callTokens={callTokens} />
    </div>
  );
}
