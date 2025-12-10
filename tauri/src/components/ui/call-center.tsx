import { formatDistanceToNow } from "date-fns";
import { LuMicOff, LuVideo, LuVideoOff, LuScreenShare, LuScreenShareOff } from "react-icons/lu";
import useStore, { CallState, ParticipantRole } from "@/store/store";
import { useKrispNoiseFilter } from "@livekit/components-react/krisp";
import { Separator } from "@/components/ui/separator";
import { ToggleIconButton } from "@/components/ui/toggle-icon-button";
import { sounds } from "@/constants/sounds";
import { socketService } from "@/services/socket";
import {
  useLocalParticipant,
  useMediaDeviceSelect,
  useRemoteParticipants,
  useRoomContext,
  useTracks,
} from "@livekit/components-react";
import {
  Track,
  ConnectionState,
  RoomEvent,
  VideoPresets,
  LocalTrack,
  RemoteTrackPublication,
  ParticipantEvent,
} from "livekit-client";
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
import { usePostHog } from "posthog-js/react";
import { ChevronDownIcon } from "@radix-ui/react-icons";
import { HiOutlinePhoneXMark } from "react-icons/hi2";
import toast from "react-hot-toast";
import ListenToRemoteAudio from "./listen-to-remote-audio";
import { useScreenShareListener } from "@/lib/hooks";
import { useAudioVolume } from "@/components/ui/bar-visualizer";
import { LiveWaveform } from "@/components/ui/live-waveform";

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

  useScreenShareListener();

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

  // Stop call when teammate disconnects
  useEffect(() => {
    if (!callTokens || !callParticipant) return;

    if (!callParticipant.is_active) {
      handleEndCall();
    }
  }, [callParticipant, teammates, callTokens]);

  return (
    <>
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
            <MediaDevicesSettings />
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
      <ListenToRemoteAudio muted={callTokens?.cameraWindowOpen} />
    </>
  );
}

function MicrophoneIcon() {
  const [retry, setRetry] = useState(0);
  const { updateCallTokens, callTokens } = useStore();
  const hasAudioEnabled = callTokens?.hasAudioEnabled || false;
  const { localParticipant } = useLocalParticipant();
  const [audioStream, setAudioStream] = useState<MediaStream | null>(null);

  // Use the useAudioVolume hook from bar-visualizer for the mic button
  const audioLevel = useAudioVolume(audioStream, { fftSize: 32, smoothingTimeConstant: 0.3 });

  useEffect(() => {
    if (!localParticipant) return;

    const updateStream = () => {
      const trackPub = localParticipant.getTrackPublications().find((p) => p.kind === Track.Kind.Audio);
      if (trackPub?.track?.mediaStreamTrack) {
        setAudioStream(new MediaStream([trackPub.track.mediaStreamTrack]));
      } else {
        setAudioStream(null);
      }
    };

    updateStream();
    const onLocalTrackPublished = () => updateStream();
    const onLocalTrackUnpublished = () => updateStream();
    const onTrackMuted = () => updateStream();
    const onTrackUnmuted = () => updateStream();

    localParticipant.on(ParticipantEvent.LocalTrackPublished, onLocalTrackPublished);
    localParticipant.on(ParticipantEvent.LocalTrackUnpublished, onLocalTrackUnpublished);
    localParticipant.on(ParticipantEvent.TrackMuted, onTrackMuted);
    localParticipant.on(ParticipantEvent.TrackUnmuted, onTrackUnmuted);

    return () => {
      localParticipant.off(ParticipantEvent.LocalTrackPublished, onLocalTrackPublished);
      localParticipant.off(ParticipantEvent.LocalTrackUnpublished, onLocalTrackUnpublished);
      localParticipant.off(ParticipantEvent.TrackMuted, onTrackMuted);
      localParticipant.off(ParticipantEvent.TrackUnmuted, onTrackUnmuted);
    };
  }, [localParticipant]);

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
        if (device.deviceId === lastUsedMic && device.deviceId !== activeMicrophoneDeviceId) {
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
    if (value !== activeMicrophoneDeviceId) {
      setActiveMicrophoneDevice(value);
      updateMicrophonePreference(value);
    }
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
        <div className="relative flex items-center justify-center">
          {hasAudioEnabled ?
            <CustomIcons.MicWithLevel level={audioLevel} className={`size-4 ${Colors.mic.icon} relative z-10`} />
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
              {hasAudioEnabled && (
                <>
                  <div className="my-2 border-t border-slate-100" />
                  <div className="flex flex-row justify-between items-center gap-2">
                    <span className="text-xs font-medium ml-2 text-slate-500">Input</span>
                    <div className="relative flex h-8 max-w-[120px] items-center rounded-md bg-slate-100 overflow-hidden px-3">
                      <LiveWaveform
                        active={hasAudioEnabled}
                        deviceId={activeMicrophoneDeviceId}
                        barWidth={3}
                        barGap={1}
                        barRadius={4}
                        fadeEdges={true}
                        fadeWidth={24}
                        sensitivity={1.4}
                        smoothingTimeConstant={0.85}
                        height={24}
                        mode="static"
                        className="h-full w-full"
                      />
                    </div>
                  </div>
                </>
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

function CameraIcon() {
  const { updateCallTokens, callTokens } = useStore();
  const [retry, setRetry] = useState(0);
  const tracks = useTracks([Track.Source.Camera], {});
  const { localParticipant } = useLocalParticipant();
  const cameraEnabled = callTokens?.hasCameraEnabled || false;

  const clickedCameraRef = useRef(false);
  const errorCallback = useCallback(
    (error: Error) => {
      if (!clickedCameraRef.current) return;

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

  const isDisabled = cameraDevices.length === 0;

  const handleCameraToggle = () => {
    clickedCameraRef.current = true;
    let newCameraEnabled = !cameraEnabled;
    updateCallTokens({
      ...callTokens,
      hasCameraEnabled: newCameraEnabled,
    });
    if (!newCameraEnabled) {
      const cameraTrack = localParticipant
        .getTrackPublications()
        .filter((track) => track.source === Track.Source.Camera)[0];
      if (cameraTrack && cameraTrack.track && cameraTrack.track instanceof LocalTrack) {
        localParticipant.unpublishTrack(cameraTrack.track);
      }
    }
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
    // Filter out anonymous tracks that do not share their camera
    const filteredTracks = tracks.filter((track) => {
      if (!track.participant.identity.includes("anonymous")) {
        return true;
      }

      // If participant is anonymous and the video track is muted or not shared, return false
      for (const trackPublication of track.participant.trackPublications) {
        console.log("--- Track publication: ", trackPublication);
        const pub: RemoteTrackPublication = trackPublication[1] as RemoteTrackPublication;
        if (pub.source === Track.Source.Camera && pub.isMuted) {
          return false;
        }
      }

      return true;
    });

    if (filteredTracks.length > 0) {
      tauriUtils.ensureCameraWindowIsVisible(callTokens?.cameraToken || "");
      updateCallTokens({
        ...callTokens,
        cameraWindowOpen: true,
      });
    } else {
      // If there are 0 then close the window
      tauriUtils.closeCameraWindow();
      updateCallTokens({
        ...callTokens,
        cameraWindowOpen: false,
      });
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
            className="hover:outline hover:outline-slate-300 focus:ring-0 focus-visible:ring-0 hover:bg-slate-200 size-4 rounded-sm p-0 border-0 shadow-none hover:shadow-xs"
          />
          <SelectPortal container={document.getElementsByClassName("container")[0]}>
            <SelectContent align="center">
              {cameraDevices.map((device) => {
                return (
                  device.deviceId !== "" && (
                    <SelectItem key={device.deviceId} value={device.deviceId}>
                      <span className="text-xs truncate">
                        {device.label || `Camera ${device.label.slice(0, 8)}...`}
                      </span>
                    </SelectItem>
                  )
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

function MediaDevicesSettings() {
  const { callTokens, setCallTokens } = useStore();
  const { state: roomState } = useRoomContext();
  const { localParticipant } = useLocalParticipant();
  const { isNoiseFilterPending, setNoiseFilterEnabled, isNoiseFilterEnabled } = useKrispNoiseFilter({
    filterOptions: {
      quality: "medium",
      bufferOverflowMs: 100,
      bufferDropMs: 200,
    },
  });

  // Monitor camera bandwidth usage
  //useCameraBandwidthMonitor();

  const room = useRoomContext();
  const [roomConnected, setRoomConnected] = useState(false);
  useEffect(() => {
    room.on(RoomEvent.Connected, () => {
      setRoomConnected(true);
    });
  }, [room]);

  useEffect(() => {
    if (!callTokens) return;

    console.debug(
      `state changed: ${roomState} mic: ${callTokens?.hasAudioEnabled} camera: ${callTokens?.hasCameraEnabled}`,
    );
    if (roomState === ConnectionState.Connected) {
      console.debug(`Setting microphone enabled: ${callTokens?.hasAudioEnabled}`);
      localParticipant.setMicrophoneEnabled(callTokens?.hasAudioEnabled);

      localParticipant.setCameraEnabled(
        callTokens?.hasCameraEnabled,
        {
          resolution: VideoPresets.h720.resolution,
        },
        {
          videoCodec: "h264",
          simulcast: true,
          videoEncoding: {
            maxBitrate: 1_300_000,
          },
          videoSimulcastLayers: [VideoPresets.h360, VideoPresets.h216],
        },
      );

      // Enable Krisp filter if audio is enabled and krispToggle is not disabled
      const krispEnabled = callTokens?.krispToggle !== false; // Default to true if undefined
      if (callTokens?.hasAudioEnabled && !isNoiseFilterPending && !isNoiseFilterEnabled && krispEnabled) {
        console.log("Enabling Krisp filter");
        setNoiseFilterEnabled(true);
      } else if (!krispEnabled && isNoiseFilterEnabled) {
        console.log("Disabling Krisp filter");
        setNoiseFilterEnabled(false);
      }
    }
  }, [
    roomState,
    callTokens?.hasAudioEnabled,
    localParticipant,
    roomConnected,
    callTokens?.hasCameraEnabled,
    callTokens?.krispToggle,
  ]);

  const remoteParticipants = useRemoteParticipants();
  const [controllerSupportsAv1, setControllerSupportsAv1] = useState(false);
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

  useEffect(() => {
    if (!callTokens) return;

    if (callTokens.controllerSupportsAv1 !== controllerSupportsAv1) {
      setCallTokens({
        ...callTokens,
        controllerSupportsAv1,
      });
    }
  }, [controllerSupportsAv1, callTokens, setCallTokens]);

  return (
    <div className="flex flex-row gap-1 w-full">
      <MicrophoneIcon />
      <CameraIcon />
      <ScreenShareIcon
        callTokens={callTokens}
        setCallTokens={setCallTokens}
        controllerSupportsAv1={controllerSupportsAv1}
      />
    </div>
  );
}
