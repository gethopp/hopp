import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar";
import { HiHome, HiCog6Tooth, HiUserGroup, HiArrowRightStartOnRectangle, HiCreditCard } from "react-icons/hi2";
import { PiPlugsFill } from "react-icons/pi";
import Logo from "@/assets/Hopp.png";
import DoorImage from "@/assets/door.png";
import { Button } from "./ui/button";
import { resetAllStores, useHoppStore } from "@/store/store";
import { useAPI } from "@/hooks/useQueryClients";
import { useLocation } from "react-router-dom";
import { Badge } from "./ui/badge";

const items = [
  {
    title: "Dashboard",
    url: "/dashboard",
    icon: HiHome,
  },
  {
    title: "Settings",
    url: "/settings",
    icon: HiCog6Tooth,
  },
  {
    title: "Teammates",
    url: "/teammates",
    icon: HiUserGroup,
  },
  {
    title: "Integrations",
    url: "/integrations",
    icon: PiPlugsFill,
  },
];

function RoomsBetaBanner() {
  return (
    <div className="relative flex flex-col rounded-xl bg-white shadow-xs p-3 mt-auto">
      <p className="text-sm font-semibold text-slate-800">Rooms are in beta</p>
      <div className="relative mt-3 w-full">
        <img
          className="aspect-video w-full rounded-lg object-contain bg-white"
          alt="Room illustration"
          src={DoorImage}
        />
      </div>
      <p className="mt-3 text-xs text-slate-600 leading-relaxed text-balance">
        You cannot remote control your teammates computer from the web interface.
        <br />
        <br />
        But you can still see your teammates cursors.
      </p>
      <div className="mt-3">
        <a
          href="https://docs.gethopp.app/features/rooms/"
          target="_blank"
          rel="noopener noreferrer"
          className="text-xs font-medium text-blue-600 hover:text-blue-700 hover:underline"
        >
          Read the docs
        </a>
      </div>
    </div>
  );
}

export function HoppSidebar() {
  const setAuthToken = useHoppStore((state) => state.setAuthToken);
  const location = useLocation();

  const { useQuery } = useAPI();
  const authToken = useHoppStore((store) => store.authToken);

  // Check if user is in a room based on the URL path
  const isInRoom = location.pathname.startsWith("/room/");

  // TODO: add user object in store
  const { data: user } = useQuery("get", "/api/auth/user", undefined, {
    queryHash: `user-${authToken}`,
    select: (data) => data,
    enabled: !!authToken,
    refetchInterval: 10_000,
  });

  // Add subscription item for admin users
  const navigationItems = [
    ...items,
    ...(user?.is_admin ?
      [
        {
          title: "Subscription",
          url: "/subscription",
          icon: HiCreditCard,
        },
      ]
    : []),
  ];

  return (
    <Sidebar className="px-1 py-3 bg-sidebar">
      <SidebarHeader>
        <img src={Logo} alt="Hopp Logo" className="mr-auto h-[40px]" />
      </SidebarHeader>
      <SidebarContent>
        <SidebarGroup>
          <SidebarMenu>
            {navigationItems.map((item) => (
              <SidebarMenuItem key={item.title}>
                <SidebarMenuButton asChild>
                  <a href={item.url}>
                    <item.icon />
                    <span>{item.title}</span>
                    {item.title === "Integrations" && (
                      <Badge className="scale-80" variant="secondary">
                        New
                      </Badge>
                    )}
                  </a>
                </SidebarMenuButton>
              </SidebarMenuItem>
            ))}
          </SidebarMenu>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter>
        {isInRoom && (
          <SidebarGroup className="p-0">
            <RoomsBetaBanner />
          </SidebarGroup>
        )}
        <Button
          variant="outline"
          className="w-full flex flex-row justify-start max-w-min items-start gap-2"
          onClick={() => {
            resetAllStores();
            setAuthToken(null);
          }}
        >
          <HiArrowRightStartOnRectangle /> Logout
        </Button>
      </SidebarFooter>
    </Sidebar>
  );
}
