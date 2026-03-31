import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "path";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [react()],

  // Multi-page: capsule + settings windows
  build: {
    rollupOptions: {
      input: {
        capsule: resolve(__dirname, "capsule.html"),
        settings: resolve(__dirname, "settings.html"),
      },
    },
  },

  // Vite server config for Tauri development
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 5174,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
