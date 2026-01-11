import ReactJson from "react-json-view";
import Draggable from "react-draggable";
import { throttle } from "lodash";
import { RiDraggable } from "react-icons/ri";
import { HiPencil } from "react-icons/hi2";
import { LiveKitRoom, useDataChannel, useLocalParticipant, useRoomContext, useTracks, VideoTrack } from "@livekit/components-react";
import { ConnectionState, DataPublishOptions, LocalParticipant, Track } from "livekit-client";
import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { resizeWindow } from "./utils";
import { useSharingContext } from "@/windows/screensharing/context";
import { useResizeListener } from "@/lib/hooks";
import { cn, getRelativePosition, applyCursorRippleEffect } from "@/lib/utils";
import {
  TPAddToClipboard,
  TPKeystroke,
  TPMouseClick,
  TPMouseMove,
  TPMouseVisible,
  TPPasteFromClipboard,
  TPRemoteControlEnabled,
  TPWheelEvent,
  TPDrawStart,
  TPDrawAddPoint,
  TPDrawEnd,
  TPDrawingModeEvent,
  TPDrawClearPath,
  TPDrawClearAllPaths,
  TPClickAnimation,
} from "@/payloads";
import { useHover, useMouse } from "@uidotdev/usehooks";
import { DEBUGGING_VIDEO_TRACK, OS } from "@/constants";
import { SvgComponent } from "../ui/cursor";
import { DrawingLayer } from "../ui/drawing-layer";
import { DrawParticipant } from "../ui/draw-participant";
import { RemoteCursors } from "../ui/remote-cursors";
import toast from "react-hot-toast";
import useStore from "@/store/store";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { getNextPathId, SVG_BADGE_COLORS } from "@/windows/screensharing/utils";

enum Topics {
  CURSORS = "participant_location",
  PARTICIPANT_IN_CONTROL = "participant_in_control",
  DRAW = "draw",
}

const LOCAL_PARTICIPANT_ID = "local";

type SharingScreenProps = {
  serverURL: string;
  token: string;
};

const encoder = new TextEncoder();

export const publishLocalParticipantData = (
  localParticipant: LocalParticipant | undefined,
  payload: any,
  topic: Topics,
  settings?: DataPublishOptions,
): Promise<void> => {
  if (!localParticipant) {
    return Promise.resolve();
  }

  return localParticipant?.publishData(encoder.encode(JSON.stringify(payload)), {
    reliable: true,
    topic: topic,
    ...settings,
  });
};

export function SharingScreen(props: SharingScreenProps) {
  const { serverURL, token } = props;

  return (
    <LiveKitRoom token={token} serverUrl={serverURL} connect={true}>
      <ConsumerComponent />
    </LiveKitRoom>
  );
}

const ConsumerComponent = React.memo(() => {
  // All state hooks first
  const [updateMouseControls, setUpdateMouseControls] = useState(false);

  // All refs
  const videoRef = useRef<HTMLVideoElement>(null);

  // All context hooks
  const tracks = useTracks([Track.Source.ScreenShare], {
    onlySubscribed: true,
  });
  const localParticipant = useLocalParticipant();
  const { state: roomState } = useRoomContext();
  const {
    isSharingMouse,
    isSharingKeyEvents,
    drawingMode,
    parentKeyTrap,
    setStreamDimensions,
    rightClickToClear,
    clearDrawingsSignal,
  } = useSharingContext();
  const isDrawingMode = drawingMode.type !== "Disabled";
  const [wrapperRef, isMouseInside] = useHover();
  const { updateCallTokens, callTokens } = useStore();
  const isRemoteControlEnabled = callTokens?.isRemoteControlEnabled ?? false;

  const [mouse, _] = useMouse();

  // Boolean to control when to show custom cursor
  const [showCustomCursor, setShowCustomCursor] = useState(true);

  // Draw participants map - stored in ref for efficiency
  // Initialized with local participant
  const drawParticipantsRef = useRef<Map<string, DrawParticipant>>(
    new Map([[LOCAL_PARTICIPANT_ID, new DrawParticipant("#FFDF20", drawingMode)]]),
  );

  // Data channel hooks - must be called unconditionally
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

  useDataChannel(Topics.PARTICIPANT_IN_CONTROL, (msg) => {
    const decoder = new TextDecoder();
    const payload = decoder.decode(msg.payload);
    if (payload === localParticipant.localParticipant?.sid) {
      setShowCustomCursor(false);
    } else {
      setShowCustomCursor(true);
    }
  });

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

  useEffect(() => {
    if (track) {
      setStreamDimensions({ width: streamWidth, height: streamHeight });
    } else {
      setStreamDimensions(null);
    }
  }, [track, streamWidth, streamHeight, setStreamDimensions]);

  // Update local participant's drawing mode when it changes
  useEffect(() => {
    const localDrawParticipant = drawParticipantsRef.current.get(LOCAL_PARTICIPANT_ID);
    if (localDrawParticipant) {
      localDrawParticipant.setDrawingMode(drawingMode);
    }
  }, [drawingMode]);

  // Set/update callback for path removal events (depends on localParticipant)
  useEffect(() => {
    const localDrawParticipant = drawParticipantsRef.current.get(LOCAL_PARTICIPANT_ID);
    if (!localDrawParticipant) return;

    // Set callback to send DrawClearPath events when paths are removed
    localDrawParticipant.setOnPathRemoved((pathIds: number[]) => {
      if (!localParticipant.localParticipant) return;

      pathIds.forEach((pathId: number) => {
        const payload: TPDrawClearPath = {
          type: "DrawClearPath",
          payload: { path_id: pathId },
        };
        publishLocalParticipantData(localParticipant.localParticipant, payload, Topics.DRAW);
      });
    });
  }, [localParticipant.localParticipant]);

  // Send DrawingMode event when drawing mode changes or when participant first connects
  // Uses a ref to track the last sent mode to avoid unnecessary sends
  const lastSentDrawingModeRef = useRef<string | null>(null);

  useEffect(() => {
    if (!localParticipant.localParticipant || roomState !== ConnectionState.Connected) {
      // Reset tracking when disconnected
      lastSentDrawingModeRef.current = null;
      return;
    }

    // Serialize current mode to compare with last sent
    const currentModeStr = JSON.stringify(drawingMode);

    // Only send if mode changed or we haven't sent yet after connecting
    if (lastSentDrawingModeRef.current === currentModeStr) return;

    const payload: TPDrawingModeEvent = {
      type: "DrawingMode",
      payload: drawingMode,
    };

    publishLocalParticipantData(localParticipant.localParticipant, payload, Topics.DRAW);

    lastSentDrawingModeRef.current = currentModeStr;
    console.debug("Sent drawing mode", drawingMode);
  }, [drawingMode, localParticipant.localParticipant, roomState]);

  // Watch for clear drawings signal and clear all drawings when it changes
  useEffect(() => {
    // Skip initial render (signal starts at 0)
    if (clearDrawingsSignal === 0) return;

    // Send DrawClearAllPaths event if we have a local participant
    if (localParticipant.localParticipant) {
      const payload: TPDrawClearAllPaths = {
        type: "DrawClearAllPaths",
      };

      publishLocalParticipantData(localParticipant.localParticipant, payload, Topics.DRAW);
    }

    // Clear all drawings from all participants (local and remote)
    drawParticipantsRef.current.forEach((participant) => {
      participant.clear();
    });
  }, [clearDrawingsSignal, localParticipant.localParticipant]);

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

    const handleMouseMove = throttle((e: MouseEvent) => {
      if (videoElement) {
        const { relativeX, relativeY } = getRelativePosition(videoElement, e);
        // If in drawing mode and left button is pressed, send DrawAddPoint
        if (isDrawingMode && (e.buttons & 1) === 1) {
          const payload: TPDrawAddPoint = {
            type: "DrawAddPoint",
            payload: { x: relativeX, y: relativeY },
          };
          publishLocalParticipantData(localParticipant.localParticipant, payload, Topics.DRAW);

          // Update local draw participant
          const drawParticipant = drawParticipantsRef.current.get(LOCAL_PARTICIPANT_ID);
          if (drawParticipant) {
            drawParticipant.handleDrawAddPoint(payload.payload);
          }
        } else {
          // Normal mouse move
          const payload: TPMouseMove = {
            type: "MouseMove",
            payload: { x: relativeX, y: relativeY, pointer: true },
          };

          publishLocalParticipantData(localParticipant.localParticipant, payload, Topics.CURSORS);
        }
      }
    }, 30);

    const handleMouseDown = (e: MouseEvent) => {
      if (videoElement) {
        const { relativeX, relativeY } = getRelativePosition(videoElement, e);

        // If in drawing mode and left button, send DrawStart
        if (isDrawingMode && e.button === 0) {
          if (drawingMode.type === "ClickAnimation") {
            // Show local ripple effect
            applyCursorRippleEffect(e.clientX, e.clientY, "var(--color-cyan-800)");

            const payload: TPClickAnimation = {
              type: "ClickAnimation",
              payload: { x: relativeX, y: relativeY },
            };

            publishLocalParticipantData(localParticipant.localParticipant, payload, Topics.DRAW);
          } else {
            const pathId = getNextPathId.next().value;
            const payload: TPDrawStart = {
              type: "DrawStart",
              payload: { point: { x: relativeX, y: relativeY }, path_id: pathId },
            };
            publishLocalParticipantData(localParticipant.localParticipant, payload, Topics.DRAW);

            // Update local draw participant
            const drawParticipant = drawParticipantsRef.current.get(LOCAL_PARTICIPANT_ID);
            if (drawParticipant) {
              drawParticipant.handleDrawStart(payload.payload.point, payload.payload.path_id);
            }
          }
          return;
        } else {
          // Always show local ripple on left click (except when in Draw mode, handled above)
          if (e.button === 0) {
            applyCursorRippleEffect(e.clientX, e.clientY, "var(--color-cyan-800)");

            // If remote control is disabled, send ClickAnimation event instead of MouseClick
            if (!isRemoteControlEnabled) {
              const payload: TPClickAnimation = {
                type: "ClickAnimation",
                payload: { x: relativeX, y: relativeY },
              };
              publishLocalParticipantData(localParticipant.localParticipant, payload, Topics.DRAW);
              return;
            }
          }
        }

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

        // If in drawing mode and left button was just released, send DrawEnd
        if (isDrawingMode && e.button === 0) {
          if (drawingMode.type !== "ClickAnimation") {
            const payload: TPDrawEnd = {
              type: "DrawEnd",
              payload: { x: relativeX, y: relativeY },
            };
            publishLocalParticipantData(localParticipant.localParticipant, payload, Topics.DRAW);

            // Update local draw participant
            const drawParticipant = drawParticipantsRef.current.get(LOCAL_PARTICIPANT_ID);
            if (drawParticipant) {
              drawParticipant.handleDrawEnd(payload.payload);
            }
          }
          return;
        } else {
          // If remote control is disabled, skip sending MouseClick on left mouse up
          if (!isRemoteControlEnabled) {
            return;
          }
        }

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
      // If in drawing mode and right-click-to-clear is enabled, clear all drawings
      if (isDrawingMode && drawingMode.type === "Draw" && rightClickToClear) {
        e.stopPropagation();
        // Send DrawClearAllPaths event on right-click
        const payload: TPDrawClearAllPaths = {
          type: "DrawClearAllPaths",
        };
        publishLocalParticipantData(localParticipant.localParticipant, payload, Topics.DRAW);

        // Update local draw participant
        const drawParticipant = drawParticipantsRef.current.get(LOCAL_PARTICIPANT_ID);
        if (drawParticipant) {
          drawParticipant.clear();
        }
        return;
      }
    };

    const handleWheel = throttle((e: WheelEvent) => {
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
    }, 16);

    // Send mouse visible data
    if (videoElement) {
      const payload: TPMouseVisible = {
        type: "MouseVisible",
        payload: { visible: !isSharingMouse },
      };
      localParticipant.localParticipant?.publishData(encoder.encode(JSON.stringify(payload)), { reliable: true });
    }

    if (videoElement) {
      videoElement.addEventListener("mousemove", handleMouseMove);
      videoElement.addEventListener("mousedown", handleMouseDown);
      videoElement.addEventListener("mouseup", handleMouseUp);
      // Always add contextmenu handler when in drawing mode or sharing mouse
      if (isDrawingMode || isSharingMouse) {
        videoElement.addEventListener("contextmenu", handleContextMenu);
      }
    }

    if (videoElement && isSharingMouse) {
      videoElement.addEventListener("wheel", handleWheel);
    }

    return () => {
      if (videoElement) {
        videoElement.removeEventListener("mousemove", handleMouseMove);
        videoElement.removeEventListener("wheel", handleWheel);
        videoElement.removeEventListener("mousedown", handleMouseDown);
        videoElement.removeEventListener("mouseup", handleMouseUp);
        if (isDrawingMode || isSharingMouse) {
          videoElement.removeEventListener("contextmenu", handleContextMenu);
        }
      }
    };
  }, [isSharingMouse, isDrawingMode, drawingMode, updateMouseControls, rightClickToClear, isRemoteControlEnabled]);

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
      if (isMouseInside && isSharingKeyEvents) {
        // Skip copy and paste keys
        if (OS === "macos") {
          if (e.metaKey && (e.code === "KeyC" || e.code === "KeyV" || e.code === "KeyX")) {
            return;
          }
        } else if (OS === "windows") {
          if (e.ctrlKey && (e.code === "KeyC" || e.code === "KeyV" || e.code === "KeyX")) {
            return;
          }
        }

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
      if (isMouseInside && isSharingKeyEvents) {
        // Skip copy and paste keys
        if (OS === "macos") {
          if (e.metaKey && (e.code === "KeyC" || e.code === "KeyV" || e.code === "KeyX")) {
            return;
          }
        } else if (OS === "windows") {
          if (e.ctrlKey && (e.code === "KeyC" || e.code === "KeyV" || e.code === "KeyX")) {
            return;
          }
        }

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

  const clearClipboard = useCallback(async () => {
    await writeText("");
  }, []);

  useEffect(() => {
    const handlePaste = (e: ClipboardEvent) => {
      e.preventDefault();
      if (isMouseInside && isSharingKeyEvents) {
        // Get text from local clipboard and send it in packets
        const clipboardText = e.clipboardData?.getData("text/plain");
        if (clipboardText && clipboardText.length > 0) {
          const textBytes = encoder.encode(clipboardText);
          const maxPacketSize = 15 * 1024; // 15KB
          const totalPackets = Math.ceil(textBytes.length / maxPacketSize);

          for (let i = 0; i < totalPackets; i++) {
            const start = i * maxPacketSize;
            const end = Math.min((i + 1) * maxPacketSize, textBytes.length);
            const chunk = textBytes.slice(start, end);

            const payload: TPPasteFromClipboard = {
              type: "PasteFromClipboard",
              payload: {
                data: {
                  packet_id: i,
                  total_packets: totalPackets,
                  data: Array.from(chunk),
                },
              },
            };
            localParticipant.localParticipant?.publishData(encoder.encode(JSON.stringify(payload)), {
              reliable: true,
            });
          }
        } else {
          // Send null data to trigger paste from remote clipboard
          const payload: TPPasteFromClipboard = {
            type: "PasteFromClipboard",
            payload: {
              data: null,
            },
          };
          localParticipant.localParticipant?.publishData(encoder.encode(JSON.stringify(payload)), { reliable: true });
        }
      }
    };

    const handleCopy = (e: ClipboardEvent) => {
      e.preventDefault();
      if (isMouseInside && isSharingKeyEvents) {
        const payload: TPAddToClipboard = {
          type: "AddToClipboard",
          payload: {
            is_copy: true,
          },
        };
        localParticipant.localParticipant?.publishData(encoder.encode(JSON.stringify(payload)), { reliable: true });
        clearClipboard();
      }
    };

    const handleCut = (e: ClipboardEvent) => {
      e.preventDefault();
      if (isMouseInside && isSharingKeyEvents) {
        const payload: TPAddToClipboard = {
          type: "AddToClipboard",
          payload: {
            is_copy: false,
          },
        };
        localParticipant.localParticipant?.publishData(encoder.encode(JSON.stringify(payload)), { reliable: true });
        clearClipboard();
      }
    };

    document.addEventListener("paste", handlePaste);
    document.addEventListener("copy", handleCopy);
    document.addEventListener("cut", handleCut);

    return () => {
      document.removeEventListener("paste", handlePaste);
      document.removeEventListener("copy", handleCopy);
      document.removeEventListener("cut", handleCut);
    };
  }, [isMouseInside, isSharingKeyEvents]);

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
      className={cn(
        "w-full screenshare-video rounded-t-lg rounded-b-lg overflow-hidden border-solid border-2 relative",
        {
          "screenshare-video-focus": isMouseInside,
          "border-slate-200": !isMouseInside,
        },
      )}
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

      <DrawingLayer videoRef={videoRef} drawParticipantsRef={drawParticipantsRef} />

      <RemoteCursors videoRef={videoRef} />

      {/* Custom cursor rendered at mouse position */}
      {showCustomCursor && mouse.x !== null && mouse.y !== null && (
        <div
          className="absolute pointer-events-none z-50"
          style={{
            left: `${mouse.x - (videoRef.current?.getBoundingClientRect().left || 0) - 4}px`,
            top: `${mouse.y - (videoRef.current?.getBoundingClientRect().top || 0) - 4}px`,
          }}
        >
          {drawingMode.type === "Draw" ?
            <HiPencil
              className="size-5 stroke-[1.5px] stroke-white rotate-75"
              style={{ color: "var(--color-cyan-800)" }}
            />
          : <SvgComponent color="var(--color-cyan-800)" />}
        </div>
      )}
    </div>
  );
});
