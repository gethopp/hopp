import { formatDistanceToNow } from "date-fns";
import { LuMic, LuMicOff, LuVideo, LuVideoOff, LuScreenShare, LuScreenShareOff } from "react-icons/lu";
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
  useRemoteParticipants,
  useRoomContext,
  useTracks,
} from "@livekit/components-react";
import { Track, RemoteParticipant, ConnectionState, RoomEvent, VideoPresets, LocalTrack } from "livekit-client";
import { useCallback, useEffect, useState } from "react";
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
import { HiOutlinePhoneXMark } from "react-icons/hi2";
import toast from "react-hot-toast";

const Colors = {
  deactivatedIcon: "text-slate-600",
  deactivatedText: "text-slate-500",
  mic: { text: "text-blue-600", icon: "text-blue-600", ring: "ring-blue-600" },
  camera: { text: "text-green-600", icon: "text-green-600", ring: "ring-green-600" },
  screen: { text: "text-yellow-600", icon: "text-yellow-600", ring: "ring-yellow-600" },
} as const;

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

      <ConnectedActions />

      {/* Horizontal line */}
      <Separator className="w-full" />
    </div>
  );
}

export function ConnectedActions() {
  const { callTokens, teammates, setCallTokens } = useStore();
  const posthog = usePostHog();
  const callParticipant = teammates?.find((user) => user.id === callTokens?.participant);
  const [controllerCursorState, setControllerCursorState] = useState(true);
  const [accessibilityPermission, setAccessibilityPermission] = useState(true);
  const [controllerSupportsAv1, setControllerSupportsAv1] = useState(false);

  const fetchAccessibilityPermission = async () => {
    const permission = await tauriUtils.getControlPermission();
    setAccessibilityPermission(permission);
    setControllerCursorState(permission);

    if (callTokens?.role === ParticipantRole.SHARER && (!permission || (permission && !accessibilityPermission))) {
      console.log("Accessibility permission is false, setting controller cursor to false");
      // We need to make sure the viewing window has opened
      setTimeout(() => {
        tauriUtils.setControllerCursor(permission);
      }, 2000);
    }
  };

  useEffect(() => {
    fetchAccessibilityPermission();
  }, [callTokens?.role]);

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

  const handleRoleChange = useCallback(
    (value: ParticipantRole) => {
      if (!callTokens) return;
      setCallTokens({
        ...callTokens,
        role: value,
      });
    },
    [callTokens],
  );

  // Stop call when teammate disconnects
  useEffect(() => {
    if (!callTokens || !callParticipant) return;

    if (!callParticipant.is_active) {
      handleEndCall();
    }
  }, [callParticipant, teammates, callTokens]);

  const { localParticipant } = useLocalParticipant();
  const remoteParticipants = useRemoteParticipants();
  const room = useRoomContext();

  useEffect(() => {
    if (!localParticipant || localParticipant === undefined || room?.state !== ConnectionState.Connected) return;

    if (localParticipant?.permissions) {
      const updatedPermissions = localParticipant.permissions;
      updatedPermissions.canUpdateMetadata = true;
      localParticipant.setPermissions(updatedPermissions);
    }

    const revCaps = RTCRtpReceiver.getCapabilities("video");
    let av1Support = false;
    for (const codec of revCaps?.codecs || []) {
      if (codec.mimeType === "video/AV1") {
        av1Support = true;
        break;
      }
    }
    localParticipant.setAttributes({
      av1Support: av1Support.toString(),
    });

    setControllerSupportsAv1(
      remoteParticipants
        .filter((p) => p.identity.includes("audio"))
        .every((p) => p.attributes["av1Support"] === "true"),
    );
  }, [localParticipant, room?.state, remoteParticipants]);

  return (
    <>
      <ScreensharingEventListener callTokens={callTokens} updateRole={handleRoleChange} />
      {/* <ConnectionsHealthDebug /> */}
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
        <div className="flex flex-col gap-2 items-center col-span-9">
          <div className="flex flex-row gap-1 w-full">
            <MicrophoneIcon />
            <CameraIcon />
            <ScreenShareIcon
              callTokens={callTokens}
              setCallTokens={setCallTokens}
              controllerSupportsAv1={controllerSupportsAv1}
            />
          </div>
          <div className="flex flex-col gap-2 w-full">
            {callTokens?.role === ParticipantRole.CONTROLLER && (
              <Button
                className="w-full border-gray-500 text-gray-600 flex flex-row gap-2"
                variant="gradient-white"
                onClick={() => {
                  tauriUtils.createScreenShareWindow(callTokens.videoToken);
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

  const { isNoiseFilterPending, setNoiseFilterEnabled } = useKrispNoiseFilter();

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
      icon={
        hasAudioEnabled ?
          <LuMic className={`size-4 ${Colors.mic.icon}`} />
        : <LuMicOff className={`size-4 ${Colors.deactivatedIcon}`} />
      }
      state={hasAudioEnabled ? "active" : "neutral"}
      size="unsized"
      className={clsx("flex-1 min-w-0", {
        [Colors.deactivatedText]: !hasAudioEnabled,
        [`${Colors.mic.text} ${Colors.mic.ring}`]: hasAudioEnabled,
      })}
      cornerIcon={
        <Select
          value={activeMicrophoneDeviceId}
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
      {hasAudioEnabled ? "Mute me" : "Open mic"}
    </ToggleIconButton>
  );
}

function ScreenShareIcon({
  callTokens,
  setCallTokens,
  controllerSupportsAv1,
}: {
  callTokens: CallState | null;
  setCallTokens: (callTokens: CallState | null) => void;
  controllerSupportsAv1: boolean;
}) {
  const isRoomCall = callTokens?.isRoomCall || false;
  const toggleScreenShare = useCallback(async () => {
    if (!callTokens || !callTokens.videoToken) return;

    if (callTokens.role === ParticipantRole.NONE || callTokens.role === ParticipantRole.CONTROLLER) {
      // On success it will update CallState.hasVideoEnabled and State.isController
      tauriUtils.createContentPickerWindow(callTokens.videoToken, controllerSupportsAv1 && !isRoomCall);
    } else if (callTokens.role === ParticipantRole.SHARER) {
      setCallTokens({
        ...callTokens,
        role: ParticipantRole.NONE,
        isRemoteControlEnabled: true,
      });
      tauriUtils.stopSharing();
    }
  }, [callTokens, callTokens?.videoToken, controllerSupportsAv1, isRoomCall]);

  const changeScreenShare = useCallback(() => {
    if (!callTokens || !callTokens.videoToken) return;
    tauriUtils.createContentPickerWindow(callTokens.videoToken, controllerSupportsAv1 && !isRoomCall);
  }, [callTokens, callTokens?.videoToken, controllerSupportsAv1, isRoomCall]);

  return (
    <ToggleIconButton
      onClick={toggleScreenShare}
      icon={
        callTokens?.role === ParticipantRole.SHARER ?
          <LuScreenShare className={`size-4 ${Colors.screen.icon}`} />
        : <LuScreenShareOff className={`size-4 ${Colors.deactivatedIcon}`} />
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
  const room = useRoomContext();
  useEffect(() => {
    const localParticipantId = localParticipant?.localParticipant.identity.split(":").slice(0, -1).join(":") || "";
    let trackFound = false;
    let screenshareTrackFound = false;
    for (const participant of room.remoteParticipants) {
      for (const track of participant[1].getTrackPublications()) {
        const trackParticipantId = participant[1].identity.split(":").slice(0, -1).join(":");

        if (track.source === "screen_share" && trackParticipantId === localParticipantId) {
          screenshareTrackFound = true;
          break;
        }

        if (track.source === "screen_share" && trackParticipantId !== localParticipantId) {
          trackFound = true;
          break;
        }
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
        tauriUtils.closeScreenShareWindow();
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

function CameraIcon() {
  const { updateCallTokens, callTokens } = useStore();
  const [retry, setRetry] = useState(0);

  const [cameraEnabled, setCameraEnabled] = useState(callTokens?.cameraTrackId !== null);

  const tracks = useTracks([Track.Source.Camera], {});
  const [roomConnected, setRoomConnected] = useState(false);
  const room = useRoomContext();
  useEffect(() => {
    room.on(RoomEvent.Connected, () => {
      setRoomConnected(true);
    });
  }, [room]);

  const { localParticipant } = useLocalParticipant();

  const pubUnpubTrack = useCallback(
    async (hasCameraEnabled: boolean) => {
      if (!localParticipant) return;

      if (hasCameraEnabled) {
        try {
          await localParticipant.setCameraEnabled(
            true,
            {
              deviceId: activeCameraDeviceId,
              resolution: VideoPresets.h216.resolution,
            },
            {
              videoCodec: "h264",
            },
          );
        } catch (error) {
          console.error("Error selecting camera: ", error);
          toast.error("Failed to select camera", {
            duration: 2500,
          });
        }
      } else {
        const cameraTrack = localParticipant
          .getTrackPublications()
          .filter((track) => track.source === Track.Source.Camera)[0];
        if (cameraTrack && cameraTrack.track && cameraTrack.track instanceof LocalTrack) {
          localParticipant.unpublishTrack(cameraTrack.track);
        }
      }
    },
    [localParticipant],
  );

  const errorCallback = useCallback(
    (error: Error) => {
      console.error("Error initializing camera: ", error);
      toast.error("Failed to initialize camera", {
        duration: 2500,
      });
    },
    [retry],
  );

  const {
    devices: cameraDevices,
    activeDeviceId: activeCameraDeviceId,
    setActiveMediaDevice: setActiveCameraDevice,
  } = useMediaDeviceSelect({
    kind: "videoinput",
    requestPermissions: true,
    onError: errorCallback,
  });

  useEffect(() => {
    if (roomConnected) {
      pubUnpubTrack(cameraEnabled);
    }
  }, [cameraEnabled, localParticipant, roomConnected]);

  const handleCameraToggle = () => {
    setCameraEnabled(!cameraEnabled);
  };

  const handleCameraChange = (value: string) => {
    console.debug("Selected camera: ", value);
    setActiveCameraDevice(value);
  };

  const handleDropdownOpenChange = (open: boolean) => {
    if (open) {
      setRetry((prev) => prev + 1);
    }
  };

  useEffect(() => {
    if (tracks.length > 0) {
      tauriUtils.ensureCameraWindowIsVisible(callTokens?.cameraToken || "");
    } else {
      // If there are 0 then close the window
      tauriUtils.closeCameraWindow();
    }

    if (localParticipant) {
      for (const track of localParticipant.getTrackPublications()) {
        if (track.source === Track.Source.Camera) {
          updateCallTokens({
            cameraTrackId: track.trackSid,
          });
        }
      }
    }
  }, [tracks]);

  const isDisabled = cameraDevices.length === 0;
  return (
    <ToggleIconButton
      onClick={handleCameraToggle}
      icon={
        cameraEnabled ?
          <LuVideo className={`size-4 ${Colors.camera.icon}`} />
        : <LuVideoOff className={`size-4 ${Colors.deactivatedIcon}`} />
      }
      state={
        cameraEnabled ? "active"
        : isDisabled ?
          "deactivated"
        : "neutral"
      }
      size="unsized"
      disabled={isDisabled}
      className={clsx("flex-1 min-w-0", {
        [Colors.deactivatedText]: !cameraEnabled,
        [`${Colors.camera.text} ${Colors.camera.ring}`]: cameraEnabled,
      })}
      cornerIcon={
        <Select value={activeCameraDeviceId} onValueChange={handleCameraChange} onOpenChange={handleDropdownOpenChange}>
          <SelectTrigger
            iconClassName={clsx({
              [Colors.camera.text]: cameraEnabled,
              [Colors.deactivatedIcon]: !cameraEnabled,
            })}
            className="hover:outline hover:outline-1 hover:outline-slate-300 focus:ring-0 focus-visible:ring-0 hover:bg-slate-200 size-4 rounded-sm p-0 border-0 shadow-none hover:shadow-xs"
          />
          <SelectPortal container={document.getElementsByClassName("container")[0]}>
            <SelectContent align="center">
              {cameraDevices.map((device) => {
                return (
                  <SelectItem key={device.deviceId} value={device.deviceId}>
                    <span className="text-xs truncate">{device.label || `Camera ${device.label.slice(0, 8)}...`}</span>
                  </SelectItem>
                );
              })}
            </SelectContent>
          </SelectPortal>
        </Select>
      }
    >
      {cameraEnabled ? "Stop sharing" : "Share cam"}
    </ToggleIconButton>
  );
}
