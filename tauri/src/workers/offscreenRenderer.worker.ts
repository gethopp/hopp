type InitMessage = {
  type: "init";
  canvas: OffscreenCanvas;
};

type FrameMessage = {
  type: "frame";
  frameId: number;
  width: number;
  height: number;
  timestamp: number;
  data: ArrayBuffer;
  fullRange: boolean;
};

type DisposeMessage = {
  type: "dispose";
};

type IncomingMessage = InitMessage | FrameMessage | DisposeMessage;

interface RendererState {
  canvas: OffscreenCanvas;
  ctx: OffscreenCanvasRenderingContext2D;
}

let state: RendererState | null = null;

function ensureState(message: IncomingMessage): RendererState | null {
  if (message.type === "init") {
    const ctx = message.canvas.getContext("2d", { colorSpace: "srgb" });
    if (!ctx) {
      console.error("Offscreen renderer: Failed to acquire 2D context");
      return null;
    }
    state = { canvas: message.canvas, ctx };
    (self as any).postMessage({ type: "ready" });
    return state;
  }

  if (!state) {
    console.warn("Offscreen renderer: Received message before initialization");
    return null;
  }

  return state;
}

function drawFrame(message: FrameMessage, renderer: RendererState) {
  const { canvas, ctx } = renderer;
  const { width, height, data, frameId, timestamp, fullRange } = message;

  const ySize = width * height;
  const uvPlaneSize = ySize >> 2;

  try {
    const { displayWidth, displayHeight } = calculateDisplaySize(width, height);
    if (canvas.width !== displayWidth || canvas.height !== displayHeight) {
      canvas.width = displayWidth;
      canvas.height = displayHeight;
    }

    const frame = new VideoFrame(data, {
      format: "I420",
      codedWidth: width,
      codedHeight: height,
      timestamp: timestamp * 1000,
      duration: 16666,
      layout: [
        { offset: 0, stride: width },
        { offset: ySize, stride: width >> 1 },
        { offset: ySize + uvPlaneSize, stride: width >> 1 },
      ],
      colorSpace: {
        primaries: "bt709",
        transfer: "bt709",
        matrix: "bt709",
        fullRange,
      },
    } as any);

    try {
      const beforeDrawMs = Date.now();
      ctx.drawImage(frame, 0, 0, displayWidth, displayHeight);
      const afterDrawMs = Date.now();
      (self as any).postMessage({
        type: "metrics",
        frameId,
        beforeDrawMs,
        afterDrawMs,
      });
    } finally {
      try {
        frame.close();
      } catch (err) {
        console.error("Offscreen renderer: Failed to close frame", err);
      }
    }
  } catch (error) {
    console.error("Offscreen renderer: Failed to draw frame", error);
  }
}

function calculateDisplaySize(width: number, height: number) {
  return { displayWidth: width, displayHeight: height, scaleX: 1 };
}

(self as any).onmessage = (event: MessageEvent<IncomingMessage>) => {
  const message = event.data;
  if (!message) return;

  if (message.type === "dispose") {
    state = null;
    return;
  }

  const renderer = ensureState(message);
  if (!renderer) return;

  if (message.type === "frame") {
    drawFrame(message, renderer);
  }
};


