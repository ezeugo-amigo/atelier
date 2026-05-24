import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  // Tauri serves production assets from its app protocol, so generated asset
  // URLs must be relative. Vite's default absolute "/assets/..." paths render
  // a blank window in the bundled app.
  base: "./",
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
  },
  envPrefix: ["VITE_", "TAURI_"],
});
