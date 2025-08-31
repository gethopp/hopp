import ReactJson from "react-json-view";
import Draggable from "react-draggable";
import { throttle } from "lodash";
import { RiDraggable } from "react-icons/ri";
import { LiveKitRoom, useDataChannel, useLocalParticipant, useTracks, VideoTrack } from "@livekit/components-react";
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

const CURSORS_TOPIC = "participant_location";
const PARTICIPANT_IN_CONTROL_TOPIC = "participant_in_control";

type SharingScreenProps = {
  serverURL: string;
  token: string;
};

const encoder = new TextEncoder();
// const decoder = new TextDecoder();

export function SharingScreen(props: SharingScreenProps) {
  const { serverURL, token } = props;

  return (
    <LiveKitRoom token={token} serverUrl={serverURL} connect={true}>
      <ConsumerComponent />
    </LiveKitRoom>
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

const ConsumerComponent = React.memo(() => {
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
  const videoRef = useRef<HTMLVideoElement>(null);

  // All context hooks
  const tracks = useTracks([Track.Source.ScreenShare], {
    onlySubscribed: true,
  });
  const localParticipant = useLocalParticipant();
  let { isSharingMouse, isSharingKeyEvents, parentKeyTrap } = useSharingContext();
  const [wrapperRef, isMouseInside] = useHover();
  const { updateCallTokens } = useStore();
  const [mouse, mouseRef] = useMouse();

  // Boolean to control when to show custom cursor
  const [showCustomCursor, setShowCustomCursor] = useState(true);

  // Data channel hooks - must be called unconditionally
  const { message: latestMessage, send } = useDataChannel(CURSORS_TOPIC, (msg) => {
    const decoder = new TextDecoder();
    const payload: TPMouseMove = JSON.parse(decoder.decode(msg.payload));

    if (!videoRef.current) return;

    const { absoluteX, absoluteY } = getAbsolutePosition(videoRef.current, payload);

    const participantName = msg.from?.name ?? "Unknown";
    const participantId = msg.from?.identity ?? "Unknown";

    /* We need the id to be unique for each participant */
    if (participantId === "Unknown") return;

    /*
     * We are keeping it simple for now and just set a slot to a participant
     * the first time they move their mouse.
     *
     * The problem with this approach is
     * that we might exhaust the number of available colors and just
     * circling through them, this can happen in the following scenario:
     *  - 10 participants join the call
     *  - 10 moved their mouse
     *  - 1 disconnected
     *  - Another joined
     *  - The new participant can't find a slot.
     *
     * To avoid this, we just use 20 available slots for now.
     */
    setCursorSlots((prev) => {
      const updated = [...prev];

      // Find existing slot for this participant
      let slotIndex = updated.findIndex((slot) => slot.participantId === participantId);

      // If not found, find first available slot
      if (slotIndex === -1) {
        slotIndex = updated.findIndex((slot) => slot.participantId === null);
      }

      let name = updated[slotIndex]?.participantName ?? "Unknown";
      // Update the slot
      if (slotIndex !== -1) {
        if (name === "Unknown") {
          name = participantName.split(" ")[0] ?? "Unknown";
          // If a name already exists, start adding characters until they don't match
          let uniqueName = name;
          let fullName = participantName;
          let j = fullName.indexOf(" ") + 2;
          while (
            updated.slice(0, slotIndex).some((slot) => slot?.participantName === uniqueName) &&
            j <= fullName.length
          ) {
            uniqueName = fullName.slice(0, j);
            j++;
          }
          name = uniqueName;
        }

        updated[slotIndex] = {
          participantId,
          participantName: name,
          x: absoluteX,
          y: absoluteY,
          lastActivity: Date.now(),
        };
      }

      return updated;
    });
  });

  useDataChannel("remote_control_enabled", (msg) => {
    const decoder = new TextDecoder();
    const payload: TPRemoteControlEnabled = JSON.parse(decoder.decode(msg.payload));
    if (payload.payload.enabled == false) {
      updateCallTokens({
        isRemoteControlEnabled: false,
      });
      toast("Sharer disabled remote control", {
        icon: "🔒",
        duration: 1500,
      });
    } else {
      updateCallTokens({
        isRemoteControlEnabled: true,
      });
      toast("Sharer enabled remote control", {
        icon: "🔓",
        duration: 1500,
      });
    }
  });

  useDataChannel(PARTICIPANT_IN_CONTROL_TOPIC, (msg) => {
    const decoder = new TextDecoder();
    const payload = decoder.decode(msg.payload);
    if (payload === localParticipant.localParticipant?.sid) {
      setShowCustomCursor(false);
    } else {
      setShowCustomCursor(true);
    }
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

  /**
   * Currently returning the last screen share track
   * If there are multiple screen share tracks, and some are "white"
   * but out of order we need to use stats about the last updated ones.
   *
   * The `prevStats` includes the stats of the last updated screen share track
   * but they are private data.
   *
   * Also the track's playback delay is set to 0 to have lower latency.
   */
  const track = useMemo(() => {
    if (tracks.length === 0) return null;
    console.info(`Tracks: `, tracks);

    return tracks[tracks.length - 1];
  }, [tracks]);

  const streamWidth = track?.publication.dimensions?.width || 16;
  const streamHeight = track?.publication.dimensions?.height || 9;
  const aspectRatio = streamWidth / streamHeight;

  const throttledResize = useMemo(
    () =>
      throttle(() => {
        resizeWindow(streamWidth, streamHeight, videoRef);
      }, 250),
    [streamWidth, streamHeight, videoRef],
  );
  useResizeListener(throttledResize);

  useEffect(() => {
    if (videoRef.current && track) {
      resizeWindow(streamWidth, streamHeight, videoRef);
    }
  }, [track, streamWidth, streamHeight]);

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

        localParticipant.localParticipant?.publishData(encoder.encode(JSON.stringify(payload)), {
          reliable: true,
          topic: CURSORS_TOPIC,
        });
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

        localParticipant.localParticipant?.publishData(encoder.encode(JSON.stringify(payload)), { reliable: true });
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

        localParticipant.localParticipant?.publishData(encoder.encode(JSON.stringify(payload)), { reliable: true });
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

        localParticipant.localParticipant?.publishData(encoder.encode(JSON.stringify(payload)), { reliable: true });
      }
    };

    // Send mouse visible data
    if (videoElement) {
      const payload: TPMouseVisible = {
        type: "MouseVisible",
        payload: { visible: isSharingMouse },
      };
      localParticipant.localParticipant?.publishData(encoder.encode(JSON.stringify(payload)), { reliable: true });
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

        localParticipant.localParticipant?.publishData(encoder.encode(JSON.stringify(payload)), { reliable: true });
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

        localParticipant.localParticipant?.publishData(encoder.encode(JSON.stringify(payload)), { reliable: true });
      }
    };

    parentKeyTrap.addEventListener("keydown", handleKeyDown);
    parentKeyTrap.addEventListener("keyup", handleKeyUp);

    return () => {
      parentKeyTrap?.removeEventListener("keydown", handleKeyDown);
      parentKeyTrap?.removeEventListener("keyup", handleKeyUp);
    };
  }, [isMouseInside, isSharingKeyEvents, parentKeyTrap]);

  useEffect(() => {
    // TODO: remove and make this enabled only on debug mode
    // Enable BigInt serialization for JSON viewer
    if (DEBUGGING_VIDEO_TRACK) {
      // @ts-ignore
      BigInt.prototype.toJSON = function () {
        return this.toString();
      };
    }
  }, [track]);

  if (!track) {
    return <div>No screen share track available yet</div>;
  }

  return (
    <div
      ref={wrapperRef}
      className={cn("w-full screenshare-video rounded-lg overflow-hidden border-solid border-2 relative", {
        "screenshare-video-focus": isMouseInside,
        "border-slate-200": !isMouseInside,
      })}
      tabIndex={-1}
    >
      {DEBUGGING_VIDEO_TRACK && (
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
      )}
      <VideoTrack
        {...track}
        className={"personal-cursor"}
        trackRef={track}
        ref={videoRef}
        style={{
          aspectRatio: `${aspectRatio}`,
          width: "100%",
          cursor: showCustomCursor ? "none" : "default",
        }}
      />

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
      {showCustomCursor && mouse.x !== null && mouse.y !== null && (
        <div
          className="absolute pointer-events-none z-50"
          style={{
            left: `${mouse.x - (videoRef.current?.getBoundingClientRect().left || 0) - 4}px`,
            top: `${mouse.y - (videoRef.current?.getBoundingClientRect().top || 0) - 4}px`,
          }}
        >
          <SvgComponent color="#3B82F6" />
        </div>
      )}
    </div>
  );
});
