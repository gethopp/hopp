import { OS } from "@/constants";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow, PhysicalSize, LogicalSize } from "@tauri-apps/api/window";
import { currentMonitor } from "@tauri-apps/api/window";

const appWindow = getCurrentWebviewWindow();

/*
 * This function resizes the window to fit the stream's aspect ratio.
 *
 * It assumes that the stream's aspect ratio is greater than 1 (width > height).
 * There are two possible scenarios that need to be handled in order to avoid
 * the window to overflowing the screen (we could only overflow the heigth because
 * we this is calculated from the width using the aspect ratio):
 *   - The monitor's width is greater than its height, and then
 *     we need to make sure the stream doesn't overflow the height.
 *
 *     In this case we calculate the max width from the monitor's height
 *     and don't allow the window to have a width greater than the
 *     calculated one.
 *
 *   - The monitor's height is greater than its width, and then
 *     we need to make sure the stream doesn't overflow the width.
 *
 *     In this case we calculate the max height from the monitor's width
 *     and don't allow the window to have a height greater than the
 *     calculated one.
 */
export async function resizeWindow(streamWidth: number, streamHeight: number, ref: React.RefObject<HTMLVideoElement | HTMLCanvasElement>) {
  if (streamWidth === 16 && streamHeight === 9) {
    return;
  }
  const monitor = await currentMonitor();
  const monitorWidth = monitor?.size.width || 0;
  const monitorHeight = monitor?.size.height || 0;
  const aspectRatio = streamWidth / streamHeight;
  const factor = await appWindow.scaleFactor();

  // TODO: We can get the menubar heigth from tauri and the core process
  // for now we will use reasonable defaults
  // Windows 40px for taskbar and 30px for title bar = 70px
  // macos 22px for menubar

  /*
   * When we have to limit the width based on the height, we need to
   * substruct from the monitor height the taskbar/menubar height.
   *
   * We also substruct the extra offset needed for the stream to be
   * shown whole. This is needed because we want the max width to be
   * calculated without this extra offset and subsequently have
   * a smaller max width than the theoretical one. This is needed
   * because the streamExtraOffset will be added regardless of
   * the width.
   *
   * If we don't do this, the window will overflow the screen.
   */
  const streamExtraOffset = 50 * factor;
  const taskbarHeight =
    (OS === "windows" ? 70
    : OS === "macos" ? 25
    : 0) * factor;

  let maxHeight = Math.floor(monitorHeight - taskbarHeight - streamExtraOffset);
  let maxWidth = Math.floor(monitorWidth);

  if (maxWidth > 0 && maxHeight > 0) {
    if (monitorWidth >= monitorHeight) {
      maxWidth = Math.floor(maxHeight * aspectRatio);
    } else {
      maxHeight = Math.floor(maxWidth / aspectRatio);
    }
    appWindow.setMaxSize(new LogicalSize(maxWidth, maxHeight + streamExtraOffset));
  }

  let size = await appWindow.innerSize();

  if (ref.current) {
    if (!aspectRatio || isNaN(aspectRatio)) {
      return;
    }

    const newWidth = Math.min(size.width, maxWidth);
    const newHeight = Math.min(Math.floor(size.width / aspectRatio), maxHeight) + streamExtraOffset;
    console.log(`Current size is ${size.width}x${size.height}; New size will be ${newWidth}x${newHeight}`);
    appWindow.setSize(
      // As the video will be always full window width minus some padding,
      // the only thing we modify is the window height
      new PhysicalSize(Math.floor(newWidth), Math.floor(newHeight)),
    );
  }
}
