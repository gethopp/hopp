import { z } from "zod";

export const PToken = z.object({
  token: z.string().min(1, "Token is required"), // Ensure token is a non-empty string
});

export type TPToken = z.infer<typeof PToken>;

export const PMouseMove = z.object({
  type: z.literal("MouseMove"),
  payload: z.object({
    x: z.number(),
    y: z.number(),
    pointer: z.boolean(),
  }),
});
export type TPMouseMove = z.infer<typeof PMouseMove>;

export const PRemoteControlEnabled = z.object({
  type: z.literal("RemoteControlEnabled"),
  payload: z.object({
    enabled: z.boolean(),
  }),
});
export type TPRemoteControlEnabled = z.infer<typeof PRemoteControlEnabled>;

export const PMouseClick = z.object({
  type: z.literal("MouseClick"),
  payload: z.object({
    x: z.number(),
    y: z.number(),
    button: z.number(),
    clicks: z.number(),
    down: z.boolean(),
    shift: z.boolean(),
    alt: z.boolean(),
    ctrl: z.boolean(),
    meta: z.boolean(),
  }),
});
export type TPMouseClick = z.infer<typeof PMouseClick>;

export const PMouseVisible = z.object({
  type: z.literal("MouseVisible"),
  payload: z.object({
    visible: z.boolean(),
  }),
});
export type TPMouseVisible = z.infer<typeof PMouseVisible>;

export const PWheelEvent = z.object({
  type: z.literal("WheelEvent"),
  payload: z.object({
    deltaX: z.number(),
    deltaY: z.number(),
  }),
});
export type TPWheelEvent = z.infer<typeof PWheelEvent>;

export const PKeystroke = z.object({
  type: z.literal("Keystroke"),
  payload: z.object({
    key: z.array(z.string()),
    meta: z.boolean(),
    alt: z.boolean(),
    ctrl: z.boolean(),
    shift: z.boolean(),
    down: z.boolean(),
  }),
});
export type TPKeystroke = z.infer<typeof PKeystroke>;

export const PAddToClipboard = z.object({
  type: z.literal("AddToClipboard"),
  payload: z.object({
    is_copy: z.boolean(),
  }),
});
export type TPAddToClipboard = z.infer<typeof PAddToClipboard>;

export const PClipboardPayload = z.object({
  packet_id: z.number(),
  total_packets: z.number(),
  data: z.array(z.number()),
});
export type TPClipboardPayload = z.infer<typeof PClipboardPayload>;

export const PPasteFromClipboard = z.object({
  type: z.literal("PasteFromClipboard"),
  payload: z.object({
    data: PClipboardPayload.nullable(),
  }),
});
export type TPPasteFromClipboard = z.infer<typeof PPasteFromClipboard>;

// Drawing event payloads
export const PClientPoint = z.object({
  x: z.number(),
  y: z.number(),
});
export type TPClientPoint = z.infer<typeof PClientPoint>;

export const PDrawPathPoint = z.object({
  point: PClientPoint,
  path_id: z.number(),
});
export type TPDrawPathPoint = z.infer<typeof PDrawPathPoint>;

export const PDrawStart = z.object({
  type: z.literal("DrawStart"),
  payload: PDrawPathPoint,
});
export type TPDrawStart = z.infer<typeof PDrawStart>;

export const PDrawAddPoint = z.object({
  type: z.literal("DrawAddPoint"),
  payload: PClientPoint,
});
export type TPDrawAddPoint = z.infer<typeof PDrawAddPoint>;

export const PDrawEnd = z.object({
  type: z.literal("DrawEnd"),
  payload: PClientPoint,
});
export type TPDrawEnd = z.infer<typeof PDrawEnd>;

export const PClickAnimation = z.object({
  type: z.literal("ClickAnimation"),
  payload: PClientPoint,
});
export type TPClickAnimation = z.infer<typeof PClickAnimation>;

export const PDrawClearPath = z.object({
  type: z.literal("DrawClearPath"),
  payload: z.object({
    path_id: z.number(),
  }),
});
export type TPDrawClearPath = z.infer<typeof PDrawClearPath>;

export const PDrawClearAllPaths = z.object({
  type: z.literal("DrawClearAllPaths"),
});
export type TPDrawClearAllPaths = z.infer<typeof PDrawClearAllPaths>;

export const PDrawSettings = z.object({
  permanent: z.boolean(),
});
export type TPDrawSettings = z.infer<typeof PDrawSettings>;

export const PDrawingMode = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("Disabled"),
  }),
  z.object({
    type: z.literal("Draw"),
    settings: PDrawSettings,
  }),
  z.object({
    type: z.literal("ClickAnimation"),
  }),
]);
export type TDrawingMode = z.infer<typeof PDrawingMode>;

export const PDrawingModeEvent = z.object({
  type: z.literal("DrawingMode"),
  payload: PDrawingMode,
});
export type TPDrawingModeEvent = z.infer<typeof PDrawingModeEvent>;

// Stored mode type for persisting user's preferred interaction mode
// (intentionally separate from PDrawingMode for future consolidation)
export const PStoredMode = z.discriminatedUnion("type", [
  z.object({ type: z.literal("RemoteControl") }),
  z.object({ type: z.literal("ClickAnimation") }),
  z.object({ type: z.literal("Draw"), permanent: z.boolean() }),
]);
export type TStoredMode = z.infer<typeof PStoredMode>;

// WebSocket Message Types
export const MessageType = z.enum([
  "success",
  "call_request",
  "incoming_call",
  "callee_offline",
  "call_reject",
  "call_accept",
  "call_tokens",
  "error",
  "call_end",
  "ping",
  "pong",
  "teammate_online",
]);

export type TMessageType = z.infer<typeof MessageType>;

// WebSocket Messages
export const PSuccessMessage = z.object({
  type: z.literal("success"),
  payload: z.object({ message: z.string() }),
});

export const PCallRequestMessage = z.object({
  type: z.literal("call_request"),
  payload: z.object({ callee_id: z.string() }),
});

export const PCallEndMessage = z.object({
  type: z.literal("call_end"),
  payload: z.object({ call_id: z.string() }),
});

export const PIncomingCallMessage = z.object({
  type: z.literal("incoming_call"),
  payload: z.object({ caller_id: z.string() }),
});

export const PAcceptCallMessage = z.object({
  type: z.literal("call_accept"),
  payload: z.object({ caller_id: z.string() }),
});

export const PCallTokensMessage = z.object({
  type: z.literal("call_tokens"),
  payload: z.object({
    audioToken: z.string(),
    videoToken: z.string(),
    cameraToken: z.string(),
    participant: z.string(),
  }),
});

export const PRejectCallMessage = z.object({
  type: z.literal("call_reject"),
  payload: z.object({
    caller_id: z.string(),
    reject_reason: z.enum(["in-call", "rejected", "trial-ended"]).optional(),
  }),
});

export const PErrorMessage = z.object({
  type: z.literal("error"),
  payload: z.object({ error: z.string() }),
});

export const PPingMessage = z.object({
  type: z.literal("ping"),
  payload: z.object({ message: z.string() }),
});

export const PPongMessage = z.object({
  type: z.literal("pong"),
  payload: z.object({ message: z.string() }),
});

export const PCalleeOfflineMessage = z.object({
  type: z.literal("callee_offline"),
  payload: z.object({ callee_id: z.string() }),
});

export const PTeammateOnlineMessage = z.object({
  type: z.literal("teammate_online"),
  payload: z.object({ teammate_id: z.string() }),
});

// Export types for all messages
export type TSuccessMessage = z.infer<typeof PSuccessMessage>;
export type TCallRequestMessage = z.infer<typeof PCallRequestMessage>;
export type TCallEndMessage = z.infer<typeof PCallEndMessage>;
export type TIncomingCallMessage = z.infer<typeof PIncomingCallMessage>;
export type TAcceptCallMessage = z.infer<typeof PAcceptCallMessage>;
export type TCallTokensMessage = z.infer<typeof PCallTokensMessage>;
export type TRejectCallMessage = z.infer<typeof PRejectCallMessage>;
export type TErrorMessage = z.infer<typeof PErrorMessage>;
export type TPingMessage = z.infer<typeof PPingMessage>;
export type TPongMessage = z.infer<typeof PPongMessage>;
export type TCalleeOfflineMessage = z.infer<typeof PCalleeOfflineMessage>;
export type TTeammateOnlineMessage = z.infer<typeof PTeammateOnlineMessage>;

// Union type for all possible messages
export const PWebSocketMessage = z.discriminatedUnion("type", [
  PSuccessMessage,
  PCallRequestMessage,
  PCallEndMessage,
  PIncomingCallMessage,
  PAcceptCallMessage,
  PCallTokensMessage,
  PRejectCallMessage,
  PErrorMessage,
  PPingMessage,
  PPongMessage,
  PCalleeOfflineMessage,
  PTeammateOnlineMessage,
]);

export type TWebSocketMessage = z.infer<typeof PWebSocketMessage>;
