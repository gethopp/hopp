import React, { useEffect, useMemo } from "react";
import throttle from "lodash/throttle";
import toast from "react-hot-toast";
import { openUrl } from "@tauri-apps/plugin-opener";
import { HiArrowDownTray, HiOutlineUsers, HiOutlineLockOpen, HiOutlineUserPlus, HiOutlineMinus } from "react-icons/hi2";
import { CgSpinner } from "react-icons/cg";
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
import { Constants, OS } from "@/constants";
import { useQueryClient } from "@tanstack/react-query";
import { downloadAndRelaunch } from "@/update";
import { LuCircleFadingArrowUp } from "react-icons/lu";

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

const DownloadNewVersionButton = () => {
  const { needsUpdate, updateInProgress, setUpdateInProgress } = useStore();

  if (!needsUpdate) {
    return null;
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          className="flex items-center justify-center rounded-lg bg-white bg-linear-to-b from-gray-100 p-1.5 border border-slate-300 mx-1 size-8 w-full hover:scale-[1.025] hover:shadow-xs transition-all duration-300"
          onClick={() => {
            setUpdateInProgress(true);
            void downloadAndRelaunch();
          }}
          disabled={updateInProgress}
        >
          {updateInProgress ?
            <CgSpinner className="animate-spin size-3.5 text-gray-800" />
          : <LuCircleFadingArrowUp className="size-3.5 text-gray-800" />}
        </button>
      </TooltipTrigger>
      <TooltipContent side="right">Download and install update</TooltipContent>
    </Tooltip>
  );
};

const TrialCountdownAvatarFill = ({ user }: { user: components["schemas"]["PrivateUser"] }) => {
  // Only show if user is in trial
  if (!user.is_trial || !user.trial_ends_at) {
    return null;
  }

  // Uncomment and modify value to test visual changes
  // const end = "2025-10-05T17:20:32.677+02:00";
  // const trialEndDate = parseISO(end);
  const trialEndDate = parseISO(user.trial_ends_at);
  const currentDate = new Date();
  const daysRemaining = differenceInDays(trialEndDate, currentDate);
  const isExpired = daysRemaining <= 0;
  const displayDays = Math.max(0, daysRemaining);

  // teams created before 2026-05-04 keep 30-day trial; new teams get 14.
  const TRIAL_GRANDFATHER_CUTOFF = new Date("2026-05-04T00:00:00Z");
  const userCreatedAt = user.created_at ? parseISO(user.created_at) : new Date();
  const maxTrialDays = userCreatedAt < TRIAL_GRANDFATHER_CUTOFF ? 30 : 14;
  const percentage = isExpired ? 100 : Math.min(100, Math.max(5, (daysRemaining / maxTrialDays) * 100));

  // Thresholds scale with maxTrialDays so bar starts green for both 14- and 30-day trials.
  // 30-day: yellow ≤14, orange ≤7, red ≤3 (matches prior behavior).
  // 14-day: yellow ≤7,  orange ≤3, red ≤1.
  const yellowAt = Math.ceil(maxTrialDays / 2);
  const orangeAt = Math.ceil(maxTrialDays / 4);
  const redAt = Math.max(3, Math.floor(maxTrialDays / 10));

  const getTextColor = (days: number) => {
    if (days <= redAt) return "text-red-800";
    if (days <= orangeAt) return "text-orange-800";
    if (days <= yellowAt) return "text-yellow-800";
    return "text-green-800";
  };

  const getBackgroundColor = (days: number) => {
    if (days <= redAt) return "#fca5a5";
    if (days <= orangeAt) return "#fdba74";
    if (days <= yellowAt) return "#fde047";
    return "#86efac";
  };

  const textColor = isExpired ? "text-red-800" : getTextColor(daysRemaining);
  const bgColor = isExpired ? "#fca5a5" : getBackgroundColor(daysRemaining);

  const handleClick = useMemo(
    () =>
      throttle(
        () => {
          if (user.is_admin) {
            void openUrl(Constants.webAppUrl + "/subscription");
          } else {
            toast("Contact your admin to manage your team's subscription.", { duration: 3000 });
          }
        },
        2000,
        { leading: true, trailing: false },
      ),
    [user.is_admin],
  );

  return (
    <div className="flex flex-col items-center">
      <Tooltip>
        <TooltipTrigger asChild>
          <div
            className={clsx(
              "relative flex items-center size-9 justify-center rounded-md bg-white text-sm font-semibold shadow-xs cursor-pointer overflow-hidden",
              textColor,
            )}
            onClick={handleClick}
          >
            {/* Background fill from bottom */}
            <div
              className="absolute bottom-0 left-0 right-0 rounded-b-md transition-all duration-300"
              style={{
                height: `${percentage}%`,
                backgroundColor: bgColor,
              }}
            />
            {/* Content */}
            <span className="relative z-10">{displayDays}</span>
          </div>
        </TooltipTrigger>
        <TooltipContent side="right">
          {isExpired ?
            "Trial expired, click to manage subscription"
          : `Trial expires in ${daysRemaining} day${daysRemaining !== 1 ? "s" : ""}, click to manage`}
        </TooltipContent>
      </Tooltip>
    </div>
  );
};

export const Sidebar = () => {
  const { tab, setTab, user, reset } = useStore();
  const queryClient = useQueryClient();

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
        <div className="flex flex-col gap-1 mt-auto">
          <div className="flex justify-center w-full">
            <DownloadNewVersionButton />
          </div>
          {user && <TrialCountdownAvatarFill user={user} />}
          <div className="mt-[-5px] h-12 w-full flex items-center justify-center">
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
                <DropdownMenuItem onClick={() => tauriUtils.openSettingsWindow()}>Settings</DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                  onClick={async () => {
                    queryClient.clear();
                    reset();
                    await invoke("delete_stored_token");
                  }}
                >
                  Sign-out
                </DropdownMenuItem>
                <DropdownMenuItem onClick={() => invoke("quit_app")}>Quit</DropdownMenuItem>
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
