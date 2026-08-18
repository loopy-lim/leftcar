import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri dev-server convention: fixed port, clear screen off, TAURI_ env pass.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  envPrefix: ["TAURI_"],
  build: {
    target: "safari15",
  },
});
