import ReactJson from "react-json-view";
import Draggable from "react-draggable";
import { throttle } from "lodash";
import { RiDraggable } from "react-icons/ri";
//import { LiveKitRoom, useDataChannel, useLocalParticipant, useTracks, VideoTrack } from "@livekit/components-react";
import { Track } from "livekit-client";
import React, { useEffect, useMemo, useRef, useState } from "react";
import { resizeWindow } from "./utils";
import { useSharingContext } from "@/windows/screensharing/context";
import { useResizeListener } from "@/lib/hooks";
import { cn, getAbsolutePosition, getRelativePosition } from "@/lib/utils";
import {
  TPKeystroke,
  TPMouseClick,
  TPMouseMove,
  TPMouseVisible,
  TPRemoteControlEnabled,
  TPWheelEvent,
} from "@/payloads";
import { useHover, useMouse } from "@uidotdev/usehooks";
import { DEBUGGING_VIDEO_TRACK } from "@/constants";
import { Cursor, SvgComponent } from "../ui/cursor";
import toast from "react-hot-toast";
import useStore from "@/store/store";
import { wsClient } from "./WSClient";
import { WebCodecsCanvas, drawI420FrameToCanvas } from "./WebCodecsCanvas";
import { drawI420FrameToCanvasWebGL, WebGLCanvas } from "./WebGLCanvas";

const CURSORS_TOPIC = "participant_location";
const PARTICIPANT_IN_CONTROL_TOPIC = "participant_in_control";
const REMOTE_CONTROL_ENABLED_TOPIC = "remote_control_enabled";

type SharingScreenProps = {
  serverURL: string;
  token: string;
  port: number;
};

const encoder = new TextEncoder();
// const decoder = new TextDecoder();

export function SharingScreen(props: SharingScreenProps) {
  const { serverURL, token, port } = props;
  console.log("sharing screen port", port);

  return (
    <ConsumerComponent port={port} />
  );
}

// Define cursor slot interface
interface CursorSlot {
  participantId: string | null;
  participantName: string;
  x: number;
  y: number;
  lastActivity: number;
}

const ConsumerComponent = React.memo(({ port }: { port: number }) => {
  // All state hooks first
  const [updateMouseControls, setUpdateMouseControls] = useState(false);

  // Hand-picked colors for the tailwind colors page:
  // https://tailwindcss.com/docs/colors
  const SVG_BADGE_COLORS = ["#7CCF00", "#615FFF", "#009689", "#C800DE", "#00A6F4", "#FFB900", "#ED0040"];
  // Pre-create 10 cursor slots, all hidden initially
  const [cursorSlots, setCursorSlots] = useState<CursorSlot[]>(() =>
    Array.from({ length: SVG_BADGE_COLORS.length }, (_, index) => ({
      participantId: null,
      participantName: "Unknown",
      x: -1000, // Position off-screen
      y: -1000, // Position off-screen
      lastActivity: Date.now(),
    })),
  );

  // All refs
  //const videoRef = useRef<HTMLVideoElement>(null);

  // All context hooks
  /* const tracks = useTracks([Track.Source.ScreenShare], {
    onlySubscribed: true,
  });
  const localParticipant = useLocalParticipant(); */
  let { isSharingMouse, isSharingKeyEvents, parentKeyTrap } = useSharingContext();
  const [wrapperRef, isMouseInside] = useHover();
  const { updateCallTokens } = useStore();
  const [mouse, mouseRef] = useMouse();

  // Boolean to control when to show custom cursor
  const [showCustomCursor, setShowCustomCursor] = useState(true);

  const videoRef = useRef<HTMLCanvasElement>(null);
  const heightRef = useRef(0);
  const widthRef = useRef(0);

  // Simple sliding window metrics (reset every 30 frames)
  const metrics = React.useRef({
    count: 0,
    sumCaptureToReceive: 0,
    sumReceiveToBeforeDraw: 0,
    headerLatency: 0,
  }).current;

  // Callback handlers for extended header data
  const handleRemoteControlChange = React.useCallback((enabled: boolean) => {
    updateCallTokens({
      isRemoteControlEnabled: enabled,
    });

    if (enabled) {
      toast("Sharer enabled remote control", {
        icon: "🔓",
        duration: 1500,
      });
    } else {
      toast("Sharer disabled remote control", {
        icon: "🔒",
        duration: 1500,
      });
    }
  }, [updateCallTokens]);

  const handleParticipantLocation = React.useCallback((x: number, y: number, sid: string) => {
    //console.log("Participant location:", x, y, sid);
    if (!videoRef.current) return;

    const canvas = videoRef.current;
    const rect = canvas.getBoundingClientRect();
    const absoluteX = (x * rect.width) + rect.left;
    const absoluteY = (y * rect.height) + rect.top;

    setCursorSlots((prev) => {
      const updated = [...prev];

      // Find existing slot for this participant
      let slotIndex = updated.findIndex((slot) => slot.participantId === sid);

      // If not found, find first available slot
      if (slotIndex === -1) {
        slotIndex = updated.findIndex((slot) => slot.participantId === null);
      }

      // Update the slot if available
      if (slotIndex !== -1) {
        updated[slotIndex] = {
          participantId: sid,
          participantName: sid.split("-")[0] || "Unknown",
          x: absoluteX,
          y: absoluteY,
          lastActivity: Date.now(),
        };
      }

      return updated;
    });
  }, []);

  const handleShowCustomCursor = React.useCallback((showCustomCursor: boolean) => {
    setShowCustomCursor(showCustomCursor);
  }, []);

  // Frame reconstruction state
  const frameBuffers = React.useRef<Map<number, {
    width: number;
    height: number;
    captureTs: number;
    chunksTotal: number;
    chunks: Map<number, Uint8Array>;
    frameData: Uint8Array | null;
    totalSize: number;
    receivedAt: number;
  }>>(new Map()).current;

  const onMessage = (data: ArrayBuffer, receivedAtMs?: number) => {
    const canvas = videoRef.current;
    if (!canvas) return;

    const dv = new DataView(data);
    const receivedAt = receivedAtMs ?? Date.now();

    if (data.byteLength < 1) return;

    // Read packet type (avoid string allocation)
    const packetTypeCode = dv.getUint8(0);

    if (packetTypeCode === 0x48) { // 'H' = 72 = 0x48
      // Header packet - minimum size is base header (18 bytes)
      if (data.byteLength < 18) return;

      const width = dv.getUint16(1, true);
      const height = dv.getUint16(3, true);
      const captureTs = Number(dv.getBigUint64(5, true));
      const frameId = dv.getUint32(13, true);
      const chunksTotal = dv.getUint8(17);

      if ((height !== heightRef.current) || (width !== widthRef.current)) {
        heightRef.current = height;
        widthRef.current = width;
      }

      // Parse extended header fields
      let offset = 18;

      // Remote control enabled flag and value
      if (offset < data.byteLength) {
        const hasRemoteControlEnabled = dv.getUint8(offset) === 1;
        offset++;
        if (hasRemoteControlEnabled && offset < data.byteLength) {
          const remoteControlEnabled = dv.getUint8(offset) === 1;
          handleRemoteControlChange(remoteControlEnabled);
        }
        offset++;
      }

      if (offset < data.byteLength) {
        const hasShowCustomCursor = dv.getUint8(offset) === 1;
        offset++;
        if (hasShowCustomCursor && offset <= data.byteLength) {
          const showCustomCursor = dv.getUint8(offset) === 1;
          handleShowCustomCursor(showCustomCursor);
        }
        offset++;
      }

      // Participant location
      let participantX: number | null = null;
      let participantY: number | null = null;
      let participantLocationSid: string | null = null;
      if (offset < data.byteLength) {
        const hasParticipantLocation = dv.getUint8(offset) === 1;
        offset++;
        //console.log("hasParticipantLocation:", hasParticipantLocation);
        if (hasParticipantLocation && offset + 16 <= data.byteLength) {
          participantX = dv.getFloat64(offset, true);
          offset += 8;
          participantY = dv.getFloat64(offset, true);
          offset += 8;

          const sidLength = dv.getUint32(offset, true);
          offset += 4;
          if (sidLength > 0 && offset + sidLength <= data.byteLength) {
            const sidBytes = new Uint8Array(data, offset, sidLength);
            participantLocationSid = new TextDecoder().decode(sidBytes);
            offset += sidLength;
          }
        }
      }

      //console.log("Participant location sid:", participantLocationSid);


      // Handle parsed extended data with callbacks
      if (participantX !== null && participantY !== null && participantLocationSid) {
        handleParticipantLocation(participantX, participantY, participantLocationSid);
      }

      // Pre-calculate frame size and pre-allocate buffer
      const ySize = width * height;
      const uvPlaneSize = ySize >> 2;
      const totalSize = ySize + uvPlaneSize + uvPlaneSize;

      // Initialize frame buffer with pre-allocated data
      frameBuffers.set(frameId, {
        width,
        height,
        captureTs,
        chunksTotal,
        chunks: new Map(),
        frameData: new Uint8Array(totalSize),
        totalSize,
        receivedAt,
      });


      metrics.headerLatency += receivedAt - captureTs;

    } else if (packetTypeCode === 0x44) { // 'D' = 68 = 0x44
      // Data packet
      if (data.byteLength < 11) return;

      const frameId = dv.getUint32(1, true);
      const chunkIndex = dv.getUint16(5, true);
      const chunkSize = dv.getUint32(7, true);

      if (data.byteLength < 11 + chunkSize) return;

      // Get frame buffer
      const frameBuffer = frameBuffers.get(frameId);
      if (!frameBuffer || !frameBuffer.frameData) return;

      // Store chunk data as a view into the original buffer - zero-copy
      const chunkData = new Uint8Array(data, 11, chunkSize);
      frameBuffer.chunks.set(chunkIndex, chunkData);

      // Check if frame is complete
      if (frameBuffer.chunks.size === frameBuffer.chunksTotal) {
        // Use cached values (avoid destructuring and recalculation)
        const width = frameBuffer.width;
        const height = frameBuffer.height;
        const captureTs = frameBuffer.captureTs;
        const ySize = width * height;
        const uvPlaneSize = ySize >> 2;

        // Combine chunks directly into pre-allocated buffer - zero-copy assembly
        let offset = 0;
        for (let i = 0; i < frameBuffer.chunksTotal; i++) {
          const chunk = frameBuffer.chunks.get(i);
          if (chunk) {
            frameBuffer.frameData!.set(chunk, offset);
            offset += chunk.length;
          }
        }

        // Create zero-copy subarrays directly from pre-allocated buffer
        const yData = frameBuffer.frameData!.subarray(0, ySize);
        const uData = frameBuffer.frameData!.subarray(ySize, ySize + uvPlaneSize);
        const vData = frameBuffer.frameData!.subarray(ySize + uvPlaneSize, ySize + uvPlaneSize + uvPlaneSize);

        // Update metrics
        const captureToReceiveMs = receivedAt - captureTs;
        metrics.count++;
        metrics.sumCaptureToReceive += captureToReceiveMs;

        // Draw frame
        //drawI420FrameToCanvasWebGL(canvas, yData, uData, vData, width, height, captureTs, (beforeDrawMs, afterDrawMs) => {
        drawI420FrameToCanvas(canvas, yData, uData, vData, width, height, captureTs, (beforeDrawMs, afterDrawMs) => {
          metrics.sumReceiveToBeforeDraw += beforeDrawMs - receivedAt;

          if (metrics.count % 30 === 0) {
            const n = metrics.count;
            console.log(
              "avg[30] capture->recv=%dms, receive->beforeDraw=%dms, headerLatency=%dms",
              Math.round(metrics.sumCaptureToReceive / n),
              Math.round(metrics.sumReceiveToBeforeDraw / n),
              Math.round(metrics.headerLatency / n),
            );
            metrics.count = 0;
            metrics.sumCaptureToReceive = 0;
            metrics.sumReceiveToBeforeDraw = 0;
            metrics.headerLatency = 0;
          }
        //}, false);
        });

        // Clean up completed frame
        frameBuffers.delete(frameId);
      }
    }

    // Clean up old incomplete frames less frequently (every 100th packet)
    if (metrics.count % 100 === 0) {
      const cutoffTime = receivedAt - 5000;
      for (const [frameId, frameBuffer] of frameBuffers.entries()) {
        if (frameBuffer.receivedAt < cutoffTime) {
          frameBuffers.delete(frameId);
        }
      }
    }
  };

  useEffect(() => {
    if (port !== 0) {
      wsClient.connect(`ws://localhost:${port}`, onMessage);
      return () => {
        wsClient.disconnect();
      };
    }
  }, [port]);


  // Data channel hooks - must be called unconditionally
  //const { message: latestMessage, send } = useDataChannel(CURSORS_TOPIC, (msg) => {
  //  const decoder = new TextDecoder();
  //  const payload: TPMouseMove = JSON.parse(decoder.decode(msg.payload));

  //  if (!videoRef.current) return;

  //  const { absoluteX, absoluteY } = getAbsolutePosition(videoRef.current, payload);

  //  const participantName = msg.from?.name ?? "Unknown";
  //  const participantId = msg.from?.identity ?? "Unknown";

  //  /* We need the id to be unique for each participant */
  //  if (participantId === "Unknown") return;

  //  /*
  //   * We are keeping it simple for now and just set a slot to a participant
  //   * the first time they move their mouse.
  //   *
  //   * The problem with this approach is
  //   * that we might exhaust the number of available colors and just
  //   * circling through them, this can happen in the following scenario:
  //   *  - 10 participants join the call
  //   *  - 10 moved their mouse
  //   *  - 1 disconnected
  //   *  - Another joined
  //   *  - The new participant can't find a slot.
  //   *
  //   * To avoid this, we just use 20 available slots for now.
  //   */
  //  setCursorSlots((prev) => {
  //    const updated = [...prev];

  //    // Find existing slot for this participant
  //    let slotIndex = updated.findIndex((slot) => slot.participantId === participantId);

  //    // If not found, find first available slot
  //    if (slotIndex === -1) {
  //      slotIndex = updated.findIndex((slot) => slot.participantId === null);
  //    }

  //    let name = updated[slotIndex]?.participantName ?? "Unknown";
  //    // Update the slot
  //    if (slotIndex !== -1) {
  //      if (name === "Unknown") {
  //        name = participantName.split(" ")[0] ?? "Unknown";
  //        // If a name already exists, start adding characters until they don't match
  //        let uniqueName = name;
  //        let fullName = participantName;
  //        let j = fullName.indexOf(" ") + 2;
  //        while (
  //          updated.slice(0, slotIndex).some((slot) => slot?.participantName === uniqueName) &&
  //          j <= fullName.length
  //        ) {
  //          uniqueName = fullName.slice(0, j);
  //          j++;
  //        }
  //        name = uniqueName;
  //      }

  //      updated[slotIndex] = {
  //        participantId,
  //        participantName: name,
  //        x: absoluteX,
  //        y: absoluteY,
  //        lastActivity: Date.now(),
  //      };
  //    }

  //    return updated;
  //  });
  //});

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

  //const streamWidth = track?.publication.dimensions?.width || 16;
  //const streamHeight = track?.publication.dimensions?.height || 9;
  const aspectRatio = widthRef.current / heightRef.current;

  const throttledResize = useMemo(
    () =>
      throttle(() => {
        resizeWindow(widthRef.current, heightRef.current, videoRef);
      }, 250),
    [widthRef.current, heightRef.current, videoRef],
  );
  useResizeListener(throttledResize);

  useEffect(() => {
    if (videoRef.current) {
      resizeWindow(widthRef.current, heightRef.current, videoRef);
    }
  }, [widthRef.current, heightRef.current, videoRef]);

  /*
   * We do this because we need a way to retrigger the useEffect below,
   * adding the videoRef.current to the dependency array doesn't work because
   * because changing a ref doesn't cause a re-render.
   * see https://www.epicreact.dev/why-you-shouldnt-put-refs-in-a-dependency-array.
   */

  useMemo(() => {
    setUpdateMouseControls(!updateMouseControls);
  }, [videoRef.current]);

  /**
   * Mouse sharing logic
   */
  useEffect(() => {
    const videoElement = videoRef.current;

    const handleMouseMove = (e: MouseEvent) => {
      if (videoElement) {
        const { relativeX, relativeY } = getRelativePosition(videoElement, e);
        // console.debug(`Mouse moving 🚶: relativeX: ${relativeX}, relativeY: ${relativeY}`);

        const payload: TPMouseMove = {
          type: "MouseMove",
          payload: { x: relativeX, y: relativeY, pointer: true },
        };

        wsClient.send(encoder.encode(JSON.stringify(payload)).buffer);
      }
    };

    const handleMouseDown = (e: MouseEvent) => {
      if (videoElement) {
        const { relativeX, relativeY } = getRelativePosition(videoElement, e);
        // console.debug(`Clicking down 🖱️: relativeX: ${relativeX}, relativeY: ${relativeY}, detail ${e.detail}`);

        const payload: TPMouseClick = {
          type: "MouseClick",
          payload: {
            x: relativeX,
            y: relativeY,
            button: e.button,
            clicks: e.detail,
            down: true,
            shift: e.shiftKey,
            alt: e.altKey,
            ctrl: e.ctrlKey,
            meta: e.metaKey,
          },
        };

        wsClient.send(encoder.encode(JSON.stringify(payload)).buffer);
      }
    };

    const handleMouseUp = (e: MouseEvent) => {
      if (videoElement) {
        const { relativeX, relativeY } = getRelativePosition(videoElement, e);
        // console.debug(`Clicking up 🖱️: relativeX: ${relativeX}, relativeY: ${relativeY}, detail ${e.detail}`);

        const payload: TPMouseClick = {
          type: "MouseClick",
          payload: {
            x: relativeX,
            y: relativeY,
            button: e.button,
            clicks: e.detail,
            down: false,
            shift: e.shiftKey,
            alt: e.altKey,
            ctrl: e.ctrlKey,
            meta: e.metaKey,
          },
        };

        wsClient.send(encoder.encode(JSON.stringify(payload)).buffer);
      }
    };

    const handleContextMenu = (e: MouseEvent) => {
      e.preventDefault();
    };

    const handleWheel = (e: WheelEvent) => {
      if (videoElement) {
        // Solve natural flow of the wheel
        // Source: https://stackoverflow.com/a/23668035
        var deltaY = e.deltaY;
        var deltaX = e.deltaX;
        //@ts-ignore
        if (e.webkitDirectionInvertedFromDevice) {
          deltaY = -deltaY;
          deltaX = -deltaX;
        }

        const payload: TPWheelEvent = {
          type: "WheelEvent",
          payload: { deltaX: deltaX, deltaY: deltaY },
        };

        wsClient.send(encoder.encode(JSON.stringify(payload)).buffer);
      }
    };

    // Send mouse visible data
    if (videoElement) {
      const payload: TPMouseVisible = {
        type: "MouseVisible",
        payload: { visible: isSharingMouse },
      };
      wsClient.send(encoder.encode(JSON.stringify(payload)).buffer);
    }

    if (videoElement) {
      videoElement.addEventListener("mousemove", handleMouseMove);
    }

    if (videoElement && isSharingMouse) {
      videoElement.addEventListener("wheel", handleWheel);
      videoElement.addEventListener("mousedown", handleMouseDown);
      videoElement.addEventListener("mouseup", handleMouseUp);
      videoElement.addEventListener("contextmenu", handleContextMenu);
    }

    return () => {
      if (videoElement) {
        videoElement.removeEventListener("mousemove", handleMouseMove);
        videoElement.removeEventListener("wheel", handleWheel);
        videoElement.removeEventListener("mousedown", handleMouseDown);
        videoElement.removeEventListener("mouseup", handleMouseUp);
        videoElement.removeEventListener("contextmenu", handleContextMenu);
      }
    };
  }, [isSharingMouse, updateMouseControls]);

  /**
   * Keyboard sharing logic
   *
   * On the first render, set the keyParentTrap
   * to listen to the keyboard events and if the keyboard event is triggered
   * while the mouse is inside the video element, and the sharing key events is enabled
   * then we will send the keystroke to the server
   */
  useEffect(() => {
    if (!parentKeyTrap) return;
    // console.debug(`isMouseInside: ${isMouseInside}, isSharingKeyEvents: ${isSharingKeyEvents}`);

    const handleKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      if (isMouseInside && isSharingKeyEvents) {
        e.preventDefault();
        /*
         * Hack to handle dead quote key, this
         * list should be updated with other dead keys as they are
         * reported to us.
         */
        let key = e.key as string;
        if (key === "Dead") {
          if (e.code === "Quote") {
            key = e.shiftKey ? '"' : "'";
          } else if (e.code === "Backquote") {
            key = e.shiftKey ? "~" : "`";
          } else if (e.code === "Digit6" && e.shiftKey) {
            key = "^";
          } else if (e.code === "KeyU" && e.altKey) {
            key = "¨";
          }
        }
        const payload: TPKeystroke = {
          type: "Keystroke",
          payload: {
            key: [key],
            meta: e.metaKey,
            alt: e.altKey,
            ctrl: e.ctrlKey,
            shift: e.shiftKey,
            down: true,
          },
        };

        // console.debug("Sending keystroke", payload);

        wsClient.send(encoder.encode(JSON.stringify(payload)).buffer);
      }
    };
    const handleKeyUp = (e: KeyboardEvent) => {
      e.preventDefault();
      if (isMouseInside && isSharingKeyEvents) {
        e.preventDefault();
        /*
         * Hack to handle dead quote key, this
         * list should be updated with other dead keys as they are
         * reported to us.
         */
        let key = e.key as string;
        if (key === "Dead") {
          if (e.code === "Quote") {
            key = e.shiftKey ? '"' : "'";
          } else if (e.code === "Backquote") {
            key = e.shiftKey ? "~" : "`";
          } else if (e.code === "Digit6" && e.shiftKey) {
            key = "^";
          } else if (e.code === "KeyU" && e.altKey) {
            key = "¨";
          }
        }
        const payload: TPKeystroke = {
          type: "Keystroke",
          payload: {
            key: [key],
            meta: e.metaKey,
            alt: e.altKey,
            ctrl: e.ctrlKey,
            shift: e.shiftKey,
            down: false,
          },
        };

        // console.debug("Sending keystroke", payload);

        wsClient.send(encoder.encode(JSON.stringify(payload)).buffer);
      }
    };

    parentKeyTrap.addEventListener("keydown", handleKeyDown);
    parentKeyTrap.addEventListener("keyup", handleKeyUp);

    return () => {
      parentKeyTrap?.removeEventListener("keydown", handleKeyDown);
      parentKeyTrap?.removeEventListener("keyup", handleKeyUp);
    };
  }, [isMouseInside, isSharingKeyEvents, parentKeyTrap]);

  //useEffect(() => {
  //  // TODO: remove and make this enabled only on debug mode
  //  // Enable BigInt serialization for JSON viewer
  //  if (DEBUGGING_VIDEO_TRACK) {
  //    // @ts-ignore
  //    BigInt.prototype.toJSON = function () {
  //      return this.toString();
  //    };
  //  }
  //}, [track]);

  //if (!track) {
  //  return <div>No screen share track available yet</div>;
  //}

  return (
    <div
      ref={wrapperRef}
      className={cn("w-full screenshare-video rounded-lg overflow-hidden border-solid border-2 relative", {
        "screenshare-video-focus": isMouseInside,
        "border-slate-200": !isMouseInside,
      })}
      tabIndex={-1}
    >
      {/* {DEBUGGING_VIDEO_TRACK && (
        <div className="w-full h-full absolute top-0 left-0 z-10">
          <Draggable axis="both" handle=".handle" defaultPosition={{ x: 0, y: 0 }} grid={[25, 25]} scale={1}>
            <div className="w-[300px] h-[250px] bg-slate-200 overflow-auto rounded-md p-2">
              <div className="handle mb-2">
                <RiDraggable className="size-6" />{" "}
              </div>
              <ReactJson src={track.publication?.trackInfo || {}} collapsed={true} />
            </div>
          </Draggable>
        </div>
      )} */}
      <WebCodecsCanvas
        ref={videoRef}
        className="w-full h-full"
        style={{ display: 'block' }}
      />
      {/* <WebGLCanvas
        ref={videoRef}
        className="w-full h-full"
        style={{ display: 'block' }}
      /> */}
      {cursorSlots.map((slot, index) => {
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


      {/* Custom cursor rendered at mouse position */}
      {/* {showCustomCursor && mouse.x !== null && mouse.y !== null && (
        <div
          className="absolute pointer-events-none z-50"
          style={{
            left: `${mouse.x - (videoRef.current?.getBoundingClientRect().left || 0) - 4}px`,
            top: `${mouse.y - (videoRef.current?.getBoundingClientRect().top || 0) - 4}px`,
          }}
        >
          <SvgComponent color="#3B82F6" />
        </div>
      )} */}
    </div>
  );
});
