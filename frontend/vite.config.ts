import path from "path"
import tailwindcss from "@tailwindcss/vite"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": path.resolve(import.meta.dirname, "./src"),
    },
  },
  server: {
    // `npm run dev` talks to a backend started separately; the SSE endpoints
    // need the proxy too, hence proxying the whole /api prefix.
    proxy: {
      "/api": {
        target: process.env.SHIMAU_DEV_BACKEND ?? "http://127.0.0.1:8080",
        changeOrigin: false,
      },
    },
  },
  build: {
    // Served by the Rust binary from SHIMAU_STATIC_DIR.
    outDir: "dist",
    sourcemap: false,
  },
})
