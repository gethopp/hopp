import clsx from "clsx";
import React from "react";
type ToggleIconButtonState = "deactivated" | "active" | "neutral";

export const ToggleIconButton: React.FC<
  React.PropsWithChildren<
    {
      icon?: React.ReactNode;
      state: ToggleIconButtonState;
      cornerIcon?: React.ReactNode;
      size?: "default" | "unsized";
    } & React.ComponentPropsWithoutRef<"button">
  >
> = ({ icon, children, state = "neutral", cornerIcon, className = {}, size = "default", ...props }) => {
  return (
    <button
      {...props}
      className={clsx(
        "flex flex-col p-4 small items-center justify-center gap-1 px-4 py-1.5 rounded-md ring-1 ring-inset shadow-sm transition-colors duration-100 relative font-medium",
        {
          "bg-gray-300 text-gray-600": state === "deactivated",
          "ring-emerald-600": state === "active",
          "bg-white text-gray-500 hover:bg-gray-100 ring-slate-200": state === "neutral",
          "h-[65px] w-[110px]": size === "default",
        },
        className,
      )}
    >
      {cornerIcon && (
        <span
          onClick={(e) => {
            e.stopPropagation();
            e.preventDefault();
          }}
          className="absolute top-1.5 right-1.5 text-gray-500"
        >
          {cornerIcon}
        </span>
      )}
      {icon && <span>{icon}</span>}
      {children && <span className="text-xs whitespace-nowrap">{children}</span>}
    </button>
  );
};
