import React, { useEffect } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { HiOutlineUsers, HiOutlineLockOpen, HiOutlineUserPlus, HiOutlineMinus } from "react-icons/hi2";
import { differenceInDays, parseISO } from "date-fns";
import { Separator } from "../ui/separator";
import { clsx } from "clsx";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { invoke } from "@tauri-apps/api/core";
import useStore, { Tab } from "@/store/store";
import { components } from "@/openapi";
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
          label: "Broken again?",
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

const TrialCountdownAvatarFill = ({ user }: { user: components["schemas"]["PrivateUser"] }) => {
  // Only show if user is in trial
  if (!user.is_trial || !user.trial_ends_at) {
    return null;
  }

  console.log("user", user);

  // Uncomment and modify value to test visual changes
  // const end = "2025-10-05T17:20:32.677+02:00";
  // const trialEndDate = parseISO(end);
  const trialEndDate = parseISO(user.trial_ends_at);
  const currentDate = new Date();
  const daysRemaining = differenceInDays(trialEndDate, currentDate);

  // Don't show if trial has expired
  if (daysRemaining <= 0) {
    return null;
  }

  // Calculate percentage based on days remaining (max 30 days trial)
  const maxTrialDays = 30;
  const percentage = Math.min(100, Math.max(5, (daysRemaining / maxTrialDays) * 100)); // Min 5% to always be visible

  // Color intensity based on urgency
  const getTextColor = (days: number) => {
    if (days <= 3) return "text-red-800";
    if (days <= 7) return "text-orange-800";
    if (days <= 14) return "text-yellow-800";
    return "text-green-800";
  };

  const getBackgroundColor = (days: number) => {
    if (days <= 3) return "#fca5a5"; // red-300
    if (days <= 7) return "#fdba74"; // orange-300
    if (days <= 14) return "#fde047"; // yellow-300
    return "#86efac"; // green-300
  };

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <div
          className={clsx(
            "relative flex items-center size-9 justify-center rounded-md bg-white text-sm font-semibold shadow-xs cursor-pointer overflow-hidden",
            getTextColor(daysRemaining),
          )}
        >
          {/* Background fill from bottom */}
          <div
            className="absolute bottom-0 left-0 right-0 rounded-b-md transition-all duration-300"
            style={{
              height: `${percentage}%`,
              backgroundColor: getBackgroundColor(daysRemaining),
            }}
          />
          {/* Content */}
          <span className="relative z-10">{daysRemaining}</span>
        </div>
      </TooltipTrigger>
      <TooltipContent side="right">
        Trial expires in {daysRemaining} day{daysRemaining !== 1 ? "s" : ""}
      </TooltipContent>
    </Tooltip>
  );
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
        <div className="flex flex-col mt-auto">
          <div className="flex flex-col gap-2 items-center">{user && <TrialCountdownAvatarFill user={user} />}</div>
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
                <div className="muted text-slate-500 px-2 py-0.5">App version: {appVersion}</div>
              </DropdownMenuContent>
            </DropdownMenu>
          </div>
        </div>
      </div>
    </TooltipProvider>
  );
};
