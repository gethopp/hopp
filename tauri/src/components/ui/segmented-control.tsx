"use client";

import * as React from "react";
import { motion, LayoutGroup } from "framer-motion";
import { cn } from "@/lib/utils";
import { Tooltip, TooltipContent, TooltipTrigger } from "./tooltip";

interface SegmentedControlItem {
  id: string;
  content: React.ReactNode;
  tooltipContent?: React.ReactNode;
}

interface SegmentedControlProps {
  items: Array<SegmentedControlItem>;
  value?: string;
  defaultValue?: string;
  onValueChange?: (value: string) => void;
  className?: string;
  disabled?: boolean;
}

const SegmentedControl = React.forwardRef<HTMLOListElement, SegmentedControlProps>(
  ({ items, value: valueProp, defaultValue, onValueChange, className, disabled, ...props }, ref) => {
    const uniqueId = React.useId();
    const layoutId = `SegmentedControlActive-${uniqueId}`;

    const [internalValue, setInternalValue] = React.useState(() => {
      if (defaultValue) {
        const foundItem = items.find((item) => item.id === defaultValue);
        return foundItem ? defaultValue : items[0]?.id || "";
      }
      return items[0]?.id || "";
    });

    const activeItemId = React.useMemo(() => {
      if (valueProp !== undefined) {
        const foundItem = items.find((item) => item.id === valueProp);
        return foundItem ? valueProp : items[0]?.id || "";
      }
      return internalValue;
    }, [valueProp, items, internalValue]);

    const handleItemClick = React.useCallback(
      (itemId: string) => {
        if (disabled) return;

        if (valueProp === undefined) {
          setInternalValue(itemId);
        }
        onValueChange?.(itemId);
      },
      [disabled, valueProp, onValueChange],
    );

    return (
      <LayoutGroup>
        <ol
          ref={ref}
          className={cn(
            "inline-flex m-0 p-0.5 list-none bg-gray-500/80 dark:bg-zinc-600 rounded-lg h-[28px] cursor-default",
            disabled && "opacity-50 cursor-not-allowed",
            className,
          )}
          role="tablist"
          {...props}
        >
          {items.map((item, i) => {
            const isActive = item.id === activeItemId;
            const showDivider = !isActive && i !== items.findIndex((itm) => itm.id === activeItemId) - 1;

            return (
              <motion.li
                key={item.id}
                className={cn(
                  "relative mb-0 mt-0 leading-none pl-0",
                  // Divider styles
                  showDivider &&
                    "after:absolute after:top-[15%] after:right-[-0.5px] after:block after:w-px after:h-[70%] after:bg-gray-100 after:opacity-20 after:transition-opacity after:duration-200 after:ease-out after:content-['']",
                  // Hide divider for last item
                  i === items.length - 1 && "after:hidden",
                  // Hide divider when it shouldn't show
                  !showDivider && "after:opacity-0",
                )}
              >
                {item.tooltipContent ?
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        onClick={() => handleItemClick(item.id)}
                        type="button"
                        role="tab"
                        aria-selected={isActive}
                        disabled={disabled}
                        className={cn(
                          "relative m-0 px-3 py-1 text-white text-xs leading-none bg-transparent border-none outline-hidden h-6 flex items-center",
                          "disabled:cursor-not-allowed",
                        )}
                      >
                        {isActive && (
                          <motion.div
                            layoutId={layoutId}
                            className="absolute inset-0 z-1 bg-slate-300/50 rounded-md"
                            style={{
                              boxShadow: "0 1px 2px rgba(0,0,0,.1)",
                            }}
                          />
                        )}
                        <span className="relative z-2">{item.content}</span>
                      </button>
                    </TooltipTrigger>
                    <TooltipContent>{item.tooltipContent}</TooltipContent>
                  </Tooltip>
                : <button
                    onClick={() => handleItemClick(item.id)}
                    type="button"
                    role="tab"
                    aria-selected={isActive}
                    disabled={disabled}
                    className={cn(
                      "relative m-0 px-3 py-1 text-white text-xs leading-none bg-transparent border-none outline-hidden h-6 flex items-center",
                      "disabled:cursor-not-allowed",
                    )}
                  >
                    {isActive && (
                      <motion.div
                        layoutId={layoutId}
                        className="absolute inset-0 z-1 bg-slate-300/50 rounded-md"
                        style={{
                          boxShadow: "0 1px 2px rgba(0,0,0,.1)",
                        }}
                      />
                    )}
                    <span className="relative z-2">{item.content}</span>
                  </button>
                }
              </motion.li>
            );
          })}
        </ol>
      </LayoutGroup>
    );
  },
);

SegmentedControl.displayName = "SegmentedControl";

export { SegmentedControl };
export type { SegmentedControlProps, SegmentedControlItem };
