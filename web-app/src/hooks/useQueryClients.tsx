import { createContext, useContext, useMemo, type ReactNode } from "react";
import createFetchClient from "openapi-fetch";
import createClient, { type OpenapiQueryClient } from "openapi-react-query";
import type { paths } from "../openapi";
import { useHoppStore } from "@/store/store";
import { BACKEND_URLS } from "@/constants";

// Custom error type for fetch errors
export interface FetchError extends Error {
  name: "FetchError";
  response: Response;
}

// Type-guard for FetchError
export const isFetchError = (error: unknown): error is FetchError => {
  return error instanceof Error && error.name === "FetchError";
};

type QueryContextType = {
  fetchClient: ReturnType<typeof createFetchClient<paths>>;
  apiClient: OpenapiQueryClient<paths>;
};

const QueryContext = createContext<QueryContextType | null>(null);

interface QueryProviderProps {
  children: ReactNode;
}

export function QueryProvider({ children }: QueryProviderProps) {
  const authToken = useHoppStore((state) => state.authToken);

  const fetchClient = useMemo(
    () =>
      createFetchClient<paths>({
        baseUrl: BACKEND_URLS.BASE,
        headers:
          authToken ?
            {
              Authorization: `Bearer ${authToken}`,
            }
          : undefined,
      }),
    [authToken],
  );

  const apiClient = useMemo(() => createClient<paths>(fetchClient), [fetchClient]);

  if (fetchClient) {
    fetchClient.use({
      async onResponse({ response }) {
        if (!response.ok) {
          // Create a custom error with response details
          const error = new Error(`HTTP ${response.status}: ${response.statusText}`) as FetchError;
          error.name = "FetchError";
          error.response = response;

          throw error;
        }
        return response;
      },
    });
  }

  const value = useMemo(
    () => ({
      fetchClient,
      apiClient,
    }),
    [fetchClient, apiClient],
  );

  return <QueryContext.Provider value={value}>{children}</QueryContext.Provider>;
}

export function useFetchClient() {
  const context = useContext(QueryContext);
  if (!context) {
    throw new Error("useFetchClient must be used within a QueryProvider");
  }
  return context.fetchClient;
}

export function useAPI() {
  const context = useContext(QueryContext);
  if (!context) {
    throw new Error("useAPI must be used within a QueryProvider");
  }
  return context.apiClient;
}
