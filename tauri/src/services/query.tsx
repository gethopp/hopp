import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import createFetchClient from "openapi-fetch";
import createClient, { type OpenapiQueryClient } from "openapi-react-query";
import type { paths } from "../openapi";
import useStore from "../store/store";
import { getBackendBaseUrl } from "@/constants";
import { tauriUtils } from "@/windows/window-utils";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type QueryContextType = {
  fetchClient: ReturnType<typeof createFetchClient<paths>>;
  apiClient: OpenapiQueryClient<paths>;
};

const QueryContext = createContext<QueryContextType | null>(null);

interface QueryProviderProps {
  children: ReactNode;
}

export function QueryProvider({ children }: QueryProviderProps) {
  const [authToken, setAuthToken] = useState<string | null>(null);
  const customServerUrl = useStore((state) => state.customServerUrl);

  // Load stored token on app start
  useEffect(() => {
    (async () => {
      if (!isTauri()) return;
      const token = await tauriUtils.getStoredToken();
      if (token) {
        setAuthToken(token);
      }
    })();
  }, []);

  useEffect(() => {
    // Update auth token when it changes in the backend
    const setupChangeTokenListener = async () => {
      const unlistenFn = await listen("token_changed", (event) => {
        const token = event.payload as string;
        if (token) {
          setAuthToken(token);
        }
      });

      return unlistenFn;
    };

    let unlistenChangeToken: (() => void) | undefined;
    setupChangeTokenListener().then((fn) => {
      unlistenChangeToken = fn;
    });

    return () => {
      if (unlistenChangeToken) unlistenChangeToken();
    };
  }, []);

  const fetchClient = useMemo(
    () =>
      createFetchClient<paths>({
        baseUrl: getBackendBaseUrl(),
        headers:
          authToken ?
            {
              Authorization: `Bearer ${authToken}`,
            }
          : undefined,
      }),
    [authToken, customServerUrl],
  );

  const apiClient = useMemo(() => createClient<paths>(fetchClient), [fetchClient]);

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
