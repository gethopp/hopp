import { useCallback, useEffect, useState } from "react";
import { getHoppConnection, type HoppConnection } from "./hopp";

interface State<T> {
  data: T | null;
  error: Error | null;
  isLoading: boolean;
}

export interface HoppApiResult<T> extends State<T> {
  revalidate: () => void;
}

/**
 * Fetches an authenticated endpoint on the Hopp backend using the session
 * stored by the desktop app. `path` is the API path, e.g. `/api/auth/rooms`.
 */
export function useHoppApi<T>(path: string): HoppApiResult<T> {
  const [state, setState] = useState<State<T>>({ data: null, error: null, isLoading: true });
  const [nonce, setNonce] = useState(0);

  useEffect(() => {
    const controller = new AbortController();

    const run = async () => {
      setState((previous) => ({ ...previous, isLoading: true }));

      let connection: HoppConnection;
      try {
        connection = getHoppConnection();
      } catch (error) {
        if (!controller.signal.aborted) {
          setState({ data: null, error: toError(error), isLoading: false });
        }
        return;
      }

      try {
        const response = await fetch(`https://${connection.serverUrl}${path}`, {
          headers: connection.headers,
          signal: controller.signal,
        });
        if (!response.ok) {
          throw new Error(apiErrorMessage(response.status));
        }
        const data = (await response.json()) as T;
        if (!controller.signal.aborted) {
          setState({ data, error: null, isLoading: false });
        }
      } catch (error) {
        if (!controller.signal.aborted) {
          setState({ data: null, error: toError(error), isLoading: false });
        }
      }
    };

    void run();
    return () => controller.abort();
  }, [path, nonce]);

  const revalidate = useCallback(() => setNonce((n) => n + 1), []);

  return { ...state, revalidate };
}

function toError(error: unknown): Error {
  return error instanceof Error ? error : new Error(String(error));
}

function apiErrorMessage(status: number): string {
  switch (status) {
    case 401:
      return "Hopp session expired, sign in to the app again";
    case 403:
      return "You don't have access to this resource";
    case 404:
      return "Not found";
    default:
      return `Hopp API request failed (${status})`;
  }
}
