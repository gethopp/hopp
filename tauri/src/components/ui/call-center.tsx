import { formatDistanceToNow } from "date-fns";
import { LuMicOff, LuVideo, LuVideoOff, LuScreenShare, LuScreenShareOff } from "react-icons/lu";
import { PiScribbleLoopBold, PiCursorBold } from "react-icons/pi";
import useStore, { CallState, ParticipantRole } from "@/store/store";
import { Separator } from "@/components/ui/separator";
import { ToggleIconButton } from "@/components/ui/toggle-icon-button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuTrigger,
  DropdownMenuCheckboxItem,
} from "@/components/ui/dropdown-menu";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Select, SelectContent, SelectItem, SelectTrigger } from "./select";
import { SelectPortal } from "@radix-ui/react-select";
import { Button } from "./button";
import { tauriUtils } from "@/windows/window-utils";
import { HiOutlineCursorClick, HiOutlineEye } from "react-icons/hi";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { CustomIcons } from "@/components/ui/icons";
import clsx from "clsx";
import { ChevronDownIcon } from "@radix-ui/react-icons";
import { MoreHorizontal, X } from "lucide-react";
import { HiOutlinePhoneXMark, HiMiniLink } from "react-icons/hi2";
import toast from "react-hot-toast";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Constants } from "@/constants";
import { useEndCall } from "@/lib/hooks";
import { typedInvoke } from "@/core_payloads";
import { useQuery } from "@tanstack/react-query";
import { listen } from "@tauri-apps/api/event";
import { sounds } from "@/constants/sounds";

const Colors = {
  deactivatedIcon: "text-slate-600",
  deactivatedText: "text-slate-500",
  mic: { text: "text-blue-600", icon: "text-blue-600", ring: "ring-blue-600" },
  camera: { text: "text-green-600", icon: "text-green-600", ring: "ring-green-600" },
  screen: { text: "text-yellow-600", icon: "text-yellow-600", ring: "ring-yellow-600" },
} as const;

export function CallCenter() {
  const { callTokens, teammates } = useStore();
  const [controllerCursorState, setControllerCursorState] = useState(true);
  const [accessibilityPermission, setAccessibilityPermission] = useState(true);

  const { data: userSettings } = useQuery({
    queryKey: ["user-settings"],
    queryFn: () => typedInvoke("get_user_settings"),
    refetchOnWindowFocus: true,
  });

  const handleEndCall = useEndCall();
  const handleEndCallRef = useRef(handleEndCall);
  handleEndCallRef.current = handleEndCall;

  const handleCopyRoomLink = async () => {
    if (!callTokens?.room) return;
    const roomLink = `${Constants.webAppUrl}/room/${callTokens.room.id}`;
    await writeText(roomLink);
    toast.success("Room link copied to clipboard");
  };

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
        {!callTokens.isInitialisingCall &&
          (callTokens.room ?
            <div className="w-full flex flex-col items-center gap-0.5 mb-4">
              <div className="flex items-center justify-center gap-1.5">
                <span className="text-xs font-medium">{callTokens.room.name}</span>
                <TooltipProvider delayDuration={100}>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        onClick={handleCopyRoomLink}
                        className="flex items-center justify-center size-5 rounded-sm text-slate-500 hover:bg-slate-200 hover:text-slate-700"
                        aria-label="Copy room link"
                      >
                        <HiMiniLink className="size-3" />
                      </button>
                    </TooltipTrigger>
                    <TooltipContent side="bottom" className="flex flex-col items-center gap-0">
                      <span>Copy room link for web</span>
                      <span className="text-xs text-slate-400">Share with teammates</span>
                    </TooltipContent>
                  </Tooltip>
                </TooltipProvider>
              </div>
              <span className="text-xs muted font-medium">
                started{" "}
                {formatDistanceToNow(callTokens.timeStarted, {
                  addSuffix: true,
                })}
              </span>
            </div>
            : <div className="w-full text-center mb-4">
              <span className="text-xs font-medium">Pairing</span>{" "}
              <span className="text-xs muted font-medium">
                started{" "}
                {formatDistanceToNow(callTokens.timeStarted, {
                  addSuffix: true,
                })}
              </span>
            </div>)}
      </div>
      <div className="gap-2 px-3 flex flex-col items-center w-full mb-2">
        <div className="flex flex-row gap-1 w-full">
          <MediaDevicesSettings
            micShortcut={userSettings?.shortcut_toggle_mic}
            cameraShortcut={userSettings?.shortcut_toggle_camera}
            screenShareShortcut={userSettings?.shortcut_toggle_screenshare}
          />
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
            <TooltipProvider>
              <Tooltip delayDuration={100}>
                <TooltipTrigger asChild>
                  <Button
                    className="w-full border-red-500 text-red-600 flex flex-row gap-2"
                    variant="gradient-white"
                    onClick={handleEndCall}
                  >
                    <HiOutlinePhoneXMark className="size-4" />
                    End call
                  </Button>
                </TooltipTrigger>
                {userSettings?.shortcut_end_call && (
                  <TooltipContent sideOffset={0} side="bottom" variant="transparent">
                    <kbd className="text-xs">{userSettings.shortcut_end_call}</kbd>
                  </TooltipContent>
                )}
              </Tooltip>
            </TooltipProvider>
          </div>
        </div>
      </div>
      <CallParticipants />
      {/* Horizontal line */}
      <Separator className="w-full mt-4" />
    </div>
  );
}

function CallParticipants() {
  const { callTokens, teammates, user } = useStore();
  const coreParticipants = callTokens?.participants ?? [];
  const isRoomCall = !!(callTokens?.isRoomCall || callTokens?.room);
  const prevCountRef = useRef(coreParticipants.length);

  // Play sound when a new participant connects (count increases)
  useEffect(() => {
    if (coreParticipants.length > prevCountRef.current && prevCountRef.current !== 0) {
      sounds.callAccepted.play();
    }
    prevCountRef.current = coreParticipants.length;
  }, [coreParticipants.length]);

  const participantList = useMemo(() => {
    const extractUserId = (identity: string): string => {
      const parts = identity.split(":");
      return parts.length >= 4 ? (parts[2] ?? identity) : identity;
    };

    const findUser = (participantId: string) => {
      if (user && user.id === participantId) return user;
      return teammates?.find((t) => t.id === participantId) ?? null;
    };

    const localEntry =
      isRoomCall && user ?
        {
          id: "local",
          participantId: user.id,
          user,
          isLocal: true,
          isMicrophoneEnabled: callTokens?.hasAudioEnabled ?? true,
        }
        : null;

    const remoteEntries = coreParticipants
      .filter((p) => p.connected)
      .filter((p) => {
        if (p.identity === "local") return false;
        const pid = extractUserId(p.identity);
        return pid !== user?.id;
      })
      .map((p) => {
        const participantId = extractUserId(p.identity);
        return {
          id: p.identity,
          participantId,
          user: findUser(participantId),
          isLocal: false,
          isMicrophoneEnabled: !p.muted,
        };
      });

    return localEntry ? [localEntry, ...remoteEntries] : remoteEntries;
  }, [coreParticipants, teammates, user, isRoomCall, callTokens?.hasAudioEnabled]);

  if (participantList.length === 0) return null;

  return (
    <div className="flex flex-col w-full mt-3 px-3">
      <span className="text-xs font-medium text-slate-600 mb-2">Participants</span>
      <div className="flex flex-col gap-3">
        {participantList.map((participant) => (
          <div key={participant.id} className="flex items-center gap-3">
            {participant.user ?
              <>
                <div className="size-8 rounded-md bg-emerald-200 flex items-center justify-center shrink-0 overflow-hidden ring-1 ring-white">
                  {participant.user.avatar_url ?
                    <img
                      src={participant.user.avatar_url}
                      alt={`${participant.user.first_name} ${participant.user.last_name}`}
                      className="size-full object-cover"
                    />
                    : <span className="text-[10px] font-medium text-emerald-700">
                      {participant.user.first_name[0]}
                      {participant.user.last_name[0]}
                    </span>
                  }
                </div>
                <div className="flex flex-col">
                  <span className="text-sm font-medium">
                    {participant.user.first_name} {participant.user.last_name}
                    {participant.isLocal && " (You)"}
                  </span>
                  {!participant.isMicrophoneEnabled && (
                    <span className="flex items-center gap-1 text-xs font-medium text-orange-500">
                      <LuMicOff className="size-3" />
                      <span className="mt-0.5">Muted</span>
                    </span>
                  )}
                </div>
              </>
              : <>
                <div className="size-8 rounded-md bg-slate-200 flex items-center justify-center shrink-0">
                  <span className="text-xs font-medium text-slate-600">?</span>
                </div>
                <div className="flex flex-col">
                  <span className="text-sm font-medium text-slate-600">
                    Unknown user
                    {participant.isLocal && " (You)"}
                  </span>
                </div>
              </>
            }
          </div>
        ))}
      </div>
    </div>
  );
}

function DrawingEnableButton() {
  const [drawingPermanent, setDrawingPermanent] = useState(false);
  const [drawingEnabled, setDrawingEnabled] = useState(false);
  const [dropdownOpen, setDropdownOpen] = useState(false);
  const [hintShown, setHintShown] = useState(false);

  useEffect(() => {
    const loadPreferences = async () => {
      try {
        const [permanent, shown] = await Promise.all([
          tauriUtils.getSharerDrawPersist(),
          tauriUtils.getDrawingHintShown(),
        ]);
        setDrawingPermanent(permanent);
        setHintShown(shown);
      } catch (error) {
        console.error("Failed to load drawing preferences:", error);
      }
    };
    loadPreferences();
  }, []);

  useEffect(() => {
    const unlisten = listen("core_drawing_disabled", () => {
      setDrawingEnabled(false);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const handlePermanentToggle = async (checked: boolean) => {
    setDrawingPermanent(checked);
    try {
      await tauriUtils.setSharerDrawPersist(checked);
    } catch (error) {
      console.error("Failed to save drawing permanent preference:", error);
    }
  };

  const handleToggleDrawing = async () => {
    const newEnabled = !drawingEnabled;
    try {
      if (newEnabled && !hintShown) {
        await new Promise<void>((resolve) => {
          const toastDurationMs = 3_000;
          let dismissed = false;
          let timeoutId: ReturnType<typeof setTimeout> | undefined;

          const dismissHintToast = () => {
            if (dismissed) return;
            dismissed = true;
            if (timeoutId) clearTimeout(timeoutId);
            resolve();
          };

          const toastId = toast(
            (t) => (
              <div className="flex items-center gap-3">
                <span className="text-sm leading-none">Press ESC to exit drawing mode</span>
                <button
                  type="button"
                  onClick={() => {
                    toast.dismiss(t.id);
                    dismissHintToast();
                  }}
                  aria-label="Dismiss drawing hint"
                  className="size-6 shrink-0 rounded-full border-0 bg-white p-0 text-slate-900 transition-colors hover:bg-slate-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-slate-400/60"
                >
                  <X className="mx-auto size-3 stroke-[1.25]" />
                </button>
              </div>
            ),
            { duration: toastDurationMs },
          );

          timeoutId = setTimeout(() => {
            toast.dismiss(toastId);
            dismissHintToast();
          }, toastDurationMs);
        });
        await tauriUtils.setDrawingHintShown(true);
        setHintShown(true);
      }

      await tauriUtils.setDrawingEnabled(newEnabled, drawingPermanent);
      setDrawingEnabled(newEnabled);
    } catch (error) {
      console.error("Failed to toggle drawing:", error);
      toast.error("Failed to toggle drawing", { duration: 2500 });
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
              onClick={handleToggleDrawing}
              className="rounded-none first:rounded-l-lg focus:z-10"
            >
              {drawingEnabled ?
                <PiCursorBold className="size-4" />
                : <PiScribbleLoopBold className="size-4" />}
            </Button>
          </TooltipTrigger>
          <TooltipContent side="bottom">{drawingEnabled ? "Disable drawing" : "Enable drawing"}</TooltipContent>
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
 */
function MicrophoneIcon({ shortcut }: { shortcut?: string }) {
  const { updateCallTokens, callTokens } = useStore();
  const hasAudioEnabled = callTokens?.hasAudioEnabled || false;

  const { data: microphoneDevices = [], refetch: refetchMics } = useQuery({
    queryKey: ["list_microphones"],
    enabled: !callTokens?.isInitialisingCall,
    queryFn: async () => typedInvoke("list_microphones"),
    select: (data) => data.sort((a, b) => a.name.localeCompare(b.name)),
  });

  const [activeMicId, setActiveMicId] = useState<string>("");
  const [tooltipOpen, setTooltipOpen] = useState(false);
  const [selectOpen, setSelectOpen] = useState(false);
  const suppressTooltipRef = useRef(false);

  const handleTooltipOpenChange = useCallback(
    (open: boolean) => {
      if (!open) {
        setTooltipOpen(false);
        return;
      }

      if (selectOpen || suppressTooltipRef.current) return;
      setTooltipOpen(true);
    },
    [selectOpen],
  );

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
      setSelectOpen(open);
      if (open) {
        setTooltipOpen(false);
        suppressTooltipRef.current = false;
        refetchMics();
      } else {
        suppressTooltipRef.current = true;
        setTooltipOpen(false);
      }
    },
    [refetchMics],
  );

  return (
    <TooltipProvider>
      <Tooltip delayDuration={100} open={tooltipOpen} onOpenChange={handleTooltipOpenChange}>
        <TooltipTrigger asChild>
          <ToggleIconButton
            onPointerEnter={() => {
              if (!selectOpen) {
                suppressTooltipRef.current = false;
              }
            }}
            onClick={handleMicToggle}
            icon={
              <div className="relative flex items-center justify-center">
                {hasAudioEnabled ?
                  <CustomIcons.MicWithLevel
                    level={callTokens?.micLevel ?? 0}
                    className={`size-4 ${Colors.mic.icon} relative z-10`}
                  />
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
              <Select
                value={activeMicId}
                onValueChange={handleMicrophoneChange}
                onOpenChange={handleDropdownOpenChange}
              >
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
        </TooltipTrigger>
        {shortcut && (
          <TooltipContent sideOffset={0} side="bottom" variant="transparent">
            <kbd className="text-xs">{shortcut}</kbd>
          </TooltipContent>
        )}
      </Tooltip>
    </TooltipProvider>
  );
}

function ScreenShareIcon({ callTokens, shortcut }: { callTokens: CallState | null; shortcut?: string }) {
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
    <TooltipProvider>
      <Tooltip delayDuration={100}>
        <TooltipTrigger asChild>
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
        </TooltipTrigger>
        {shortcut && (
          <TooltipContent sideOffset={0} side="bottom" variant="transparent">
            <kbd className="text-xs">{shortcut}</kbd>
          </TooltipContent>
        )}
      </Tooltip>
    </TooltipProvider>
  );
}

function CameraIcon({ shortcut }: { shortcut?: string }) {
  const { updateCallTokens, callTokens } = useStore();
  const cameraEnabled = callTokens?.hasCameraEnabled || false;

  const { data: cameraDevices = [], refetch: refetchCameras } = useQuery({
    queryKey: ["list_webcams"],
    queryFn: () => typedInvoke("list_webcams"),
    select: (data) => data.sort((a, b) => a.name.localeCompare(b.name)),
  });

  const [activeCamera, setActiveCamera] = useState<string>("");
  const [tooltipOpen, setTooltipOpen] = useState(false);
  const [selectOpen, setSelectOpen] = useState(false);
  const suppressTooltipRef = useRef(false);

  const handleTooltipOpenChange = useCallback(
    (open: boolean) => {
      if (!open) {
        setTooltipOpen(false);
        return;
      }

      if (selectOpen || suppressTooltipRef.current) return;
      setTooltipOpen(true);
    },
    [selectOpen],
  );

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
      setSelectOpen(open);
      if (open) {
        setTooltipOpen(false);
        suppressTooltipRef.current = false;
        refetchCameras();
      } else {
        suppressTooltipRef.current = true;
        setTooltipOpen(false);
      }
    },
    [refetchCameras],
  );

  return (
    <TooltipProvider>
      <Tooltip delayDuration={100} open={tooltipOpen} onOpenChange={handleTooltipOpenChange}>
        <TooltipTrigger asChild>
          <ToggleIconButton
            onPointerEnter={() => {
              if (!selectOpen) {
                suppressTooltipRef.current = false;
              }
            }}
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
        </TooltipTrigger>
        {shortcut && (
          <TooltipContent sideOffset={0} side="bottom" variant="transparent">
            <kbd className="text-xs">{shortcut}</kbd>
          </TooltipContent>
        )}
      </Tooltip>
    </TooltipProvider>
  );
}

function MediaDevicesSettings({
  micShortcut,
  cameraShortcut,
  screenShareShortcut,
}: {
  micShortcut?: string;
  cameraShortcut?: string;
  screenShareShortcut?: string;
}) {
  const { callTokens } = useStore();

  return (
    <div className="flex flex-row gap-1 w-full">
      <MicrophoneIcon shortcut={micShortcut} />
      <CameraIcon shortcut={cameraShortcut} />
      <ScreenShareIcon callTokens={callTokens} shortcut={screenShareShortcut} />
    </div>
  );
}
