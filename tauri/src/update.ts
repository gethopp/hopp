import { check, Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

let pendingUpdate: Update | null = null;

export async function checkForUpdates() {
  const update = await check();
  if (update) {
    console.debug(`found update ${update.version} from ${update.date} with notes ${update.body}`);
  }
  return update;
}

export function hasPendingUpdate() {
  return pendingUpdate !== null;
}

export async function downloadUpdateInBackground() {
  if (pendingUpdate) return pendingUpdate;

  const update = await check();
  if (!update) return null;

  await update.download();
  pendingUpdate = update;
  console.debug(`downloaded update ${update.version} in background`);
  return update;
}

export async function installAndRelaunch() {
  if (!pendingUpdate) return false;

  await pendingUpdate.install();
  console.debug("update installed");
  await relaunch();
  return true;
}

export async function downloadAndRelaunch() {
  const update = await check();
  if (update) {
    let downloaded = 0;
    let contentLength: number | undefined = 0;
    await update.downloadAndInstall((event) => {
      switch (event.event) {
        case "Started":
          contentLength = event.data.contentLength;
          console.debug(`started downloading ${event.data.contentLength} bytes`);
          break;
        case "Progress":
          downloaded += event.data.chunkLength;
          console.debug(`downloaded ${downloaded} from ${contentLength}`);
          break;
        case "Finished":
          console.debug("download finished");
          break;
      }
    });

    console.debug("update installed");
    await relaunch();
  }
}
