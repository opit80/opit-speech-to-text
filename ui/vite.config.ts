import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [svelte()],
  // Keep Rust/Tauri CLI output visible.
  clearScreen: false,
  server: {
    // Must match build.devUrl in crates/app/tauri.conf.json.
    port: 5173,
    strictPort: true,
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // WebView2 (Chromium) on Windows.
    target: "chrome105",
  },
});
