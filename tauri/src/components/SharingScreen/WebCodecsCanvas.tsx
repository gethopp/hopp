import React from "react";

type Props = {
  className?: string;
  style?: React.CSSProperties;
  width?: number;
  height?: number;
};

export const WebCodecsCanvas = React.forwardRef<HTMLCanvasElement, Props>(function WebCodecsCanvas(
  { className, style, width, height }: Props,
  forwardedRef,
) {
  const canvasRef = React.useRef<HTMLCanvasElement>(null);

  React.useImperativeHandle(forwardedRef, () => canvasRef.current as HTMLCanvasElement, []);

  React.useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    // Initialize 2D context so the parent can draw frames immediately
    canvas.getContext("2d", { colorSpace: "srgb" });
  }, []);

  return <canvas ref={canvasRef} className={className} style={style} width={width} height={height} />;
});

function calculateDisplaySize(width: number, height: number) {
  // For now, render at source resolution; hook for future scaling or DPR handling.
  return { displayWidth: width, displayHeight: height, scaleX: 1 };
}

// Reusable buffer to avoid allocations
let reusableBuffer: ArrayBuffer | null = null;
let reusableBufferSize = 0;

export function drawI420FrameToCanvas(
  canvas: HTMLCanvasElement,
  yData: Uint8Array,
  uData: Uint8Array,
  vData: Uint8Array,
  width: number,
  height: number,
  timestamp: number,
  onMetrics?: (captureToBeforeDrawMs: number, captureToAfterDrawMs: number) => void,
) {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  const ySize = width * height;
  const uvPlaneSize = (width * height) >> 2; // 4:2:0
  const totalSize = ySize + uvPlaneSize + uvPlaneSize;

  // Reuse buffer if possible to avoid allocations
  if (!reusableBuffer || reusableBufferSize < totalSize) {
    reusableBuffer = new ArrayBuffer(totalSize);
    reusableBufferSize = totalSize;
  }

  const view = new Uint8Array(reusableBuffer, 0, totalSize);

  // Direct memory copies - unavoidable for VideoFrame API but minimized
  view.set(yData, 0);
  view.set(uData, ySize);
  view.set(vData, ySize + uvPlaneSize);

  const frame = new VideoFrame(view.buffer.slice(0, totalSize), {
    format: "I420",
    codedWidth: width,
    codedHeight: height,
    timestamp: timestamp * 1000, // microseconds
    duration: 16666,
    layout: [
      { offset: 0, stride: width }, // Y
      { offset: ySize, stride: width >> 1 }, // U
      { offset: ySize + uvPlaneSize, stride: width >> 1 }, // V
    ],
    colorSpace: {
      primaries: "bt709",
      transfer: "bt709",
      matrix: "bt709",
      fullRange: false
    }
  } as any);

  try {
    const beforeDrawMs = Date.now();
    const { displayWidth, displayHeight } = calculateDisplaySize(width, height);
    if (canvas.width !== displayWidth || canvas.height !== displayHeight) {
      canvas.width = displayWidth;
      canvas.height = displayHeight;
    }
    ctx.drawImage(frame, 0, 0, displayWidth, displayHeight);
    const afterDrawMs = Date.now();
    if (onMetrics) onMetrics(beforeDrawMs, afterDrawMs);
  } finally {
    try { frame.close(); } catch {}
  }
}

// Cleanup function to release the reusable buffer
export function cleanupWebCodecsCanvas() {
  reusableBuffer = null;
  reusableBufferSize = 0;
}
