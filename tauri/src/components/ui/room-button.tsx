import clsx from "clsx";
import React from "react";
type RoomButtonState = "deactivated" | "active" | "neutral";

export const RoomButton: React.FC<
  React.PropsWithChildren<
    {
      cornerIcon?: React.ReactNode;
      size?: "default" | "unsized";
    } & React.ComponentPropsWithoutRef<"button">
  >
> = ({ children, cornerIcon, className = "", size = "default", ...props }) => {
  return (
    <button
      {...props}
      className={clsx(
        "group h-16 flex flex-col gap-5 p-4 border border-gray-200 rounded-md overflow-hidden shadow-xs relative cursor-pointer",
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
      {children && <span className="text-xs whitespace-nowrap">{children}</span>}
    </button>
  );
};
