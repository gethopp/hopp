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

const PeerBadge = ({ peer }: { peer: CallPeer }) => (
  <div className="group absolute bottom-0 right-0 size-3.5 rounded-full outline-solid outline-2 outline-white overflow-visible bg-emerald-200 flex items-center justify-center">
    <Avatar className="size-full rounded-full overflow-hidden">
      <AvatarImage className="object-cover h-full" src={peer.avatarUrl || ""} />
      <AvatarFallback className="text-[6px]">
        {peer.firstName[0]}
        {peer.lastName[0]}
      </AvatarFallback>
    </Avatar>
    <div className="pointer-events-none absolute bottom-full right-0 mb-1 hidden group-hover:flex size-8 rounded-full outline-solid outline-2 outline-white overflow-hidden bg-emerald-200 shadow-md">
      <Avatar className="size-full">
        <AvatarImage className="object-cover h-full" src={peer.avatarUrl || ""} />
        <AvatarFallback className="text-xs">
          {peer.firstName[0]}
          {peer.lastName[0]}
        </AvatarFallback>
      </Avatar>
    </div>
  </div>
);

export const HoppAvatar = ({ src, firstName, lastName, status, className, isMuted, callPeers }: HoppAvatarProps) => {
  const peers = callPeers ?? [];

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
      {peers.length === 1 ?
        <PeerBadge peer={peers[0]} />
      : peers.length === 2 ?
        <div className="group absolute bottom-0 right-0 flex overflow-visible">
          <div className="pointer-events-none absolute bottom-full right-0 mb-1 hidden group-hover:flex gap-1 rounded-md outline-solid outline-2 outline-white bg-white shadow-md p-1">
            {[peers[1], peers[0]].map((peer, i) => (
              <div
                key={i}
                className="size-8 rounded-full outline-solid outline-2 outline-white overflow-hidden bg-emerald-200"
              >
                <Avatar className="size-full">
                  <AvatarImage className="object-cover h-full" src={peer.avatarUrl || ""} />
                  <AvatarFallback className="text-xs">
                    {peer.firstName[0]}
                    {peer.lastName[0]}
                  </AvatarFallback>
                </Avatar>
              </div>
            ))}
          </div>
          <div className="size-3.5 rounded-full outline-solid outline-2 outline-white overflow-visible bg-emerald-200 flex items-center justify-center -mr-1">
            <Avatar className="size-full rounded-full overflow-hidden">
              <AvatarImage className="object-cover h-full" src={peers[1].avatarUrl || ""} />
              <AvatarFallback className="text-[6px]">
                {peers[1].firstName[0]}
                {peers[1].lastName[0]}
              </AvatarFallback>
            </Avatar>
          </div>
          <div className="size-3.5 rounded-full outline-solid outline-2 outline-white overflow-hidden bg-emerald-200 flex items-center justify-center">
            <Avatar className="size-full rounded-full overflow-hidden">
              <AvatarImage className="object-cover h-full" src={peers[0].avatarUrl || ""} />
              <AvatarFallback className="text-[6px]">
                {peers[0].firstName[0]}
                {peers[0].lastName[0]}
              </AvatarFallback>
            </Avatar>
          </div>
        </div>
      : peers.length >= 3 ?
        <div className="group absolute bottom-0 right-0 flex overflow-visible">
          <div className="pointer-events-none absolute bottom-full right-0 mb-1 hidden group-hover:flex gap-1 rounded-md outline-solid outline-2 outline-white bg-white shadow-md p-1">
            {peers.map((peer, i) => (
              <div
                key={i}
                className="size-8 rounded-full outline-solid outline-2 outline-white overflow-hidden bg-emerald-200"
              >
                <Avatar className="size-full">
                  <AvatarImage className="object-cover h-full" src={peer.avatarUrl || ""} />
                  <AvatarFallback className="text-xs">
                    {peer.firstName[0]}
                    {peer.lastName[0]}
                  </AvatarFallback>
                </Avatar>
              </div>
            ))}
          </div>
          <div className="size-3.5 rounded-full outline-solid outline-2 outline-white overflow-hidden bg-slate-500 flex items-center justify-center text-white -mr-1">
            <span className="text-[5px] font-bold">+{peers.length - 1}</span>
          </div>
          <div className="size-3.5 rounded-full outline-solid outline-2 outline-white overflow-hidden bg-emerald-200 flex items-center justify-center">
            <Avatar className="size-full rounded-full overflow-hidden">
              <AvatarImage className="object-cover h-full" src={peers[0].avatarUrl || ""} />
              <AvatarFallback className="text-[6px]">
                {peers[0].firstName[0]}
                {peers[0].lastName[0]}
              </AvatarFallback>
            </Avatar>
          </div>
        </div>
      : status ?
        <div
          className={clsx("absolute bottom-0 right-0 size-2 outline-solid outline-3 outline-white rounded-full", {
            "bg-emerald-500": status === "online",
            "bg-red-400": status === "offline",
          })}
        />
      : null}
    </div>
  );
};
