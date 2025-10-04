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
import Logo from "@/assets/Hopp.png";
import { Button } from "./ui/button";
import { resetAllStores, useHoppStore } from "@/store/store";
import { useAPI } from "@/hooks/useQueryClients";

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
];

export function HoppSidebar() {
  const setAuthToken = useHoppStore((state) => state.setAuthToken);

  const { useQuery } = useAPI();
  const authToken = useHoppStore((store) => store.authToken);

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
                  </a>
                </SidebarMenuButton>
              </SidebarMenuItem>
            ))}
          </SidebarMenu>
        </SidebarGroup>
      </SidebarContent>
      <SidebarFooter>
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
