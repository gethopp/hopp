import path, { resolve } from "path";
import { defineConfig } from "vite";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { sentryVitePlugin } from "@sentry/vite-plugin";

// https://vitejs.dev/config/
export default defineConfig(async (config) => {
  return {
    plugins: [
      react(),
      tailwindcss(),
      // Enable only if Sentry is enabled
      process.env.SENTRY_AUTH_TOKEN ?
        sentryVitePlugin({
          org: "renkey",
          project: "tauri-app",
          authToken: process.env.SENTRY_AUTH_TOKEN,
        })
      : undefined,
    ],
    resolve: {
      alias: {
        "@": path.resolve(__dirname, "./src"),
      },
    },
    // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
    //
    // 1. prevent vite from obscuring rust errors
    clearScreen: false,
    // 2. tauri expects a fixed port, fail if that port is not available
    server: {
      port: process.env.VITE_PORT ? parseInt(process.env.VITE_PORT) : 1420,
      strictPort: true,
      watch: {
        // 3. tell vite to ignore watching `src-tauri`
        ignored: ["**/src-tauri/**", "**/vite.config.ts"],
      },
    },
    build: {
      sourcemap: true,
      rollupOptions: {
        input: {
          main: resolve(__dirname, "index.html"),
          screenshare: resolve(__dirname, "screenshare.html"),
          contentPicker: resolve(__dirname, "contentPicker.html"),
          permissions: resolve(__dirname, "permissions.html"),
          trayNotification: resolve(__dirname, "trayNotification.html"),
          camera: resolve(__dirname, "camera.html"),
          feedback: resolve(__dirname, "feedback.html"),
        },
      },
    },
  };
});
