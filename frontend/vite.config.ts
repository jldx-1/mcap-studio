import { fileURLToPath } from "node:url";
import { defineConfig, loadEnv } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig(({ mode }) => {
  // Load the root .env even when running the frontend alone on another host.
  const env = { ...loadEnv(mode, fileURLToPath(new URL("../", import.meta.url)), "MCAP_"), ...process.env };
  const host = env.MCAP_HOST || "127.0.0.1";
  const backendHost = host === "0.0.0.0" ? "127.0.0.1" : host === "[::]" || host === "::" ? "[::1]" : host;
  return {
    plugins: [react()],
    server: {
      host,
      port: Number(env.MCAP_FRONTEND_PORT || 5173),
      strictPort: true,
      hmr: true,
      watch: {
        // IDE edits on Windows-mounted WSL drives may not emit native file events.
        usePolling: true,
        interval: 200,
      },
      proxy: {
        "/api": {
          target: env.MCAP_API_PROXY_TARGET || `http://${backendHost}:${env.MCAP_BACKEND_PORT || "8000"}`,
          changeOrigin: true,
        },
      },
    },
  };
});
