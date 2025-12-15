import createFetchClient from "openapi-fetch";
import toast from "react-hot-toast";
import useStore from "@/store/store";
import { tauriUtils } from "@/windows/window-utils";
import { paths } from "@/openapi";
import { Constants } from "@/constants";

/**
 * Validates an authentication token by fetching user details.
 * If valid, it sets the token in the global store and Tauri backend.
 * If invalid, it shows an error toast and clears the store.
 */
export const validateAndSetAuthToken = async (token: string) => {
  const { reset, setUser, setAuthToken } = useStore.getState();
  if (!token) {
    reset(); // Resets the store to initial state, including clearing token and user
    await tauriUtils.deleteStoredToken(); // Clear token in backend
  }

  const tempFetchClient = createFetchClient<paths>({
    baseUrl: Constants.backendUrl,
    headers: {
      Authorization: `Bearer ${token}`,
    },
  });

  try {
    const { data, error, response } = await tempFetchClient.GET("/api/auth/user");

    if (response.ok && data) {
      await tauriUtils.storeTokenBackend(token);
      setAuthToken(token);
      setUser(data);
      await tauriUtils.storeTokenBackend(token);
      toast.success("Successfully authenticated!");
    } else if (response.status === 401) {
      const { reset } = useStore.getState();
      reset();
      await tauriUtils.deleteStoredToken();
      toast.error("Authentication failed, try to login again");
    } else {
      toast.error(`Authentication verification unexpectedly, try to login again`);
      console.error("Auth validation failed", error, response);
    }
  } catch (e: unknown) {
    console.error("Exception during token validation:", e);
    toast.error("An unexpected error occurred during token validation. Please try again.");
  }
};
