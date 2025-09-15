import { formatDistanceToNow } from "date-fns";
import { HiMiniComputerDesktop, HiOutlineMicrophone, HiOutlinePhoneXMark } from "react-icons/hi2";
import useStore, { CallState, ParticipantRole } from "@/store/store";
import { useKrispNoiseFilter } from "@livekit/components-react/krisp";
import { Separator } from "@/components/ui/separator";
import { ToggleIconButton } from "@/components/ui/toggle-icon-button";
import { sounds } from "@/constants/sounds";
import { socketService } from "@/services/socket";
import {
  AudioTrack,
  ParticipantTile,
  StartAudio,
  useLocalParticipant,
  useMediaDeviceSelect,
  useRoomContext,
  useTracks,
} from "@livekit/components-react";
import { Track, RemoteParticipant, LocalTrack, ConnectionState, RoomEvent } from "livekit-client";
import { useCallback, useEffect, useRef, useState } from "react";
import { Select, SelectContent, SelectItem, SelectTrigger } from "./select";
import { SelectPortal } from "@radix-ui/react-select";
import { Button } from "./button";
import { tauriUtils } from "@/windows/window-utils";
import { HoppAvatar } from "./hopp-avatar";
import { HiOutlineCursorClick, HiOutlineEye } from "react-icons/hi";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import clsx from "clsx";
import { usePostHog } from "posthog-js/react";
import { ChevronDownIcon } from "@radix-ui/react-icons";

export function CallCenter() {
  const { callTokens } = useStore();

  if (!callTokens) return null;

  return (
    <div className="flex flex-col items-center w-full max-w-sm mx-auto bg-white pt-4 mb-4">
      <div className="w-full">
        {/* Call Timer */}
        {callTokens && (
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

      <ConnectedActions token={callTokens.audioToken} />

      {/* Horizontal line */}
      <Separator className="w-full" />
    </div>
  );
}

export function ConnectedActions({ token }: { token: string }) {
  const { callTokens, teammates, setCallTokens } = useStore();
  const posthog = usePostHog();
  const callParticipant = teammates?.find((user) => user.id === callTokens?.participant);
  const [controllerCursorState, setControllerCursorState] = useState(true);

  const handleEndCall = useCallback(() => {
    if (!callTokens) return;

    const { timeStarted, participant } = callTokens;

    // Send websocket message to end call
    socketService.send({
      type: "call_end",
      payload: {
        participant_id: participant,
      },
    });

    // Play end call sound
    sounds.callAccepted.play();

    // Clear call tokens
    if (callTokens.role === ParticipantRole.SHARER) {
      tauriUtils.stopSharing();
    }
    tauriUtils.endCallCleanup();

    setCallTokens(null);

    // Send posthog event on how much
    // time in seconds the call lasted.
    // Time is serialized as a string in store
    // so its not saved as a Date object
    console.log(`Duration of the call: ${(Date.now() - new Date(timeStarted).getTime()) / 1000}seconds`);
    posthog.capture("call_ended", {
      duration_in_seconds: Date.now() - new Date(timeStarted).getTime() / 1000,
      participant,
    });
  }, [callTokens, setCallTokens]);

  const handleRoleChange = useCallback((value: ParticipantRole) => {
    if (!callTokens) return;
    setCallTokens({
      ...callTokens,
      role: value,
    });
  }, [callTokens]);

  // Stop call when teammate disconnects
  useEffect(() => {
    if (!callTokens || !callParticipant) return;

    if (!callParticipant.is_active) {
      handleEndCall();
    }
  }, [callParticipant, teammates, callTokens]);

  return (
    <>
      <ScreensharingEventListener
        callTokens={callTokens}
        updateRole={handleRoleChange}
      />
      {/* <ConnectionsHealthDebug /> */}
      <div
        className={clsx("gap-2 px-4 flex-nowrap grid mb-4", {
          "grid-cols-6": callTokens?.isRoomCall,
          "grid-cols-12": !callTokens?.isRoomCall,
        })}
      >
        {!callTokens?.isRoomCall && (
          <div className="flex flex-col items-start mb-4 col-span-4 relative">
            <div className="relative mt-1">
              {callParticipant && (
                <HoppAvatar
                  src={callParticipant?.avatar_url || undefined}
                  firstName={callParticipant?.first_name}
                  lastName={callParticipant?.last_name}
                />
              )}
            </div>
            <div className="flex flex-col items-start mt-2 w-full">
              <span className="text-sm text-left w-full">{callParticipant?.first_name}</span>
              <span className="text-sm text-left text-slate-500 w-full truncate">{callParticipant?.last_name}</span>
            </div>
          </div>
        )}
        <div className="flex flex-col gap-2 items-center col-span-8">
          <div className="flex flex-row gap-2 w-full">
            <MicrophoneIcon />
            <ScreenShareIcon callTokens={callTokens} setCallTokens={setCallTokens} />
          </div>
          <div className="flex flex-col gap-2 w-full">
            {callTokens?.role === ParticipantRole.CONTROLLER && (
              <Button
                className="w-full border-gray-500 text-gray-600 flex flex-row gap-2"
                variant="gradient-white"
                onClick={() => {
                  tauriUtils.createScreenShareWindow(callTokens.videoToken)
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
                        state={controllerCursorState ? "active" : "neutral"}
                        size="unsized"
                        className="size-9"
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
      <ListenToRemoteAudio />
    </>
  );
}

function MicrophoneIcon() {
  const { state: roomState } = useRoomContext();
  const { localParticipant } = useLocalParticipant();
  const [roomConnected, setRoomConnected] = useState(false);
  const [retry, setRetry] = useState(0);
  const { updateCallTokens, callTokens } = useStore();
  const hasAudioEnabled = callTokens?.hasAudioEnabled || false;

  const { isNoiseFilterEnabled, isNoiseFilterPending, setNoiseFilterEnabled } = useKrispNoiseFilter();

  const room = useRoomContext();
  useEffect(() => {
    room.on(RoomEvent.Connected, () => {
      setRoomConnected(true);
    });
  }, [room]);

  useEffect(() => {
    console.debug(`Microphone state changed: ${roomState} mic: ${hasAudioEnabled}`);
    if (roomState === ConnectionState.Connected) {
      void localParticipant.setMicrophoneEnabled(hasAudioEnabled, {
        noiseSuppression: true,
        echoCancellation: true,
      });
    }

    if (hasAudioEnabled && !isNoiseFilterPending) {
      setNoiseFilterEnabled(true);
    }
  }, [roomState, hasAudioEnabled, localParticipant, roomConnected]);

  /* Force re enumeration of mic devices on dropdown open */
  const errorCallback = useCallback(
    (error: Error) => {
      console.error("Error selecting microphone: ", error);
    },
    [retry],
  );

  const {
    devices: microphoneDevices,
    activeDeviceId: activeMicrophoneDeviceId,
    setActiveMediaDevice: setActiveMicrophoneDevice,
  } = useMediaDeviceSelect({
    kind: "audioinput",
    requestPermissions: true,
    onError: errorCallback,
  });

  useEffect(() => {
    const updateDefaultMic = async () => {
      const lastUsedMic = await getLastUsedMic();
      if (!lastUsedMic) return;

      for (const device of microphoneDevices) {
        if (device.deviceId === lastUsedMic) {
          setActiveMicrophoneDevice(device.deviceId);
          break;
        }
      }
    };
    updateDefaultMic();
  }, [microphoneDevices]);

  const getLastUsedMic = useCallback(async () => {
    return await tauriUtils.getLastUsedMic();
  }, []);

  const updateMicrophonePreference = useCallback(async (deviceId: string) => {
    return await tauriUtils.setLastUsedMic(deviceId);
  }, []);

  const handleMicrophoneChange = (value: string) => {
    console.debug("Selected microphone: ", value);
    setActiveMicrophoneDevice(value);
    updateMicrophonePreference(value);
  };

  const handleDropdownOpenChange = (open: boolean) => {
    if (open) {
      setRetry((prev) => prev + 1);
    }
  };

  return (
    <ToggleIconButton
      onClick={() => {
        updateCallTokens({
          hasAudioEnabled: !hasAudioEnabled,
        });
      }}
      icon={<HiOutlineMicrophone className="size-5" />}
      state={hasAudioEnabled ? "active" : "neutral"}
      cornerIcon={
        <Select
          value={activeMicrophoneDeviceId}
          onValueChange={handleMicrophoneChange}
          onOpenChange={handleDropdownOpenChange}
        >
          <SelectTrigger className="hover:outline hover:outline-1 hover:outline-slate-300 focus:ring-0 focus-visible:ring-0 hover:bg-slate-200 size-4 rounded-sm p-0 border-0 shadow-none hover:shadow-sm" />
          <SelectPortal container={document.getElementsByClassName("container")[0]}>
            <SelectContent align="center">
              {microphoneDevices.map((device) => {
                return (
                  <SelectItem key={device.deviceId} value={device.deviceId}>
                    <span className="text-xs truncate">
                      {device.label || `Microphone ${device.label.slice(0, 8)}...`}
                    </span>
                  </SelectItem>
                );
              })}
            </SelectContent>
          </SelectPortal>
        </Select>
      }
    >
      {hasAudioEnabled ? "Mute me" : "Unmute me"}
    </ToggleIconButton>
  );
}

function ScreenShareIcon({ callTokens, setCallTokens }: { callTokens: CallState | null, setCallTokens: (callTokens: CallState | null) => void }) {
  const toggleScreenShare = useCallback(() => {
    if (!callTokens || !callTokens.videoToken) return;

    if (callTokens.role === ParticipantRole.NONE || callTokens.role === ParticipantRole.CONTROLLER) {
      // On success it will update CallState.hasVideoEnabled and State.isController
      tauriUtils.createContentPickerWindow();
    } else if (callTokens.role === ParticipantRole.SHARER) {
      setCallTokens({
        ...callTokens,
        role: ParticipantRole.NONE,
        isRemoteControlEnabled: true,
      });
      tauriUtils.stopSharing();
    }
  }, [callTokens, callTokens?.videoToken]);

  const changeScreenShare = useCallback(() => {
    if (!callTokens || !callTokens.videoToken) return;
    tauriUtils.createContentPickerWindow();
  }, [callTokens, callTokens?.videoToken]);

  return (
    <ToggleIconButton
      onClick={toggleScreenShare}
      icon={<HiMiniComputerDesktop className="size-5" />}
      state={callTokens?.role === ParticipantRole.SHARER ? "active" : "neutral"}
      cornerIcon={
        callTokens?.role === ParticipantRole.SHARER && (
          <button
            onClick={changeScreenShare}
            className="hover:outline hover:outline-1 hover:outline-slate-300 focus:ring-0 focus-visible:ring-0 hover:bg-slate-200 size-4 rounded-sm p-0 border-0 shadow-none hover:shadow-sm"
          >
            <ChevronDownIcon className="size-3" />
          </button>
        )
      }
    >
      {callTokens?.role === ParticipantRole.SHARER ? "Stop sharing" : "Share screen"}
    </ToggleIconButton>
  )
}

const ListenToRemoteAudio = () => {
  const tracks = useTracks([Track.Source.Microphone], {
    onlySubscribed: true,
  });

  return (
    <>
      {tracks
        .filter((track) => track.participant instanceof RemoteParticipant)
        .map((track) => (
          <ParticipantTile key={`${track.participant.identity}_${track.publication.trackSid}`} trackRef={track}>
            <StartAudio label="Click to allow audio playback" />
            <AudioTrack />
          </ParticipantTile>
        ))}
    </>
  );
};

function ScreensharingEventListener({
  callTokens,
  updateRole,
}: {
  callTokens: CallState | null;
  updateRole: (value: ParticipantRole) => void;
}) {
  if (!callTokens || !callTokens.videoToken) return null;

  const tracks = useTracks([Track.Source.ScreenShare]);
  const localParticipant = useLocalParticipant();
  useEffect(() => {
    const localParticipantId = localParticipant?.localParticipant.identity.split(":").slice(0, -1).join(":") || "";
    let trackFound = false;
    let screenshareTrackFound = false;
    for (const track of tracks) {
      const trackParticipantId = track.participant.identity.split(":").slice(0, -1).join(":");

      if (track.source === "screen_share" && trackParticipantId === localParticipantId) {
        screenshareTrackFound = true;
        break;
      }

      if (track.source === "screen_share" && trackParticipantId !== localParticipantId) {
        trackFound = true;
        break;
      }
    }

    let newRole: ParticipantRole;
    if (trackFound) {
      newRole = ParticipantRole.CONTROLLER;
    } else if (screenshareTrackFound) {
      newRole = ParticipantRole.SHARER;
    } else {
      newRole = ParticipantRole.NONE;
    }

    if (callTokens?.role !== newRole) {
      if (callTokens?.role === ParticipantRole.CONTROLLER && !trackFound) {
        //tauriUtils.closeScreenShareWindow();
        tauriUtils.setDockIconVisible(false);
      }

      if (newRole === ParticipantRole.CONTROLLER) {
        tauriUtils.createScreenShareWindow(callTokens.videoToken, false);
      }

      updateRole(newRole);
    }
  }, [tracks]);
  return <div />;
}
