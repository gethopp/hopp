// The public Turnstile site key is baked in at build time via the standard Vite
// VITE_CLOUDFLARE_SITE_KEY variable. When it's unset the widget renders nothing
// and the backend skips verification, so email/password auth still works on
// local/self-host builds.
export const TURNSTILE_SITE_KEY = import.meta.env.VITE_CLOUDFLARE_SITE_KEY;

// isTurnstileEnabled lets callers require a token before submitting.
export const isTurnstileEnabled = Boolean(TURNSTILE_SITE_KEY);
