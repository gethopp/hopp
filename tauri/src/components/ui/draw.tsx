import * as React from "react";
import { useEffect, useRef, useImperativeHandle } from "react";
import { cn } from "@/lib/utils";
import { DrawParticipant } from "./draw-participant";

export interface DrawProps extends React.CanvasHTMLAttributes<HTMLCanvasElement> {
  videoRef: React.RefObject<HTMLVideoElement>;
  participants: Map<string, DrawParticipant>;
  className?: string;
}

const Draw = React.forwardRef<HTMLCanvasElement, DrawProps>(({ videoRef, participants, className, ...props }, ref) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const animationFrameRef = useRef<number | null>(null);

  // Expose canvas ref via forwardRef
  useImperativeHandle(ref, () => canvasRef.current as HTMLCanvasElement, []);

  // Handle canvas resizing to match video element
  useEffect(() => {
    const videoElement = videoRef.current;
    const canvasElement = canvasRef.current;
    const containerElement = containerRef.current;

    if (!videoElement || !canvasElement || !containerElement) return;

    const resizeCanvas = () => {
      const rect = videoElement.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;

      // Update container size
      containerElement.style.width = `${rect.width}px`;
      containerElement.style.height = `${rect.height}px`;

      // Update canvas size
      canvasElement.width = rect.width * dpr;
      canvasElement.height = rect.height * dpr;
      canvasElement.style.width = `${rect.width}px`;
      canvasElement.style.height = `${rect.height}px`;

      const ctx = canvasElement.getContext("2d");
      if (ctx) {
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.scale(dpr, dpr);
      }
    };

    // Initial resize
    resizeCanvas();

    // Observe video element for size changes
    const resizeObserver = new ResizeObserver(resizeCanvas);
    resizeObserver.observe(videoElement);

    return () => {
      resizeObserver.disconnect();
    };
  }, [videoRef]);

  // Render all paths from all participants
  useEffect(() => {
    const canvasElement = canvasRef.current;
    if (!canvasElement) return;

    const render = () => {
      const ctx = canvasElement.getContext("2d");
      if (!ctx) return;

      // Get logical dimensions (after DPR scaling)
      const logicalWidth = parseFloat(canvasElement.style.width) || canvasElement.getBoundingClientRect().width;
      const logicalHeight = parseFloat(canvasElement.style.height) || canvasElement.getBoundingClientRect().height;

      // Clear canvas
      ctx.clearRect(0, 0, logicalWidth, logicalHeight);

      // Render paths from all participants
      participants.forEach((participant) => {
        const paths = participant.getAllPaths();

        paths.forEach((path) => {
          if (path.points.length === 0) return;

          ctx.strokeStyle = path.color;
          ctx.lineWidth = 5;
          ctx.lineCap = "round";
          ctx.lineJoin = "round";

          ctx.beginPath();
          const firstPoint = path.points[0];
          if (!firstPoint) return;
          ctx.moveTo(firstPoint.x * logicalWidth, firstPoint.y * logicalHeight);

          for (let i = 1; i < path.points.length; i++) {
            const point = path.points[i];
            if (!point) continue;
            ctx.lineTo(point.x * logicalWidth, point.y * logicalHeight);
          }

          ctx.stroke();
        });
      });

      animationFrameRef.current = requestAnimationFrame(render);
    };

    animationFrameRef.current = requestAnimationFrame(render);

    return () => {
      if (animationFrameRef.current !== null) {
        cancelAnimationFrame(animationFrameRef.current);
      }
    };
  }, [participants]);

  return (
    <div ref={containerRef} className={cn("absolute top-0 left-0 pointer-events-none", className)}>
      <canvas ref={canvasRef} className={cn("block w-full h-full", className)} {...props} />
    </div>
  );
});

Draw.displayName = "Draw";

export { Draw };
