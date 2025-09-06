import { Button } from "@/components/ui/button";
import { open } from "@tauri-apps/plugin-shell";
import dotsBackground from "../../assets/dots.svg";
import logo from "../../assets/Hopp.png";
import { BlurIn } from "@/components/ui/text-effects";
import { motion } from "framer-motion";
import { BACKEND_URLS } from "@/constants";
import { CopiableInput } from "@/components/ui/copiable-input";
import { Separator } from "@/components/ui/separator";
import { usePostHog } from "posthog-js/react";

export const Login = () => {
  const posthog = usePostHog();

  return (
    <div className="w-full h-full flex flex-row items-center justify-center">
      <div className="w-[280px] h-[300px]">
        <div className="flex flex-col items-center justify-center relative mt-12 mb-8 w-full">
          <motion.img
            src={logo}
            alt="Hopp Logo"
            className="w-[90px] h-auto z-10 logo-hover"
            whileHover={{
              rotate: [-0.5, 0.5, -0.5],
              transition: {
                duration: 0.5,
                repeat: Infinity,
                ease: "easeInOut",
              },
            }}
          />
          <img
            src={dotsBackground}
            alt="dotted-background"
            className="absolute w-[110%] object-cover h-auto z-1"
            style={{
              transform: "scale(1.3) translate(0px, -5px)",
            }}
          />
        </div>
        <div className="flex flex-col items-start justify-start mt-12">
          <BlurIn as="h4" sentence="Hopp on a call 📞" className="mb-2" />
          <div className="flex flex-col items-start justify-start leading-relaxed">
            <p className="leading-normal">
              We are building Hopp to deliver <span className="font-semibold whitespace-nowrap">low-latency</span>,
              <span className="font-semibold whitespace-nowrap"> crystal-clear</span> screen sharing that feels like
              you're coding side-by-side.
            </p>
          </div>
        </div>
        <div className="flex flex-col items-start justify-start gap-2">
          <Button
            className="mt-6"
            variant="gradient-white"
            onClick={() => {
              // The flow for login with JWT from the app is:
              // 1. Redirect to "/login-app" inside the web app
              // 2. If the web-app is authenticated, it will fetch a JWT token and redirect to the app (deeplink)
              // 3. If the web-app is not authenticated, it will redirect to the login page, then keep track somehow of the state to redirect back to the app (deeplink)
              open(BACKEND_URLS.LOGIN_JWT);
              posthog.capture("user_click_jwt_sign");
            }}
          >
            Sign-in
          </Button>
          <div className="flex flex-row items-center justify-center gap-2">
            <Separator className="w-[20px]" /> or <Separator className="w-[20px]" />
          </div>
          <CopiableInput
            value={BACKEND_URLS.LOGIN_JWT}
            readOnly
            className="text-slate-600"
            onCopy={() => {
              // Send posthog event for custom URL sign-in
              posthog.capture("user_click_custom_url_sign");
            }}
          />
        </div>
      </div>
    </div>
  );
};
