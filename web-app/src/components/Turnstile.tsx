import { forwardRef, useImperativeHandle, useRef } from "react";
import { Turnstile as CfTurnstile, TurnstileInstance } from "@marsidev/react-turnstile";
import { TURNSTILE_SITE_KEY } from "@/lib/turnstile";

export interface TurnstileHandle {
  reset: () => void;
}

interface TurnstileProps {
  // action is verified server-side and must match the route (e.g. "signin").
  action: string;
  // onToken receives the token on success, or "" when it expires/errors.
  onToken: (token: string) => void;
}

/**
 * Thin wrapper around @marsidev/react-turnstile. It keeps callers decoupled from
 * the library and gates on the build-time site key: when it's unset the widget
 * renders nothing and the backend skips verification, so email/password auth
 * still works on local/self-host builds.
 *
 * "interaction-only" keeps the widget hidden unless a human challenge is actually
 * required. Tokens are single-use, so reset the widget after every failed
 * submission via the ref handle.
 */
export const Turnstile = forwardRef<TurnstileHandle, TurnstileProps>(function Turnstile({ action, onToken }, ref) {
  const widgetRef = useRef<TurnstileInstance>(null);

  useImperativeHandle(ref, () => ({
    reset: () => {
      widgetRef.current?.reset();
      onToken("");
    },
  }));

  if (!TURNSTILE_SITE_KEY) {
    return null;
  }

  return (
    <CfTurnstile
      ref={widgetRef}
      siteKey={TURNSTILE_SITE_KEY}
      options={{ action, theme: "auto", size: "flexible", appearance: "interaction-only" }}
      onSuccess={onToken}
      onExpire={() => onToken("")}
      onError={() => onToken("")}
      className="flex justify-center"
    />
  );
});
