// TODO: Create a shared package in the monorepo for these constants

// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-ignore - VITE_API_BASE_URL is defined by us with Vite naming convention
const explicitBase = import.meta.env.VITE_API_BASE_URL as string | undefined;

// When VITE_API_BASE_URL is set at build time (dev / official prod), bake the
// full https URL. When unset (self-host image), derive at runtime from
// window.location so a single image works for any domain + protocol.
const baseUrl =
  explicitBase ? `https://${explicitBase}`
  : typeof window !== "undefined" ? window.location.origin
  : "https://localhost:1926";

export const URLS = {
  API_BASE_URL: explicitBase || "localhost:1926",
} as const;

export const META = {
  // eslint-disable-next-line @typescript-eslint/ban-ts-comment
  // @ts-ignore - MODE is set by the Vite environment
  DEV_MODE: import.meta.env.MODE === "development",
};

export const BACKEND_URLS = {
  BASE: baseUrl,
  AUTHENTICATE_APP: `${baseUrl}/api/auth/authenticate-app`,
  INVITATION_DETAILS: (uuid: string) => `${baseUrl}/api/invitation-details/${uuid}`,
} as const;
