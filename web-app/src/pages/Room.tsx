import { useParams, useNavigate } from "react-router-dom";
import {
  LiveKitRoom,
  useLocalParticipant,
  useMediaDeviceSelect,
  useTracks,
  useRemoteParticipants,
  useRoomContext,
  VideoTrack,
  AudioTrack,
  TrackReference,
  StartAudio,
  useDataChannel,
} from "@livekit/components-react";
import "@livekit/components-styles";
import { HiMiniUser } from "react-icons/hi2";
import { useEffect, useState, useCallback, useMemo, useRef } from "react";
import { LuMic, LuMicOff, LuVideo, LuVideoOff, LuScreenShare } from "react-icons/lu";
import { HiOutlinePhoneXMark } from "react-icons/hi2";
import { ToggleIconButton } from "@/components/ui/toggle-icon-button";
import { Button } from "@/components/ui/button";
import clsx from "clsx";
import { VideoPresets, Track, LocalTrack, Participant } from "livekit-client";
import { useAPI } from "@/hooks/useQueryClients";
import { useHoppStore } from "@/store/store";

// HACK: Import shared components from tauri app for cursor rendering
// These files use relative imports so they work across projects
import { Cursor } from "../../../tauri/src/components/ui/cursor";
import { TPMouseMove } from "../../../tauri/src/payloads";

const CURSORS_TOPIC = "participant_location";

// Cursor slot for tracking remote participant cursors
interface CursorSlot {
  participantId: string | null;
  participantName: string;
  x: number;
  y: number;
  lastActivity: number;
}

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
  const { roomId } = useParams<{ roomId: string }>();
  const navigate = useNavigate();
  const [hasAudioEnabled, setHasAudioEnabled] = useState(false);
  const [hasCameraEnabled, setHasCameraEnabled] = useState(false);

  const { useQuery } = useAPI();

  // Fetch room tokens using the same endpoint the app uses
  const {
    data: roomTokens,
    isLoading: isLoadingTokens,
    error: tokensError,
  } = useQuery("get", "/api/auth/room/{id}", {
    params: {
      path: {
        id: roomId || "",
      },
    },
  });

  const {
    data: livekitServer,
    isLoading: isLoadingServer,
    error: serverError,
  } = useQuery("get", "/api/auth/livekit/server-url", undefined);

  const isLoading = isLoadingTokens || isLoadingServer;
  const error = tokensError || serverError;

  if (!roomId) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-center">
          <h1 className="text-2xl font-semibold mb-2">Room Not Found</h1>
          <p className="text-gray-600 mb-4">No room ID was provided.</p>
          <Button onClick={() => navigate("/dashboard")}>Go to Dashboard</Button>
        </div>
      </div>
    );
  }

  if (isLoading) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-center">
          <h1 className="text-2xl font-semibold mb-2">Joining Room...</h1>
          <p className="text-gray-600">Please wait while we connect you to the room.</p>
        </div>
      </div>
    );
  }

  if (error || !roomTokens || !livekitServer) {
    return (
      <div className="flex items-center justify-center h-full">
        <div className="text-center">
          <h1 className="text-2xl font-semibold mb-2">Unable to Join Room</h1>
          <p className="text-gray-600 mb-4">
            {error ? "You don't have access to this room or the room doesn't exist." : "Failed to get room access."}
          </p>
          <Button onClick={() => navigate("/dashboard")}>Go to Dashboard</Button>
        </div>
      </div>
    );
  }

  return (
    <div
      className="w-full h-full flex flex-col overflow-hidden"
      style={{
        maxHeight: "calc(100vh - var(--spacing)*16)",
      }}
    >
      <LiveKitRoom
        className="relative flex flex-col"
        token={roomTokens.audioToken}
        serverUrl={livekitServer.url}
        connect={true}
      >
        {/* TODO: Add a prop for on-render on this component, if there is an issue to maybe fire-off a toast to the user */}
        {/* to ask them to "unmute" or something explicit as an action for sound to play */}
        <StartAudio label="Click to allow audio playback" />
        {/* Main content area - takes remaining space, no overflow */}
        <div className="flex-1 min-h-0 p-4">
          <ParticipantsGrid />
        </div>
        {/* Fixed controls at bottom */}
        <div className="justify-end grow-0 w-full border-t border-slate-200 bg-white pt-4">
          <MediaControls
            hasAudioEnabled={hasAudioEnabled}
            setHasAudioEnabled={setHasAudioEnabled}
            hasCameraEnabled={hasCameraEnabled}
            setHasCameraEnabled={setHasCameraEnabled}
          />
        </div>
      </LiveKitRoom>
    </div>
  );
}

function ParticipantsGrid() {
  const { localParticipant } = useLocalParticipant();
  const remoteParticipants = useRemoteParticipants();
  const user = useHoppStore((state) => state.user);

  const cameraTracks = useTracks([Track.Source.Camera], {
    onlySubscribed: false, // Include local tracks too
  });
  const audioTracks = useTracks([Track.Source.Microphone], {
    onlySubscribed: false,
  });
  const screenShareTracks = useTracks([Track.Source.ScreenShare], {
    onlySubscribed: false,
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

  // Build participants list - always include local participant first, then remote participants
  const participants = useMemo(() => {
    const participantMap = new Map<
      string,
      { participant: Participant; cameraTrack?: TrackReference; audioTrack?: TrackReference; isLocal: boolean }
    >();

    // Always add local participant first
    if (localParticipant) {
      const localCameraTrack = visibleCameraTracks.find(
        (track) => track.participant.identity === localParticipant.identity,
      );
      const localAudioTrack = visibleAudioTracks.find(
        (track) => track.participant.identity === localParticipant.identity,
      );
      participantMap.set(localParticipant.identity, {
        participant: localParticipant,
        cameraTrack: localCameraTrack,
        audioTrack: localAudioTrack,
        isLocal: true,
      });
    }

    // Add participants with camera tracks
    visibleCameraTracks.forEach((track) => {
      const participantId = track.participant.identity;
      if (!participantMap.has(participantId)) {
        participantMap.set(participantId, {
          participant: track.participant,
          cameraTrack: track,
          audioTrack: visibleAudioTracks.find((audioTrack) => audioTrack.participant.identity === participantId),
          isLocal: false,
        });
      }
    });

    // Add all remote participants that don't have camera tracks
    remoteParticipants.forEach((participant) => {
      const participantName = participant.name || participant.identity || "";
      if (!shouldFilterParticipant(participantName)) {
        const participantId = participant.identity;
        if (!participantMap.has(participantId)) {
          participantMap.set(participantId, {
            participant: participant,
            cameraTrack: undefined,
            audioTrack: visibleAudioTracks.find((audioTrack) => audioTrack.participant.identity === participantId),
            isLocal: false,
          });
        }
      }
    });

    return Array.from(participantMap.values());
  }, [visibleCameraTracks, visibleAudioTracks, remoteParticipants, localParticipant]);

  // Get screen share tracks (including local) - don't filter by participant name for screen shares
  const activeScreenShare = useMemo(() => {
    // Screen shares might come from any participant, don't filter them
    return screenShareTracks.length > 0 ? screenShareTracks[0] : null;
  }, [screenShareTracks]);

  // Get the user's display name for local participant
  const localUserName = user ? `${user.first_name} ${user.last_name}` : "You";

  // If there's a screen share, show focus layout (screen share center, participants on side)
  if (activeScreenShare) {
    const screenShareOwnerName =
      activeScreenShare.participant.identity === localParticipant?.identity ?
        localUserName
      : cleanParticipantName(activeScreenShare.participant.name || activeScreenShare.participant.identity);

    return (
      <div className="flex flex-row gap-4 h-full">
        {/* Participants carousel on the side */}
        <aside className="flex flex-col gap-3 w-52 shrink-0 overflow-visible">
          {participants.map(({ participant, cameraTrack, isLocal }) => (
            <ParticipantCard
              key={participant.identity}
              participant={participant}
              cameraTrack={cameraTrack}
              isLocal={isLocal}
              localUserName={localUserName}
              compact={true}
            />
          ))}
        </aside>
        {/* Screen share takes focus with cursor overlay */}
        <ScreenShareView screenShareTrack={activeScreenShare} ownerName={screenShareOwnerName} />
      </div>
    );
  }

  // No screen share - show participants in grid, centered
  return (
    <div className="h-full flex items-center justify-center">
      <div className={`grid ${getGridCols(participants.length)} gap-4 w-full max-w-5xl`}>
        {participants.map(({ participant, cameraTrack, isLocal }) => (
          <ParticipantCard
            key={participant.identity}
            participant={participant}
            cameraTrack={cameraTrack}
            isLocal={isLocal}
            localUserName={localUserName}
            compact={false}
          />
        ))}
      </div>
    </div>
  );
}

// Hand-picked colors for cursor badges (same as tauri app)
const SVG_BADGE_COLORS = ["#0040FF", "#7CCF00", "#615FFF", "#009689", "#C800DE", "#00A6F4", "#FFB900", "#ED0040"];

function ScreenShareView({ screenShareTrack, ownerName }: { screenShareTrack: TrackReference; ownerName: string }) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  // Cursor slots for remote participants
  const [cursorSlots, setCursorSlots] = useState<CursorSlot[]>(() =>
    Array.from({ length: SVG_BADGE_COLORS.length }, () => ({
      participantId: null,
      participantName: "Unknown",
      x: -1000,
      y: -1000,
      lastActivity: Date.now(),
    })),
  );

  // Calculate cursor position accounting for object-contain letterboxing
  // The video element uses object-contain, so the actual video content may be
  // smaller than the element with letterboxing (black bars)
  const calculateCursorPosition = useCallback((payload: TPMouseMove): { x: number; y: number } | null => {
    if (!videoRef.current || !containerRef.current) return null;

    const video = videoRef.current;
    const containerRect = containerRef.current.getBoundingClientRect();

    // Get the original video dimensions (the actual video content size)
    const videoWidth = video.videoWidth;
    const videoHeight = video.videoHeight;

    if (!videoWidth || !videoHeight) return null;

    // Get the video element's rendered dimensions
    const elementWidth = video.clientWidth;
    const elementHeight = video.clientHeight;

    // Calculate aspect ratios
    const videoAspect = videoWidth / videoHeight;
    const elementAspect = elementWidth / elementHeight;

    // Calculate the actual rendered video size within the element (accounting for object-contain)
    let renderedWidth: number, renderedHeight: number, videoOffsetX: number, videoOffsetY: number;

    if (videoAspect > elementAspect) {
      // Video is wider than element - letterboxing on top/bottom
      renderedWidth = elementWidth;
      renderedHeight = elementWidth / videoAspect;
      videoOffsetX = 0;
      videoOffsetY = (elementHeight - renderedHeight) / 2;
    } else {
      // Video is taller than element - letterboxing on left/right
      renderedHeight = elementHeight;
      renderedWidth = elementHeight * videoAspect;
      videoOffsetX = (elementWidth - renderedWidth) / 2;
      videoOffsetY = 0;
    }

    // Calculate video element's position within the container
    const videoRect = video.getBoundingClientRect();
    const containerOffsetX = videoRect.left - containerRect.left;
    const containerOffsetY = videoRect.top - containerRect.top;

    // Calculate cursor position:
    // 1. payload.x/y are 0-1 relative coordinates within the video content
    // 2. Multiply by rendered video size to get position within the rendered content
    // 3. Add letterbox offset (position of content within video element)
    // 4. Add container offset (position of video element within container)
    const x = payload.payload.x * renderedWidth + videoOffsetX + containerOffsetX;
    const y = payload.payload.y * renderedHeight + videoOffsetY + containerOffsetY;

    return { x, y };
  }, []);

  // Listen for cursor position updates from other participants
  useDataChannel(CURSORS_TOPIC, (msg) => {
    const decoder = new TextDecoder();
    const payload: TPMouseMove = JSON.parse(decoder.decode(msg.payload));

    const position = calculateCursorPosition(payload);
    if (!position) return;

    const participantName = msg.from?.name ?? "Unknown";
    const participantId = msg.from?.identity ?? "Unknown";

    if (participantId === "Unknown") return;

    setCursorSlots((prev) => {
      const updated = [...prev];

      // Find existing slot for this participant
      let slotIndex = updated.findIndex((slot) => slot.participantId === participantId);

      // If not found, find first available slot
      if (slotIndex === -1) {
        slotIndex = updated.findIndex((slot) => slot.participantId === null);
      }

      if (slotIndex === -1) return prev; // No available slots

      let name = updated[slotIndex]?.participantName ?? "Unknown";
      if (name === "Unknown") {
        name = participantName.split(" ")[0] ?? "Unknown";
      }

      updated[slotIndex] = {
        participantId,
        participantName: name,
        x: position.x,
        y: position.y,
        lastActivity: Date.now(),
      };

      return updated;
    });
  });

  // Hide cursors after 5 seconds of inactivity
  useEffect(() => {
    const interval = setInterval(() => {
      const now = Date.now();
      setCursorSlots((prev) =>
        prev.map((slot) => {
          if (slot.participantId && now - slot.lastActivity > 5000) {
            return { ...slot, x: -1000, y: -1000 };
          }
          return slot;
        }),
      );
    }, 1000);

    return () => clearInterval(interval);
  }, []);

  return (
    <div className="flex-1 flex items-center justify-center min-w-0 min-h-0">
      <div
        ref={containerRef}
        className="w-full h-full max-h-full rounded-lg overflow-hidden bg-slate-600 relative flex items-center justify-center"
      >
        <VideoTrack trackRef={screenShareTrack} className="max-w-full max-h-full object-contain" ref={videoRef} />

        {/* Render remote participant cursors */}
        {cursorSlots.map((slot, index) => {
          if (slot.x < 0 || slot.y < 0) return null;
          const color = SVG_BADGE_COLORS[index % SVG_BADGE_COLORS.length];
          return (
            <Cursor
              key={index}
              name={slot.participantName}
              color={color}
              style={{
                left: `${slot.x}px`,
                top: `${slot.y}px`,
              }}
            />
          );
        })}

        {/* Screen share label */}
        <div className="absolute bottom-3 left-3 bg-black/60 text-white text-xs px-2 py-1 rounded flex items-center gap-2">
          <LuScreenShare className="size-3" />
          <span>{ownerName}&apos;s screen</span>
        </div>
      </div>
    </div>
  );
}

function ParticipantCard({
  participant,
  cameraTrack,
  isLocal,
  localUserName,
  compact = false,
}: {
  participant: Participant;
  cameraTrack?: TrackReference;
  isLocal: boolean;
  localUserName: string;
  compact?: boolean;
}) {
  const rawParticipantName = participant.name || participant.identity || "Unknown";
  const participantName = isLocal ? localUserName : cleanParticipantName(rawParticipantName);

  // Check if participant is muted
  const isMicMuted = !participant.isMicrophoneEnabled;

  return (
    <div
      className={clsx("relative rounded-lg bg-slate-400 aspect-video overflow-hidden", {
        "outline-2 outline-green-500 -outline-offset-1": participant.isSpeaking,
      })}
    >
      {cameraTrack ?
        <VideoTrack trackRef={cameraTrack} className="w-full h-full object-cover" />
      : <div className="w-full h-full flex items-center justify-center bg-slate-600">
          <HiMiniUser className="size-10 text-white/80" />
        </div>
      }
      {/* Audio playback for remote participants */}
      {!isLocal && participant.audioTrackPublications.size > 0 && (
        <AudioTrack
          trackRef={{
            participant,
            source: Track.Source.Microphone,
            publication: Array.from(participant.audioTrackPublications.values())[0],
          }}
          volume={1.0}
        />
      )}
      {/* Participant metadata bar */}
      <div className="absolute bottom-0 left-0 right-0 bg-linear-to-t from-black/80 to-transparent p-2">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-1.5">
            {isMicMuted && <LuMicOff className="size-3 text-red-400" />}
            <span className={clsx("text-white text-xs font-medium", { "text-[10px]": compact })}>
              {participantName}
              {isLocal && " (You)"}
            </span>
          </div>
        </div>
      </div>
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
}: {
  hasAudioEnabled: boolean;
  setHasAudioEnabled: (enabled: boolean) => void;
  hasCameraEnabled: boolean;
  setHasCameraEnabled: (enabled: boolean) => void;
}) {
  const { localParticipant } = useLocalParticipant();
  useEffect(() => {
    if (!localParticipant) return;

    // Handle microphone - unpublish when disabled to fully release the device
    if (hasAudioEnabled) {
      localParticipant.setMicrophoneEnabled(true);
    } else {
      const micTrack = localParticipant
        .getTrackPublications()
        .find((track) => track.source === Track.Source.Microphone);
      if (micTrack && micTrack.track && micTrack.track instanceof LocalTrack) {
        localParticipant.unpublishTrack(micTrack.track);
      }
    }

    // Handle camera - unpublish when disabled to fully release the device
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
      const cameraTrack = localParticipant.getTrackPublications().find((track) => track.source === Track.Source.Camera);
      if (cameraTrack && cameraTrack.track && cameraTrack.track instanceof LocalTrack) {
        localParticipant.unpublishTrack(cameraTrack.track);
      }
    }
  }, [localParticipant, hasAudioEnabled, hasCameraEnabled]);

  return (
    <div className="flex flex-row gap-2 justify-center items-center flex-wrap">
      <AudioButton hasAudioEnabled={hasAudioEnabled} setHasAudioEnabled={setHasAudioEnabled} />
      <CameraButton hasCameraEnabled={hasCameraEnabled} setHasCameraEnabled={setHasCameraEnabled} />
      {/* Screen sharing is disabled in web-app for now - use the desktop app to share */}
      {/* <ScreenShareButton isScreenSharing={isScreenSharing} setIsScreenSharing={setIsScreenSharing} /> */}
      <EndCallButton />
    </div>
  );
}

function EndCallButton() {
  const room = useRoomContext();
  const navigate = useNavigate();

  const handleEndCall = useCallback(async () => {
    try {
      await room.disconnect();
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
