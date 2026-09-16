/// <reference types="vite/client" />

interface ImportMetaEnv {
  // Public Cloudflare Turnstile site key, exposed to the client via the standard
  // VITE_ prefix. Optional: left unset on self-host builds that disable Turnstile.
  readonly VITE_CLOUDFLARE_SITE_KEY?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
