import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { useEffect } from "react";
import hotkeys from "hotkeys-js";

const appWindow = getCurrentWebviewWindow();

export const useResizeListener = (callback: () => void) => {
  useEffect(() => {
    // Run only once hook
    // Hacky way to initialise the callbacks with a Promise inside a hook
    const setupResizeListener = async () => {
      const unlisten = await appWindow.onResized(callback);
      return unlisten;
    };

    let unlisten: (() => void) | undefined;

    setupResizeListener().then((fn) => {
      unlisten = fn;
    });

    return () => {
      if (unlisten) unlisten();
    };
  }, [callback]);
};

/**
 * This is a hack to prevent the context menu from being shown
 * when the user right clicks on the screen.
 * @see: https://github.com/tauri-apps/tauri/discussions/3844#discussioncomment-8578187
 */
export const useDisableNativeContextMenu = () => {
  useEffect(() => {
    let isDevToolsEnabled = false;

    // Register the hotkey
    // For macOS, 'command+shift+d'
    // For Windows/Linux, 'ctrl+shift+d'
    hotkeys("cmd+shift+d, ctrl+shift+d", (event) => {
      event.preventDefault();
      isDevToolsEnabled = !isDevToolsEnabled;
    });

    document.addEventListener("contextmenu", (event) => {
      if (import.meta.env.MODE === "development") return;
      if (isDevToolsEnabled) return;
      event.preventDefault();
    });
  }, []);
};
