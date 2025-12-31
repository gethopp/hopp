import clsx from "clsx";
import React from "react";
type RoomButtonState = "deactivated" | "active" | "neutral";

export const RoomButton: React.FC<
  React.PropsWithChildren<
    {
      cornerIcon?: React.ReactNode;
      presenceAvatars?: React.ReactNode;
      size?: "default" | "unsized";
      title: string;
    } & React.ComponentPropsWithoutRef<"button">
  >
> = ({ cornerIcon, presenceAvatars, title, className = "", size = "default", ...props }) => {
  return (
    <button
      {...props}
      className={clsx(
        "group h-16 flex flex-col p-2 justify-between border border-gray-200 rounded-md overflow-hidden shadow-xs relative cursor-pointer",
        className,
      )}
    >
      <div className="flex flex-row justify-between w-full">
        <span className="flex-1 text-balance text-left text-xs font-medium text-slate-800 overflow-hidden line-clamp-2 leading-tight">
          {title}
        </span>
        {cornerIcon && (
          <span
            onClick={(e) => {
              e.stopPropagation();
              e.preventDefault();
            }}
            className="flex flex-row justify-center text-gray-500 size-4 shrink-0"
          >
            {cornerIcon}
          </span>
        )}
      </div>
      {presenceAvatars && (
        <div
          onClick={(e) => {
            e.stopPropagation();
          }}
          className="flex flex-row items-center"
        >
          {presenceAvatars}
        </div>
      )}
    </button>
  );
};
