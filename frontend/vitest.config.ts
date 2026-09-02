import path from "path"
import react from "@vitejs/plugin-react"
import { defineConfig } from "vitest/config"

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: { "@": path.resolve(import.meta.dirname, "./src") },
  },
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./src/test/setup.ts"],
    coverage: {
      provider: "v8",
      reporter: ["text", "lcov"],
      // The generated shadcn primitives are upstream code, and main.tsx is a
      // three-line bootstrap: neither is ours to test.
      exclude: [
        "src/components/ui/**",
        "src/main.tsx",
        "src/test/**",
        "**/*.config.*",
        "dist/**",
      ],
    },
  },
})
