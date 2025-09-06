import React, { useEffect } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { HiOutlineUsers, HiOutlineLockOpen, HiOutlineUserPlus, HiOutlineMinus } from "react-icons/hi2";
import { Separator } from "../ui/separator";
import { clsx } from "clsx";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { invoke } from "@tauri-apps/api/core";
import useStore, { Tab } from "@/store/store";
import { HiOutlineAnnotation, HiOutlineDotsHorizontal, HiOutlineUserGroup } from "react-icons/hi";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { appVersion, tauriUtils } from "@/windows/window-utils.ts";
import { OS } from "@/constants";

const SidebarButton = ({
  active,
  children,
  label,
  ...rest
}: {
  label: React.ReactNode;
  active?: boolean;
} & React.ButtonHTMLAttributes<HTMLButtonElement>) => {
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          className={clsx(
            "p-1.5 rounded-md flex items-center justify-center size-8",
            !active && "hover:bg-gray-200",
            active && "bg-white shadow-xs outline-solid outline-1 outline-slate-200",
          )}
          {...rest}
        >
          {children}
        </button>
      </TooltipTrigger>
      <TooltipContent side="right">{label}</TooltipContent>
    </Tooltip>
  );
};

const getAvailableTabs = (
  hasUser: boolean,
): Array<{
  label: string;
  icon: React.ReactNode;
  key: Tab;
}> => {
  const baseTabs =
    !hasUser ?
      [
        {
          label: "Login",
          icon: <HiOutlineLockOpen className="size-4 stroke-[1.5]" />,
          key: "login",
        } as const,
      ]
    : [
        {
          label: "User List",
          icon: <HiOutlineUsers className="size-4 stroke-[1.5]" />,
          key: "user-list",
        } as const,
        {
          label: "Rooms",
          icon: <HiOutlineUserGroup className="size-4 stroke-[1.5]" />,
          key: "rooms",
        } as const,
        {
          label: "Invite",
          icon: <HiOutlineUserPlus className="size-4 stroke-[1.5]" />,
          key: "invite",
        } as const,
        {
          label: "Report issue",
          icon: <HiOutlineAnnotation className="size-4 stroke-[1.5]" />,
          key: "report-issue",
        } as const,
      ];

  return [
    ...baseTabs,
    // ...[
    //   {
    //     label: "Debug",
    //     icon: <HiOutlineBugAnt className="size-4" />,
    //     key: "debug",
    //   } as const,
    // ],
  ];
};

export const Sidebar = () => {
  const { tab, setTab, user, reset } = useStore();

  useEffect(() => {
    // If user is not set, show login tab
    if (!user) {
      setTab("login");
    }
  }, [user]);

  return (
    <TooltipProvider>
      <div className="w-[50px] min-w-[50px] h-full bg-slate-100 border-r border-gray-200 flex flex-col">
        <div className="py-3 flex flex-col gap-2 items-center">
          {getAvailableTabs(!!user).map((t) => (
            <SidebarButton key={t.key} active={t.key === tab} label={t.label} onClick={() => setTab(t.key)}>
              {t.icon}
            </SidebarButton>
          ))}
          {OS === "windows" && (
            <SidebarButton label="Minimize" onClick={() => tauriUtils.minimizeMainWindow()}>
              <HiOutlineMinus className="size-4" />
            </SidebarButton>
          )}
        </div>
        <Separator className="w-[70%] mx-auto" />
        {/* Bottom user section */}
        <div className="mt-auto h-12 w-full flex items-center justify-center">
          <DropdownMenu>
            <DropdownMenuTrigger>
              {!user && (
                <div className="size-9 shrink-0 rounded-md flex justify-center items-center text-gray-600 outline-solid outline-1 outline-gray-300 shadow-xs cursor-pointer">
                  <HiOutlineDotsHorizontal />
                </div>
              )}
              {user && (
                <div
                  className={clsx(
                    "size-9 shrink-0 rounded-md flex justify-center items-center text-gray-600 outline-solid outline-1 outline-gray-300 shadow-xs cursor-pointer",
                    !user.avatar_url && "bg-gray-200",
                  )}
                  style={{
                    background: user.avatar_url ? `url(${user.avatar_url}) center center/cover no-repeat` : undefined,
                  }}
                >
                  {user.avatar_url ? "" : user.first_name.charAt(0).toUpperCase()}
                </div>
              )}
            </DropdownMenuTrigger>
            <DropdownMenuContent className="w-[200px]" side="top" align="start">
              <DropdownMenuItem onClick={() => openUrl("https://pair.gethopp.app")}>Profile</DropdownMenuItem>
              <DropdownMenuItem onClick={() => setTab("debug")}>Debug</DropdownMenuItem>
              <DropdownMenuSeparator />
              <DropdownMenuItem
                onClick={async () => {
                  reset();
                  await invoke("delete_stored_token");
                }}
              >
                Sign-out
              </DropdownMenuItem>
              <DropdownMenuSeparator />
              <div className="muted px-2 py-0.5">App version: {appVersion}</div>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      </div>
    </TooltipProvider>
  );
};
