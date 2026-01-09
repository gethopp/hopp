import React, { useRef, useState, useEffect, useCallback } from "react";
import { useDataChannel } from "@livekit/components-react";
import { CursorParticipant } from "./cursor-participant";
import { Cursor } from "./cursor";
import { TPMouseMove } from "@/payloads";
import { getAbsolutePosition } from "@/lib/utils";
import { getOrAssignColor } from "@/windows/screensharing/utils";

const CURSORS_TOPIC = "participant_location";

interface RemoteCursorsProps {
  videoRef: React.RefObject<HTMLVideoElement>;
}

export const RemoteCursors = ({ videoRef }: RemoteCursorsProps) => {
  const cursorParticipantsRef = useRef<Map<string, CursorParticipant>>(new Map());
  const [, setUpdateTrigger] = useState(0);

  const forceUpdate = useCallback(() => setUpdateTrigger((prev) => prev + 1), []);

  const generateUniqueName = useCallback((participantName: string, existingNames: string[]): string => {
    let name = participantName.split(" ")[0] ?? "Unknown";
    let uniqueName = name;
    let fullName = participantName;
    let j = fullName.indexOf(" ") + 2;

    while (existingNames.includes(uniqueName) && j <= fullName.length) {
      uniqueName = fullName.slice(0, j);
      j++;
    }

    return uniqueName;
  }, []);

  useDataChannel(CURSORS_TOPIC, (msg) => {
    const decoder = new TextDecoder();
    const payload: TPMouseMove = JSON.parse(decoder.decode(msg.payload));

    if (!videoRef.current) return;

    const { absoluteX, absoluteY } = getAbsolutePosition(videoRef.current, payload);
    const participantName = msg.from?.name ?? "Unknown";
    const participantId = msg.from?.identity ?? "Unknown";

    if (participantId === "Unknown") return;

    const color = getOrAssignColor(participantId);
    let cursorParticipant = cursorParticipantsRef.current.get(participantId);

    if (!cursorParticipant) {
      const existingNames = Array.from(cursorParticipantsRef.current.values()).map((cp) => cp.participantName);
      const uniqueName = generateUniqueName(participantName, existingNames);

      cursorParticipant = new CursorParticipant(participantId, uniqueName, color, absoluteX, absoluteY);
      cursorParticipantsRef.current.set(participantId, cursorParticipant);
    } else {
      cursorParticipant.updatePosition(absoluteX, absoluteY);
    }

    forceUpdate();
  });

  // Hide cursors after 5 seconds of inactivity
  useEffect(() => {
    const interval = setInterval(() => {
      let shouldUpdate = false;
      cursorParticipantsRef.current.forEach((cursorParticipant) => {
        if (cursorParticipant.shouldHide(5000)) {
          cursorParticipant.hide();
          shouldUpdate = true;
        }
      });

      if (shouldUpdate) {
        forceUpdate();
      }
    }, 1000);

    return () => clearInterval(interval);
  }, [forceUpdate]);

  return (
    <>
      {Array.from(cursorParticipantsRef.current.values()).map((cursorParticipant) => (
        <Cursor
          key={cursorParticipant.participantId}
          name={cursorParticipant.participantName}
          color={cursorParticipant.color}
          style={{
            left: `${cursorParticipant.x}px`,
            top: `${cursorParticipant.y}px`,
          }}
        />
      ))}
    </>
  );
};
