import clsx from "clsx";
import React from "react";
type RoomButtonState = "deactivated" | "active" | "neutral";

export const RoomButton: React.FC<
  React.PropsWithChildren<
    {
      cornerIcon?: React.ReactNode;
      size?: "default" | "unsized";
      title: string;
    } & React.ComponentPropsWithoutRef<"button">
  >
> = ({ cornerIcon, title, className = "", size = "default", ...props }) => {
  return (
    <button
      {...props}
      className={clsx(
        "group h-16 flex flex-row gap-5 p-3 justify-between border border-gray-200 rounded-md overflow-hidden shadow-xs relative cursor-pointer",
        className,
      )}
    >
      <span className="w-full h-full text-balance text-left text-xs font-medium text-slate-800 overflow-hidden">
        {title}
      </span>
      {cornerIcon && (
        <span
          onClick={(e) => {
            e.stopPropagation();
            e.preventDefault();
          }}
          className="flex flex-row justify-center text-gray-500 size-4"
        >
          {cornerIcon}
        </span>
      )}
    </button>
  );
};
