import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
    hmr: true,
    watch: {
      // IDE edits on Windows-mounted WSL drives may not emit native file events.
      usePolling: true,
      interval: 200,
    },
    proxy: {
      "/api": {
        target: `http://${process.env.MCAP_HOST || "127.0.0.1"}:${process.env.MCAP_BACKEND_PORT || "8000"}`,
        changeOrigin: true,
      },
    },
  },
});
