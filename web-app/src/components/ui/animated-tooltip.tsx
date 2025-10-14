import React, { useState, useRef, useLayoutEffect } from "react";
import { motion, useTransform, AnimatePresence, useMotionValue, useSpring } from "motion/react";

export const AnimatedTooltip = ({
  items,
}: {
  items: {
    id: number;
    name: string;
    designation: string;
    image: string;
    quote: string | React.ReactNode;
  }[];
}) => {
  const [hoveredIndex, setHoveredIndex] = useState<number | null>(null);
  const [tooltipOffset, setTooltipOffset] = useState<number>(-100);
  const springConfig = { stiffness: 500, damping: 15 };
  const x = useMotionValue(0);
  const animationFrameRef = useRef<number | null>(null);
  const quoteRef = useRef<HTMLDivElement>(null);

  const rotate = useSpring(useTransform(x, [-50, 50], [-15, 15]), springConfig);
  const translateX = useSpring(useTransform(x, [-30, 100], [-50, 50]), springConfig);

  useLayoutEffect(() => {
    if (quoteRef.current && hoveredIndex !== null) {
      const quoteHeight = quoteRef.current.offsetHeight;
      const offset = -(quoteHeight + 15);
      setTooltipOffset(offset);
    }
  }, [hoveredIndex]);

  const handleMouseMove = (event: React.MouseEvent<HTMLImageElement>) => {
    if (animationFrameRef.current) {
      cancelAnimationFrame(animationFrameRef.current);
    }

    animationFrameRef.current = requestAnimationFrame(() => {
      const halfWidth = (event.target as HTMLElement).offsetWidth / 2;
      x.set(event.nativeEvent.offsetX - halfWidth);
    });
  };

  return (
    <>
      {items.map((item) => (
        <div
          className="group relative -mr-2"
          key={item.name}
          onMouseEnter={() => setHoveredIndex(item.id)}
          onMouseLeave={() => setHoveredIndex(null)}
        >
          <AnimatePresence>
            {hoveredIndex === item.id && (
              <motion.div
                initial={{ opacity: 0, y: 20, scale: 0.6 }}
                animate={{
                  opacity: 1,
                  y: tooltipOffset,
                  scale: 1,
                  transition: {
                    type: "spring",
                    stiffness: 500,
                    damping: 15,
                  },
                }}
                exit={{ opacity: 0, y: 20, scale: 0.6 }}
                style={{
                  translateX: translateX,
                  rotate: rotate,
                  // whiteSpace: "nowrap",
                }}
                className="absolute -top-16 left-1/2 z-50 flex -translate-x-1/2 flex-col items-center justify-center rounded-md bg-black px-4 py-2 text-xs shadow-xl  w-[350px] overflow-clip pointer-events-none"
              >
                <div className="absolute inset-x-10 -bottom-px z-30 h-px w-[20%] bg-gradient-to-r from-transparent via-emerald-500 to-transparent" />
                <div className="absolute -bottom-px left-10 z-30 h-px w-[40%] bg-gradient-to-r from-transparent via-sky-500 to-transparent" />
                <div className="relative z-30 text-base font-bold text-white">{item.name}</div>
                <div className="text-xs text-white mt-2" ref={quoteRef}>
                  {item.quote}
                </div>
                <div className="text-xs text-white mt-4">{item.designation}</div>
              </motion.div>
            )}
          </AnimatePresence>
          <img
            onMouseMove={handleMouseMove}
            height={100}
            width={100}
            src={item.image}
            alt={item.name}
            className="relative !m-0 size-14 rounded-full border-2 border-white object-cover object-top !p-0 transition duration-500 group-hover:z-30 group-hover:scale-105"
          />
        </div>
      ))}
    </>
  );
};
