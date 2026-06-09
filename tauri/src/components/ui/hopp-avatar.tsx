import { Avatar, AvatarFallback, AvatarImage } from "@radix-ui/react-avatar";
import { clsx } from "clsx";
import { LuMicOff } from "react-icons/lu";

type Status = "online" | "offline";

interface CallPeer {
  avatarUrl?: string;
  firstName: string;
  lastName: string;
}

interface HoppAvatarProps {
  src?: string;
  firstName: string;
  lastName: string;
  status?: Status;
  className?: string;
  isMuted?: boolean;
  callPeers?: CallPeer[];
}


const ColorsClasses: string[] = [
  "oklch(54.1% 0.281 293.009)", // Violet 600
  "oklch(59.6% 0.145 163.225)", // Emerald 700
  "oklch(55.5% 0.163 48.998)", // Amber 700
  "oklch(51.1% 0.096 186.391)" // Teal 700
]

// Source: https://stackoverflow.com/a/7616484
const generateHash = (string: string) => {
  let hash = 0;
  for (const char of string) {
    hash = (hash << 5) - hash + char.charCodeAt(0);
    hash |= 0; // Constrain to 32bit integer
    // Always return a positive number
    hash = Math.abs(hash);
  }
  return hash;
};

export function pickFromArray<T>(key: string, options: readonly T[]): T {
  const index = generateHash(key) % options.length;
  return options[index] as T;
}


export const HoppAvatar = ({ src, firstName, lastName, status, className, isMuted, callPeers }: HoppAvatarProps) => {

  const peers = callPeers ?? [];

  if (peers.length === 0) {
    return (
      <div className="relative overflow-visible">
        <Avatar
          className={clsx(
            "size-10 shrink-0 rounded-[9px] text-white flex justify-center items-center overflow-hidden",
            className,
          )}
          style={{ backgroundColor: pickFromArray(firstName + lastName, ColorsClasses) }}
        >
          <AvatarImage className="object-cover h-full" src={src || ""} />
          <AvatarFallback className="font-medium text-[12px]">
            {firstName[0]}
            {lastName[0]}
          </AvatarFallback>
        </Avatar>
        {/* Absolute gray blanket for muted indicator */}
        {isMuted && (
          <div className="absolute flex items-center justify-center inset-0 bg-gray-500/40 rounded-[9px] w-full h-full">
            <LuMicOff className="size-4 text-white" />
          </div>
        )}
        {status && (
          <div
            className={clsx("absolute bottom-0 right-0 size-2 outline-solid outline-3 outline-white rounded-full", {
              "bg-emerald-500": status === "online",
              "bg-red-400": status === "offline",
            })}
          />)
        }
      </div>
    )
  }

  if (peers.length === 1 && peers[0]) {
    // Diagonal based, a bit of overlap, and no status badges
    return (
      <div className="relative overflow-visible size-10">
        <Avatar
          className={clsx(
            "size-[23px] shrink-0 rounded-[5px] text-white flex justify-center items-center overflow-hidden border border-white",
            className,
          )}
          style={{ backgroundColor: pickFromArray(firstName + lastName, ColorsClasses) }}
        >
          <AvatarImage className="object-cover h-full" src={src || ""} />
          <AvatarFallback className="font-medium text-[7px]">
            {firstName[0]}
            {lastName[0]}
          </AvatarFallback>
        </Avatar>

        <Avatar
          className={clsx(
            "size-[23px] shrink-0 rounded-[5px] text-white flex justify-center items-center overflow-hidden border border-white absolute bottom-0 right-0",
            className,
          )}
          style={{ backgroundColor: pickFromArray(peers[0].firstName + peers[0].lastName, ColorsClasses) }}
        >
          <AvatarImage className="object-cover h-full" src={peers[0].avatarUrl || ""} />
          <AvatarFallback className="font-medium text-[7px]">
            {peers[0].firstName[0]}
            {peers[0].lastName[0]}
          </AvatarFallback>
        </Avatar>
      </div>
    )
  }


  if (peers.length >= 2 && peers.every((peer) => peer !== undefined)) {
    const isOverflow = peers.length > 3;
    const visiblePeers = peers.slice(0, isOverflow ? 2 : 3);

    // Grid based, no overlap, and no status badges
    // For >4 peers, we add a +N text element to the bottom right
    return (
      <div className="relative overflow-visible size-10 grid grid-cols-2 gap-px">
        <Avatar
          className={clsx(
            "size-[19px] shrink-0 rounded-[5px] text-white flex justify-center items-center overflow-hidden",
            className,
          )}
          style={{ backgroundColor: pickFromArray(firstName + lastName, ColorsClasses) }}
        >
          <AvatarImage className="object-cover h-full" src={src || ""} />
          <AvatarFallback className="font-medium text-[7px]">
            {firstName[0]}
            {lastName[0]}
          </AvatarFallback>
        </Avatar>

        {visiblePeers.map((peer, i) => (
          <Avatar
            key={i}
            className={clsx(
              "size-[19px] shrink-0 rounded-[5px] text-white flex justify-center items-center overflow-hidden",
              className,
            )}
            style={{ backgroundColor: pickFromArray(peer.firstName + peer.lastName, ColorsClasses) }}
          >
            <AvatarImage className="object-cover h-full" src={peer.avatarUrl || ""} />
            <AvatarFallback className="font-medium text-[7px]">
              {peer.firstName[0]}
              {peer.lastName[0]}
            </AvatarFallback>
          </Avatar>
        ))}
        {isOverflow && (
          <div className="size-[19px] shrink-0 text-slate-600 flex justify-center items-center overflow-hidden text-[11px] font-semibold">
            +{peers.length - 2}
          </div>
        )}
      </div>
    )
  }

  return null
}