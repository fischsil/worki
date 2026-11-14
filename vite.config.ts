import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig(({ mode }) => ({
  root: "ui",
  // The demo server runs beside `tauri dev`; sharing one dependency cache breaks the other's.
  cacheDir: mode === "demo" ? "../node_modules/.vite-demo" : "../node_modules/.vite",
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    target: "safari16",
  },
}));
