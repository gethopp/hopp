import { Avatar, AvatarFallback, AvatarImage } from "@radix-ui/react-avatar";
import { clsx } from "clsx";
import { LuMicOff } from "react-icons/lu";

type Status = "online" | "offline";

interface HoppAvatarProps {
  src?: string;
  firstName: string;
  lastName: string;
  status?: Status;
  className?: string;
  isMuted?: boolean;
  callPeerAvatarUrl?: string;
  callPeerFirstName?: string;
  callPeerLastName?: string;
}

export const HoppAvatar = ({
  src,
  firstName,
  lastName,
  status,
  className,
  isMuted,
  callPeerAvatarUrl,
  callPeerFirstName,
  callPeerLastName,
}: HoppAvatarProps) => {
  return (
    <div className="relative">
      <Avatar
        className={clsx(
          "size-10 shrink-0 rounded-md bg-emerald-200 flex justify-center items-center overflow-hidden",
          className,
        )}
      >
        <AvatarImage className="object-cover h-full" src={src || ""} />
        <AvatarFallback>
          {firstName[0]}
          {lastName[0]}
        </AvatarFallback>
      </Avatar>
      {/* Absolute gray blanket for muted indicator */}
      {isMuted && (
        <div className="absolute flex items-center justify-center inset-0 bg-gray-500/40 rounded-md w-full h-full">
          {isMuted && <LuMicOff className="size-4 text-white" />}
        </div>
      )}
      {callPeerFirstName ?
        <div className="group absolute bottom-0 right-0 size-3.5 rounded-full outline-solid outline-2 outline-white overflow-visible bg-emerald-200 flex items-center justify-center">
          <Avatar className="size-full rounded-full overflow-hidden">
            <AvatarImage className="object-cover h-full" src={callPeerAvatarUrl || ""} />
            <AvatarFallback className="text-[6px]">
              {callPeerFirstName[0]}
              {callPeerLastName?.[0]}
            </AvatarFallback>
          </Avatar>
          <div className="pointer-events-none absolute bottom-full right-0 mb-1 hidden group-hover:flex size-8 rounded-full outline-solid outline-2 outline-white overflow-hidden bg-emerald-200 shadow-md">
            <Avatar className="size-full">
              <AvatarImage className="object-cover h-full" src={callPeerAvatarUrl || ""} />
              <AvatarFallback className="text-xs">
                {callPeerFirstName[0]}
                {callPeerLastName?.[0]}
              </AvatarFallback>
            </Avatar>
          </div>
        </div>
      : status && (
          <div
            className={clsx("absolute bottom-0 right-0 size-2 outline-solid outline-3 outline-white rounded-full", {
              "bg-emerald-500": status === "online",
              "bg-red-400": status === "offline",
            })}
          />
        )
      }
    </div>
  );
};
