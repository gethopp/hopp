import { getCurrentWindow } from "@tauri-apps/api/window";
import { OS } from "@/constants";
import { typedInvoke } from "@/core_payloads";
import useStore from "@/store/store";
import { checkForUpdates, downloadUpdateInBackground, installAndRelaunch } from "@/update";

/** Safe to interrupt: no call activity and the window isn't in front of the user. */
async function isIdle(): Promise<boolean> {
  const { callTokens, incomingCallCallerId, calling, updateInProgress } = useStore.getState();
  if (callTokens || incomingCallCallerId || calling || updateInProgress) return false;
  return !(await getCurrentWindow().isFocused());
}

/** Refresh the update badge, and on macOS auto-install when the user is idle. */
export async function pollUpdates(setNeedsUpdate: (needsUpdate: boolean) => void) {
  try {
    const update = await checkForUpdates();
    setNeedsUpdate(update !== null);

    if (OS !== "macos" || !update) return;

    const { auto_update_enabled } = await typedInvoke("get_user_settings");
    if (!auto_update_enabled) return;

    await downloadUpdateInBackground();

    if (await isIdle()) {
      useStore.getState().setUpdateInProgress(true);
      await installAndRelaunch();
    }
  } catch (err) {
    console.error("Auto-update poll failed:", err);
    useStore.getState().setUpdateInProgress(false);
  }
}
