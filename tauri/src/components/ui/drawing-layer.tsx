import React from "react";
import { useDataChannel } from "@livekit/components-react";
import { Draw } from "./draw";
import { DrawParticipant } from "./draw-participant";

const DRAW_TOPIC = "draw";

interface DrawingLayerProps {
  videoRef: React.RefObject<HTMLVideoElement>;
  drawParticipantsRef: React.MutableRefObject<Map<string, DrawParticipant>>;
  getOrAssignColor: (participantId: string) => string;
}

export const DrawingLayer = ({ videoRef, drawParticipantsRef, getOrAssignColor }: DrawingLayerProps) => {
  // Handle incoming DRAW_TOPIC messages for remote participants
  useDataChannel(DRAW_TOPIC, (msg) => {
    const decoder = new TextDecoder();
    const payloadStr = decoder.decode(msg.payload);
    let payload: any;
    try {
      payload = JSON.parse(payloadStr);
    } catch (e) {
      console.error("Failed to parse draw payload:", e);
      return;
    }

    const participantId = msg.from?.identity ?? "Unknown";
    if (participantId === "Unknown") return;

    const color = getOrAssignColor(participantId);
    let drawParticipant = drawParticipantsRef.current.get(participantId);
    if (!drawParticipant) {
      drawParticipant = new DrawParticipant(color, null);
      drawParticipantsRef.current.set(participantId, drawParticipant);
    }

    if (payload && payload.type) {
      switch (payload.type) {
        case "DrawStart":
          drawParticipant.handleDrawStart(payload.payload.point, payload.payload.path_id);
          break;
        case "DrawAddPoint":
          drawParticipant.handleDrawAddPoint(payload.payload);
          break;
        case "DrawEnd":
          drawParticipant.handleDrawEnd(payload.payload);
          break;
        case "DrawClearPath":
          drawParticipant.clearPath(payload.payload.path_id);
          break;
        case "DrawClearAllPaths":
          drawParticipant.clear();
          break;
        case "DrawingMode":
          drawParticipant.setDrawingMode(payload.payload);
          break;
      }
    }
  });

  return <Draw videoRef={videoRef} participants={drawParticipantsRef.current} />;
};
