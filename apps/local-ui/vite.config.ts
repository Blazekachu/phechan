import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

/** Proxy /content + /r to local ord for Studio Libraries (regtest). */
const ordProxy = {
  target: "http://127.0.0.1:8081",
  changeOrigin: true,
  configure: (proxy: { on: (ev: string, fn: (...args: unknown[]) => void) => void }) => {
    proxy.on("proxyReq", (...args: unknown[]) => {
      const proxyReq = args[0] as { setHeader: (k: string, v: string) => void };
      // local ord serves brotli-inscribed bodies
      proxyReq.setHeader("Accept-Encoding", "br, gzip, deflate, identity");
      proxyReq.setHeader("Accept", "*/*");
    });
  },
};

export default defineConfig({
  plugins: [react()],
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": {
        target: "http://127.0.0.1:8787",
        changeOrigin: false,
      },
      "/preview": {
        target: "http://127.0.0.1:8787",
        changeOrigin: false,
      },
      "/content": ordProxy,
      "/r": ordProxy,
    },
  },
  preview: {
    host: "127.0.0.1",
    port: 5173,
  },
});
