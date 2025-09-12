import { Avatar, AvatarFallback, AvatarImage } from "@radix-ui/react-avatar";
import { clsx } from "clsx";

type Status = "online" | "offline";

interface HoppAvatarProps {
  src?: string;
  firstName: string;
  lastName: string;
  status?: Status;
  className?: string;
}

export const HoppAvatar = ({ src, firstName, lastName, status, className }: HoppAvatarProps) => {
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
      {status && (
        <div
          className={clsx("absolute bottom-0 right-0 size-2 outline-solid outline-3 outline-white rounded-full", {
            "bg-emerald-500": status === "online",
            "bg-red-400": status === "offline",
          })}
        />
      )}
    </div>
  );
};
