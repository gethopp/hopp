import { AudioTrack, ParticipantTile, StartAudio, useLocalParticipant, useTracks } from "@livekit/components-react";
import { RemoteParticipant, Track } from "livekit-client";

const ListenToRemoteAudio = ({ muted = false }: { muted?: boolean }) => {
  const tracks = useTracks([Track.Source.Microphone], {
    onlySubscribed: true,
  });
  const { localParticipant } = useLocalParticipant();
  const localParticipantId = localParticipant.identity.split(":").slice(0, -1).join(":") || "";

  return (
    <>
      {tracks
        .filter((track) => track.participant instanceof RemoteParticipant)
        .filter((track) => {
          const participantId = track.participant.identity.split(":").slice(0, -1).join(":");
          return participantId !== localParticipantId;
        })
        .map((track) => (
          <ParticipantTile key={`${track.participant.identity}_${track.publication.trackSid}`} trackRef={track}>
            <StartAudio label="Click to allow audio playback" />
            <AudioTrack volume={1.0} muted={muted} />
          </ParticipantTile>
        ))}
    </>
  );
};

export default ListenToRemoteAudio;
