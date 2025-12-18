import { useSearchParams, useNavigate } from "react-router-dom";
import {
  LiveKitRoom,
  useLocalParticipant,
  useMediaDeviceSelect,
  useTracks,
  useRemoteParticipants,
  useRoomContext,
  VideoTrack,
  AudioTrack,
  ParticipantTile,
  StartAudio,
} from "@livekit/components-react";
import { useEffect, useState, useCallback, useMemo } from "react";
import { LuMic, LuMicOff, LuVideo, LuVideoOff, LuScreenShare, LuScreenShareOff } from "react-icons/lu";
import { HiOutlinePhoneXMark } from "react-icons/hi2";
import { ToggleIconButton } from "@/components/ui/toggle-icon-button";
import { Button } from "@/components/ui/button";
import clsx from "clsx";
import { VideoPresets, Track, LocalTrack } from "livekit-client";

const Colors = {
  deactivatedIcon: "text-slate-600",
  deactivatedText: "text-slate-500",
  mic: { text: "text-blue-600", icon: "text-blue-600", ring: "ring-blue-600" },
  camera: { text: "text-green-600", icon: "text-green-600", ring: "ring-green-600" },
  screen: { text: "text-yellow-600", icon: "text-yellow-600", ring: "ring-yellow-600" },
} as const;

// Helper function to check if participant name should be filtered out
function shouldFilterParticipant(participantName: string): boolean {
  const lowerName = participantName.toLowerCase();
  return lowerName.includes("video") || lowerName.includes("camera");
}

// Helper function to clean participant name (remove "audio")
function cleanParticipantName(name: string): string {
  return name.replace(/audio/gi, "").trim();
}

// Helper function to get grid column classes based on participant count
function getGridCols(count: number): string {
  if (count === 1) return "grid-cols-1";
  if (count === 2) return "grid-cols-2";
  if (count <= 4) return "grid-cols-2";
  if (count <= 6) return "grid-cols-3";
  return "grid-cols-4";
}

export function Room() {
  const [searchParams] = useSearchParams();
  const [liveKitUrl, setLiveKitUrl] = useState<string | null>(null);
  const [token, setToken] = useState<string | null>(null);
  const [hasAudioEnabled, setHasAudioEnabled] = useState(false);
  const [hasCameraEnabled, setHasCameraEnabled] = useState(false);
  const [isScreenSharing, setIsScreenSharing] = useState(false);

  useEffect(() => {
    const url = searchParams.get("liveKitUrl");
    const tokenParam = searchParams.get("token");

    setLiveKitUrl(url);
    setToken(tokenParam);
  }, [searchParams]);

  if (!liveKitUrl || !token) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-center">
          <h1 className="text-2xl font-semibold mb-2">Missing Room Parameters</h1>
          <p className="text-gray-600">
            {!liveKitUrl && "LiveKitUrl is missing. "}
            {!token && "Token is missing."}
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="w-screen h-screen flex flex-col">
      <LiveKitRoom token={token} serverUrl={liveKitUrl} connect={true}>
        <div className="flex-1 p-8 overflow-auto">
          <ParticipantsGrid />
        </div>
        <div className="border-t border-slate-200 p-4">
          <MediaControls
            hasAudioEnabled={hasAudioEnabled}
            setHasAudioEnabled={setHasAudioEnabled}
            hasCameraEnabled={hasCameraEnabled}
            setHasCameraEnabled={setHasCameraEnabled}
            isScreenSharing={isScreenSharing}
            setIsScreenSharing={setIsScreenSharing}
          />
        </div>
      </LiveKitRoom>
    </div>
  );
}

function ParticipantsGrid() {
  const { localParticipant } = useLocalParticipant();
  const remoteParticipants = useRemoteParticipants();
  const cameraTracks = useTracks([Track.Source.Camera], {
    onlySubscribed: true,
  });
  const audioTracks = useTracks([Track.Source.Microphone], {
    onlySubscribed: true,
  });
  const screenShareTracks = useTracks([Track.Source.ScreenShare], {
    onlySubscribed: true,
  });

  // Filter out participants whose name contains "video" or "camera"
  const visibleCameraTracks = useMemo(() => {
    return cameraTracks.filter((track) => {
      const participantName = track.participant.name || track.participant.identity || "";
      return !shouldFilterParticipant(participantName);
    });
  }, [cameraTracks]);

  // Filter audio tracks similarly
  const visibleAudioTracks = useMemo(() => {
    return audioTracks.filter((track) => {
      const participantName = track.participant.name || track.participant.identity || "";
      return !shouldFilterParticipant(participantName);
    });
  }, [audioTracks]);

  // Get unique participants from camera tracks, but include all remote participants for audio
  const participants = useMemo(() => {
    const participantMap = new Map();

    // First, add participants with camera tracks
    visibleCameraTracks.forEach((track) => {
      const participantId = track.participant.identity;
      if (!participantMap.has(participantId)) {
        participantMap.set(participantId, {
          participant: track.participant,
          cameraTrack: track,
          audioTrack: visibleAudioTracks.find((audioTrack) => audioTrack.participant.identity === participantId),
        });
      }
    });

    // Then, add all remote participants (for audio) that don't have camera tracks
    remoteParticipants.forEach((participant) => {
      const participantName = participant.name || participant.identity || "";
      if (!shouldFilterParticipant(participantName)) {
        const participantId = participant.identity;
        if (!participantMap.has(participantId)) {
          participantMap.set(participantId, {
            participant: participant,
            cameraTrack: undefined,
            audioTrack: visibleAudioTracks.find((audioTrack) => audioTrack.participant.identity === participantId),
          });
        }
      }
    });

    return Array.from(participantMap.values());
  }, [visibleCameraTracks, visibleAudioTracks, remoteParticipants]);

  // Get the first screen share track (if any), but exclude local participant's screen share
  const screenShareTrack = useMemo(() => {
    const remoteScreenShare = screenShareTracks.find((track) => {
      if (!localParticipant) return true;
      return track.participant.identity !== localParticipant.identity;
    });
    return remoteScreenShare || null;
  }, [screenShareTracks, localParticipant]);

  if (participants.length === 0 && !screenShareTrack) {
    return (
      <div className="flex items-center justify-center h-64 border border-slate-200 rounded-lg bg-slate-50">
        <p className="text-slate-500">No other participants in the room</p>
      </div>
    );
  }

  // If there's a screen share, show it in the center with participants on the side
  if (screenShareTrack) {
    return (
      <div className="flex flex-row gap-4 h-full">
        {/* Participants column on the side */}
        {participants.length > 0 && (
          <div className="flex flex-col gap-4 w-64 flex-shrink-0 overflow-y-auto">
            {participants.map(({ participant, cameraTrack, audioTrack }) => (
              <ParticipantCard
                key={participant.identity}
                participant={participant}
                cameraTrack={cameraTrack}
                audioTrack={audioTrack}
                allAudioTracks={visibleAudioTracks}
                localParticipant={localParticipant}
              />
            ))}
          </div>
        )}
        {/* Screen share in the center */}
        <div className="flex-1 flex items-center justify-center min-w-0">
          <div className="w-full h-full rounded-lg overflow-hidden border border-slate-200 bg-slate-50">
            <VideoTrack trackRef={screenShareTrack} className="w-full h-full object-contain" />
          </div>
        </div>
      </div>
    );
  }

  // No screen share - show participants in grid
  return (
    <div>
      <div className={`grid ${getGridCols(participants.length)} gap-4`}>
        {participants.map(({ participant, cameraTrack, audioTrack }) => (
          <ParticipantCard
            key={participant.identity}
            participant={participant}
            cameraTrack={cameraTrack}
            audioTrack={audioTrack}
            allAudioTracks={visibleAudioTracks}
            localParticipant={localParticipant}
          />
        ))}
      </div>
    </div>
  );
}

function ParticipantCard({
  participant,
  cameraTrack,
  audioTrack,
  allAudioTracks,
  localParticipant,
}: {
  participant: any;
  cameraTrack: any;
  audioTrack?: any;
  allAudioTracks?: any[];
  localParticipant?: any;
}) {
  const rawParticipantName = participant.name || participant.identity || "Unknown";
  const participantName = cleanParticipantName(rawParticipantName);

  // Check if this is the local participant
  const isLocalParticipant = localParticipant && participant.identity === localParticipant.identity;

  // Always try to find an audio track for this participant
  const participantAudioTrack =
    audioTrack || allAudioTracks?.find((t: any) => t.participant.identity === participant.identity);

  return (
    <div className="relative rounded-lg overflow-hidden border border-slate-200 bg-slate-50 aspect-video">
      {cameraTrack ?
        <VideoTrack
          trackRef={cameraTrack}
          className="w-full h-full object-cover"
          style={{
            border: participant.isSpeaking ? "2px solid rgba(34, 197, 94, 0.8)" : "none",
          }}
        />
      : <div className="w-full h-full flex items-center justify-center bg-slate-200">
          <div className="text-center">
            <div className="w-16 h-16 rounded-full bg-slate-400 mx-auto mb-2 flex items-center justify-center text-white text-2xl font-semibold">
              {participantName.charAt(0).toUpperCase()}
            </div>
            {!isLocalParticipant && <p className="text-sm text-slate-600">{participantName}</p>}
          </div>
        </div>
      }
      {participantAudioTrack && (
        <ParticipantTile trackRef={participantAudioTrack}>
          <StartAudio label="Click to allow audio playback" />
          <AudioTrack volume={1.0} />
        </ParticipantTile>
      )}
      {!isLocalParticipant && (
        <div className="absolute bottom-2 left-2 bg-black/60 text-white text-xs px-2 py-1 rounded">
          {participantName}
        </div>
      )}
    </div>
  );
}

function AudioButton({
  hasAudioEnabled,
  setHasAudioEnabled,
}: {
  hasAudioEnabled: boolean;
  setHasAudioEnabled: (enabled: boolean) => void;
}) {
  const errorCallback = useCallback((error: Error) => {
    console.error("Error selecting microphone: ", error);
  }, []);

  const {
    devices: microphoneDevices,
    activeDeviceId: activeMicrophoneDeviceId,
    setActiveMediaDevice: setActiveMicrophoneDevice,
  } = useMediaDeviceSelect({
    kind: "audioinput",
    requestPermissions: false,
    onError: errorCallback,
  });

  const handleMicrophoneChange = (value: string) => {
    console.debug("Selected microphone: ", value);
    if (value !== activeMicrophoneDeviceId) {
      setActiveMicrophoneDevice(value);
    }
  };

  return (
    <ToggleIconButton
      onClick={() => {
        // Action will be implemented later
        setHasAudioEnabled(!hasAudioEnabled);
      }}
      icon={
        hasAudioEnabled ?
          <LuMic className={`size-4 ${Colors.mic.icon} relative z-10`} />
        : <LuMicOff className={`size-4 ${Colors.deactivatedIcon} relative z-10`} />
      }
      state={hasAudioEnabled ? "active" : "neutral"}
      size="unsized"
      className={clsx("min-w-[110px] max-w-[110px]", {
        [Colors.deactivatedText]: !hasAudioEnabled,
        [`${Colors.mic.text} ${Colors.mic.ring}`]: hasAudioEnabled,
      })}
      cornerIcon={
        <select
          value={activeMicrophoneDeviceId}
          onChange={(e) => handleMicrophoneChange(e.target.value)}
          onClick={(e) => e.stopPropagation()}
          className={clsx(
            "hover:outline-solid hover:outline-1 hover:outline-slate-300 focus:ring-0 focus-visible:ring-0 hover:bg-slate-200 size-4 rounded-xs p-0 border-0 shadow-none hover:shadow-xs text-xs",
            {
              [Colors.mic.text]: hasAudioEnabled,
              [Colors.deactivatedIcon]: !hasAudioEnabled,
            },
          )}
        >
          {microphoneDevices.map((device) => (
            <option key={device.deviceId} value={device.deviceId}>
              {device.label || `Microphone ${device.deviceId.slice(0, 8)}...`}
            </option>
          ))}
        </select>
      }
    >
      {hasAudioEnabled ? "Mute me" : "Open mic"}
    </ToggleIconButton>
  );
}

function CameraButton({
  hasCameraEnabled,
  setHasCameraEnabled,
}: {
  hasCameraEnabled: boolean;
  setHasCameraEnabled: (enabled: boolean) => void;
}) {
  const errorCallback = useCallback((error: Error) => {
    console.error("Error initializing camera: ", error);
  }, []);

  const {
    devices: cameraDevices,
    activeDeviceId: activeCameraDeviceId,
    setActiveMediaDevice: setActiveCameraDevice,
  } = useMediaDeviceSelect({
    kind: "videoinput",
    requestPermissions: false,
    onError: errorCallback,
  });

  const isDisabled = cameraDevices.length === 0;

  const handleCameraChange = (value: string) => {
    console.debug("Selected camera: ", value);
    setActiveCameraDevice(value);
  };

  return (
    <ToggleIconButton
      onClick={() => {
        // Action will be implemented later
        setHasCameraEnabled(!hasCameraEnabled);
      }}
      icon={
        hasCameraEnabled ?
          <LuVideo className={`size-4 ${Colors.camera.icon}`} />
        : <LuVideoOff className={`size-4 ${Colors.deactivatedIcon}`} />
      }
      state={
        hasCameraEnabled ? "active"
        : isDisabled ?
          "deactivated"
        : "neutral"
      }
      size="unsized"
      disabled={isDisabled}
      className={clsx("min-w-[110px] max-w-[110px]", {
        [Colors.deactivatedText]: !hasCameraEnabled,
        [`${Colors.camera.text} ${Colors.camera.ring}`]: hasCameraEnabled,
      })}
      cornerIcon={
        <select
          value={activeCameraDeviceId}
          onChange={(e) => handleCameraChange(e.target.value)}
          onClick={(e) => e.stopPropagation()}
          className={clsx(
            "hover:outline hover:outline-slate-300 focus:ring-0 focus-visible:ring-0 hover:bg-slate-200 size-4 rounded-sm p-0 border-0 shadow-none hover:shadow-xs text-xs",
            {
              [Colors.camera.text]: hasCameraEnabled,
              [Colors.deactivatedIcon]: !hasCameraEnabled,
            },
          )}
        >
          {cameraDevices.map(
            (device) =>
              device.deviceId !== "" && (
                <option key={device.deviceId} value={device.deviceId}>
                  {device.label || `Camera ${device.deviceId.slice(0, 8)}...`}
                </option>
              ),
          )}
        </select>
      }
    >
      {hasCameraEnabled ? "Stop sharing" : "Share cam"}
    </ToggleIconButton>
  );
}

function MediaControls({
  hasAudioEnabled,
  setHasAudioEnabled,
  hasCameraEnabled,
  setHasCameraEnabled,
  isScreenSharing,
  setIsScreenSharing,
}: {
  hasAudioEnabled: boolean;
  setHasAudioEnabled: (enabled: boolean) => void;
  hasCameraEnabled: boolean;
  setHasCameraEnabled: (enabled: boolean) => void;
  isScreenSharing: boolean;
  setIsScreenSharing: (enabled: boolean) => void;
}) {
  const { localParticipant } = useLocalParticipant();
  useEffect(() => {
    if (!localParticipant) return;
    localParticipant.setMicrophoneEnabled(hasAudioEnabled);

    if (hasCameraEnabled) {
      localParticipant.setCameraEnabled(
        hasCameraEnabled,
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
    } else {
      // Unpublish the camera track when disabled
      const cameraTrack = localParticipant.getTrackPublications().find((track) => track.source === Track.Source.Camera);
      if (cameraTrack && cameraTrack.track && cameraTrack.track instanceof LocalTrack) {
        localParticipant.unpublishTrack(cameraTrack.track);
      }
    }

    localParticipant.setScreenShareEnabled(isScreenSharing);
  }, [localParticipant, hasAudioEnabled, hasCameraEnabled, isScreenSharing]);

  return (
    <div className="flex flex-row gap-2 justify-center items-center flex-wrap">
      <AudioButton hasAudioEnabled={hasAudioEnabled} setHasAudioEnabled={setHasAudioEnabled} />
      <CameraButton hasCameraEnabled={hasCameraEnabled} setHasCameraEnabled={setHasCameraEnabled} />
      <ScreenShareButton isScreenSharing={isScreenSharing} setIsScreenSharing={setIsScreenSharing} />
      <EndCallButton />
    </div>
  );
}

function ScreenShareButton({
  isScreenSharing,
  setIsScreenSharing,
}: {
  isScreenSharing: boolean;
  setIsScreenSharing: (enabled: boolean) => void;
}) {
  return (
    <ToggleIconButton
      onClick={() => {
        // Action will be implemented later
        setIsScreenSharing(!isScreenSharing);
      }}
      icon={
        isScreenSharing ?
          <LuScreenShare className={`size-4 ${Colors.screen.icon}`} />
        : <LuScreenShareOff className={`size-4 ${Colors.deactivatedIcon}`} />
      }
      state={isScreenSharing ? "active" : "neutral"}
      size="unsized"
      className={clsx("min-w-[110px] max-w-[110px]", {
        [Colors.deactivatedText]: !isScreenSharing,
        [`${Colors.screen.text} ${Colors.screen.ring}`]: isScreenSharing,
      })}
    >
      {isScreenSharing ? "Stop sharing" : "Share screen"}
    </ToggleIconButton>
  );
}

function EndCallButton() {
  const room = useRoomContext();
  const navigate = useNavigate();

  const handleEndCall = useCallback(async () => {
    try {
      await room.disconnect();
      // Navigate away from the room page
      navigate("/dashboard");
    } catch (error) {
      console.error("Error disconnecting from room:", error);
    }
  }, [room, navigate]);

  return (
    <Button
      onClick={handleEndCall}
      className="min-w-[110px] max-w-[110px] border-red-500 text-red-600 flex flex-row gap-2"
      variant="outline"
    >
      <HiOutlinePhoneXMark className="size-4" />
      <span className="whitespace-nowrap">End call</span>
    </Button>
  );
}
