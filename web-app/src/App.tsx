import { ReactQueryDevtools } from "@tanstack/react-query-devtools";
import { createBrowserRouter, Outlet, RouterProvider, redirect } from "react-router-dom";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { useHoppStore, useHydration } from "./store/store";
import { LoginForm } from "@/pages/Login";
import { ForgotPassword } from "@/pages/ForgotPassword";
import { ResetPassword } from "@/pages/ResetPassword";
import { Dashboard } from "@/pages/Dashboard";
import { useNavigate } from "react-router-dom";
import { Toaster, toast } from "react-hot-toast";
import { QueryProvider } from "./hooks/useQueryClients";
import { SidebarProvider } from "@/components/ui/sidebar";
import { HoppSidebar } from "./components/sidebar";
import { Settings } from "./pages/Settings";
import { Teammates } from "./pages/Teammates";
import { Subscription } from "./pages/Subscription";
import { Room } from "./pages/Room";
import { BACKEND_URLS, META } from "./constants";
import { PostHogProvider } from "posthog-js/react";
import { PostHogConfig } from "posthog-js";
import { InvitationHandler } from "./pages/InvitationHandler";

const options: Partial<PostHogConfig> = {
  api_host: "https://eu.i.posthog.com",
  loaded: function (ph) {
    if (META.DEV_MODE) {
      ph.opt_out_capturing(); // opts a user out of event capture
      ph.set_config({ disable_session_recording: true });
    }
  },
};

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 5 * 60 * 1000, // 5 minutes
      retry: 1,
    },
  },
});

const Providers = ({ requireAuth, overrideRedirect = false }: { requireAuth: boolean; overrideRedirect?: boolean }) => {
  const hasHydrated = useHydration();
  const navigate = useNavigate();
  const authToken = useHoppStore((state) => state.authToken);

  if (!hasHydrated) {
    return <div>Loading...</div>;
  }

  if (!requireAuth && authToken && !overrideRedirect) navigate("/dashboard");
  if (requireAuth && !authToken && !overrideRedirect) navigate("/login");

  return (
    <div className="w-screen h-screen">
      <PostHogProvider apiKey="phc_qOumHIIkywfbcmxjoI84orWP5Wo2oZVamh83bOUeF5x" options={options}>
        <QueryClientProvider client={queryClient}>
          <QueryProvider>
            <ReactQueryDevtools initialIsOpen={false} />
            {!authToken && <Outlet />}
            {authToken && (
              <SidebarProvider>
                <HoppSidebar />
                <main className="p-8 w-full">
                  <Outlet />
                </main>
              </SidebarProvider>
            )}
          </QueryProvider>
        </QueryClientProvider>
      </PostHogProvider>
    </div>
  );
};

const router = createBrowserRouter([
  {
    path: "/login",
    element: <Providers requireAuth={false} />,
    children: [
      {
        path: "",
        element: <LoginForm />,
      },
    ],
  },
  {
    path: "/forgot-password",
    element: <Providers requireAuth={false} />,
    children: [
      {
        path: "",
        element: <ForgotPassword />,
      },
    ],
  },
  {
    path: "/reset-password/:token",
    element: <Providers requireAuth={false} />,
    children: [
      {
        path: "",
        element: <ResetPassword />,
      },
    ],
  },
  {
    path: "/invitation/:uuid",
    element: <Providers requireAuth={false} overrideRedirect={true} />,
    children: [
      {
        path: "",
        element: <InvitationHandler />,
      },
    ],
  },
  {
    path: "/room/:roomId",
    element: <Providers requireAuth={true} />,
    children: [
      {
        path: "",
        element: <Room />,
      },
    ],
  },
  {
    path: "/subscription/success",
    element: <Providers requireAuth={true} />,
    loader: () => redirect("/dashboard?subscription_success=true"),
  },
  {
    path: "/",
    element: <Providers requireAuth={true} />,
    children: [
      {
        index: true,
        loader: () => redirect("/dashboard"),
      },
      {
        path: "dashboard",
        element: <Dashboard />,
      },
      {
        path: "settings",
        element: <Settings />,
      },
      {
        path: "teammates",
        element: <Teammates />,
      },
      {
        path: "subscription",
        element: <Subscription />,
      },
      {
        path: "login-app",
        element: <LoginForm />,
        children: [
          {
            path: "",
            element: <LoginForm />,
            loader: async () => {
              try {
                // There might be a case that the user is authenticated but on a new tab,
                // the token does not exist in state.
                // In this case, we need wait for hydration to complete and then check if the token exists.
                // Wait until the store has hydrated
                let tries = 0;
                while (!useHoppStore.persist.hasHydrated()) {
                  console.log("Waiting for hydration to complete...");
                  await new Promise((resolve) => setTimeout(resolve, 200));
                  tries++;
                  if (tries > 10) {
                    throw new Error("hydration timeout in login-app");
                  }
                }

                const authToken = useHoppStore.getState().authToken;
                if (authToken) {
                  const response = await fetch(BACKEND_URLS.AUTHENTICATE_APP, {
                    headers: {
                      Authorization: `Bearer ${authToken}`,
                    },
                  });

                  const data = await response.json();

                  toast.success("Opening Hopp app...", { duration: 1_000 });

                  // Open the app with the token
                  window.open(`hopp:///authenticate?token=${data.token}`, "_blank");

                  return redirect("/dashboard?show_app_token_banner=true");
                } else {
                  return redirect("/login?redirect_to_app=true");
                }
              } catch (error) {
                toast.error("Failed to authenticate app, go to settings page and copy manually your token");
                console.error(error);
                return redirect("/settings");
              }
            },
          },
        ],
      },
    ],
  },
  {
    path: "*",
    element: <Providers requireAuth={true} />,
    loader: () => redirect("/dashboard"),
  },
]);

function App() {
  return (
    <div>
      <Toaster
        position="bottom-center"
        toastOptions={{
          duration: 5000,
        }}
      />
      <RouterProvider router={router} />
    </div>
  );
}

export default App;
