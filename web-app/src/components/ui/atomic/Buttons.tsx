import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const primaryCTAVariants = cva(
  "inline-flex items-center justify-center whitespace-nowrap rounded-md text-sm font-medium transition-all focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 disabled:pointer-events-none disabled:opacity-50 w-full py-3 px-4",
  {
    variants: {
      fill: {
        filled: "bg-indigo-600 text-white hover:bg-indigo-500 focus-visible:ring-indigo-600",
        outline: "border border-indigo-600 text-indigo-600 hover:bg-indigo-50 focus-visible:ring-indigo-600",
      },
      size: {
        default: "text-base",
        sm: "text-sm",
        lg: "text-lg",
      },
    },
    defaultVariants: {
      fill: "filled",
      size: "default",
    },
  },
);

export interface PrimaryCTAProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof primaryCTAVariants> {}

export function PrimaryCTA({ className, fill, size, ...props }: PrimaryCTAProps) {
  return <button className={cn(primaryCTAVariants({ fill, size, className }))} {...props} />;
}
