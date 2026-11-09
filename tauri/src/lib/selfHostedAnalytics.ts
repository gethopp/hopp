import { useEffect } from "react";
import { usePostHog } from "posthog-js/react";
import { getVersion } from "@tauri-apps/api/app";
import { Constants, OS, isSelfHostedUrl } from "@/constants";
import useStore from "@/store/store";

const SEND_INTERVAL_MS = 30 * 60 * 1000;

/**
 * Inspired from the snippet below to avoid adding dep:
 * https://stackoverflow.com/a/48161723
 */
async function sha256Hex(input: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(input));
  return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, "0")).join("");
}

/**
 * Sends an anonymised usage event for clients pointing at a self-hosted backend.
 * Cloud users are skipped because the Hopp backend already has information.
 */
export function useSelfHostedAnalytics() {
  const posthog = usePostHog();
  const isSelfHosted = useStore((state) => isSelfHostedUrl(state.customServerUrl));
  const userId = useStore((state) => state.user?.id);
  const teammatesLoaded = useStore((state) => state.teammates !== null);

  useEffect(() => {
    if (!isSelfHosted || !userId || !teammatesLoaded) return;

    const send = async () => {
      const { user, teammates } = useStore.getState();
      if (!user?.team_id || !teammates) return;

      const host = Constants.normalizedHost;
      const [teamHash, userHash, instanceHash] = await Promise.all([
        sha256Hex(`team:${host}:${user.team_id}`),
        sha256Hex(`user:${user.id}`),
        sha256Hex(`instance:${host}`),
      ]);

      posthog.capture("self_hosted_analytics", {
        team_hash: teamHash,
        user_hash: userHash,
        instance_hash: instanceHash,
        team_size: teammates.length + 1,
        app_version: await getVersion(),
        os: OS,
      });
    };

    const sendSafely = () => send().catch((err) => console.error("Failed to send self-hosted analytics", err));

    sendSafely();
    const interval = setInterval(sendSafely, SEND_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [isSelfHosted, userId, teammatesLoaded, posthog]);
}
