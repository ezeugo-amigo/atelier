import { defineConfig } from "vite";

export default defineConfig({
  // Tauri serves production assets from its app protocol, so asset URLs must be relative.
  base: "./",
  clearScreen: false,
  server: {
    port: 1430,
    strictPort: true,
    host: "127.0.0.1",
  },
});
