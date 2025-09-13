import path from "path";
import { defineConfig } from "vite";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { viteSingleFile } from "vite-plugin-singlefile";

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss(), viteSingleFile()],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  base: "/static/react/",
  build: {
    outDir: process.cwd() + "/static/react/assets",
    assetsDir: "",
    rollupOptions: {
      output: {
        manualChunks: undefined,
      },
    },
  },
});
