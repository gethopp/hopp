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
    canvas.getContext("2d");
  }, []);

  return <canvas ref={canvasRef} className={className} style={style} width={width} height={height} />;
});

function calculateDisplaySize(width: number, height: number) {
  // For now, render at source resolution; hook for future scaling or DPR handling.
  return { displayWidth: width, displayHeight: height, scaleX: 1 };
}

export function drawNV12FrameToCanvas(
  canvas: HTMLCanvasElement,
  yData: Uint8Array,
  uvData: Uint8Array,
  width: number,
  height: number,
  timestamp: number,
) {
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  const ySize = width * height;
  const uvSize = (width * height) / 2;
  // TODO: check if it is more efficient to use double buffer and initialize them once
  const nv12Buffer = new ArrayBuffer(ySize + uvSize);
  const nv12View = new Uint8Array(nv12Buffer);

  nv12View.set(yData, 0);
  nv12View.set(uvData, ySize);

  const frame = new VideoFrame(nv12Buffer, {
    format: "NV12",
    codedWidth: width,
    codedHeight: height,
    timestamp: timestamp * 1000, // microseconds
    duration: 16666, // ~60fps
  });

  try {
    const { displayWidth, displayHeight } = calculateDisplaySize(width, height);
    if (canvas.width !== displayWidth || canvas.height !== displayHeight) {
      canvas.width = displayWidth;
      canvas.height = displayHeight;
    }
    ctx.drawImage(frame, 0, 0, displayWidth, displayHeight);
  } finally {
    try { frame.close(); } catch {}
  }
}

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

  const buffer = new ArrayBuffer(ySize + uvPlaneSize + uvPlaneSize);
  const view = new Uint8Array(buffer);
  view.set(yData, 0);
  view.set(uData, ySize);
  view.set(vData, ySize + uvPlaneSize);

  const frame = new VideoFrame(buffer, {
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


