import * as React from "react";
import { SVGProps, useState, useRef, useEffect } from "react";

const DragIcon = (props: SVGProps<SVGSVGElement>) => (
  <svg xmlns="http://www.w3.org/2000/svg" width={29} height={29} fill="none" {...props}>
    <path
      fill="#D9D9D9"
      fillOpacity={0.9}
      d="M5.994 20.3a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM9.86 20.3a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM13.724 20.3a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM17.591 20.3a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM21.457 20.3a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM25.322 20.3a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM5.994 16.433a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM9.86 16.433a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM13.724 16.433a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM17.591 16.433a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM21.457 16.433a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM25.322 16.433a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM5.994 12.569a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM9.86 12.569a1.16 1.16 0 1 0-2.321 0 1.16 1.16 0 0 0 2.32 0ZM13.724 12.569a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM17.591 12.569a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM21.457 12.569a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM25.322 12.569a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM5.994 8.702a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM9.86 8.702a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM13.724 8.702a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM17.591 8.702a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM21.457 8.702a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0ZM25.322 8.702a1.16 1.16 0 1 0-2.32 0 1.16 1.16 0 0 0 2.32 0Z"
    />
  </svg>
);

const CornerIcon = (props: SVGProps<SVGSVGElement>) => (
  <svg xmlns="http://www.w3.org/2000/svg" width={10} height={10} fill="none" {...props}>
    <path
      stroke="#000"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={0.708}
      d="M6.25 2h.708a.708.708 0 0 1 .709.708v.709"
    />
    <path
      stroke="#000"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeOpacity={0.5}
      strokeWidth={0.708}
      d="M7.667 6.25v.708a.708.708 0 0 1-.709.709H6.25M3.417 7.667h-.709A.708.708 0 0 1 2 6.958V6.25M2 3.417v-.709A.708.708 0 0 1 2.708 2h.709"
    />
  </svg>
);

const PointerClickIcon = (props: SVGProps<SVGSVGElement>) => (
  <svg xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 200 200" {...props}>
    <g clipPath="url(#a)">
      <path
        fill="currentColor"
        d="M50.246 57.15c-7.389 4.266-9.996 13.814-5.77 21.132l30.35 52.569-4.714-1.357-1.686-.325c-8.138-2.14-16.597 2.744-18.813 10.861-2.216 8.118 2.621 16.497 10.76 18.636l.08.139 57.579 14.782.362-.023.301.197c8.654 1.007 18.112-.444 26.799-5.46l7.722-4.458c20.375-11.763 27.4-37.491 15.747-57.674L148.89 71.402c-4.225-7.319-13.798-9.835-21.186-5.57-1.913 1.105-3.384 2.65-4.612 4.332-4.817-4.937-12.654-6.23-18.919-2.613-3.44 1.986-5.826 5.148-6.978 8.664-3.621-.76-7.553-.275-10.993 1.711-1.58.912-2.88 2.104-4.01 3.428L71.431 62.72c-4.225-7.319-13.798-9.835-21.186-5.57Zm5.139 8.9c2.492-1.438 5.637-.612 7.062 1.857L90.71 116.86l8.985-5.188-10.277-17.801c-1.426-2.469-.57-5.606 1.923-7.044 2.492-1.44 5.637-.612 7.062 1.856l10.278 17.801 8.985-5.188-10.277-17.8c-1.426-2.47-.569-5.606 1.923-7.045s5.637-.612 7.062 1.857l10.277 17.8 9.407-5.43-5.139-8.9c-1.425-2.47-.568-5.606 1.924-7.045s5.637-.612 7.062 1.856l20.073 34.768c8.902 15.419 3.666 34.598-11.901 43.586l-7.722 4.458c-6.581 3.799-13.612 4.753-20.36 3.967L63.077 148.76c-3.145-.827-4.39-2.982-3.533-6.119.857-3.137 3.033-4.393 6.178-3.567l30.455 8.006L53.46 73.094c-1.425-2.468-.569-5.605 1.924-7.044Z"
      />
      <path
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth={8.899}
        d="m66.514 34.522-8.899 8.454M26.91 51.873l-12.904-3.56M30.925 69.675l-8.454 8.899M36.254 26.065l3.56 12.904"
      />
    </g>
    <defs>
      <clipPath id="a">
        <path fill="currentColor" d="M0 0h200v200H0z" />
      </clipPath>
    </defs>
  </svg>
);

/**
 * A microphone icon that fills from bottom to top based on audio level.
 * Uses smoothing for nice eased animation.
 */
const MicWithLevel = ({ level, className }: { level: number; className?: string }) => {
  const [smoothedLevel, setSmoothedLevel] = useState(0);
  const targetLevelRef = useRef(level);
  const animationRef = useRef<number>();

  useEffect(() => {
    targetLevelRef.current = level;

    const animate = () => {
      setSmoothedLevel((prev) => {
        const target = targetLevelRef.current;
        const diff = target - prev;
        // Ease towards target - faster when going up, slower when going down
        const speed = diff > 0 ? 0.3 : 0.15;
        const newValue = prev + diff * speed;
        // Stop animating when close enough
        if (Math.abs(diff) < 0.001) return target;
        return newValue;
      });
      animationRef.current = requestAnimationFrame(animate);
    };

    animationRef.current = requestAnimationFrame(animate);
    return () => {
      if (animationRef.current) cancelAnimationFrame(animationRef.current);
    };
  }, [level]);

  // Clamp level between 0 and 1, then boost it for visual effect
  const fillPercent = Math.min(1, Math.max(0, smoothedLevel * 3)) * 100;
  // Gradient goes from top (0%) to bottom (100%), so we invert
  const gradientStop = 100 - fillPercent;

  return (
    <svg
      className={className}
      width="1em"
      height="1em"
      viewBox="0 0 24 24"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <defs>
        <linearGradient id="micFillGradient" x1="0%" y1="0%" x2="0%" y2="100%">
          {/* Top part - transparent (unfilled) */}
          <stop offset={`${gradientStop}%`} stopColor="currentColor" stopOpacity="0.15" />
          {/* Bottom part - filled */}
          <stop offset={`${gradientStop}%`} stopColor="currentColor" stopOpacity="1" />
        </linearGradient>
      </defs>
      {/* Filled mic body */}
      <path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" fill="url(#micFillGradient)" />
      {/* Outline strokes */}
      <path
        d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        fill="none"
      />
      <path
        d="M19 10v2a7 7 0 0 1-14 0v-2"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <line
        x1="12"
        x2="12"
        y1="19"
        y2="22"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
};

const CustomIcons = {
  Drag: DragIcon,
  Corner: CornerIcon,
  PointerClick: PointerClickIcon,
  MicWithLevel: MicWithLevel,
};

export { CustomIcons };
