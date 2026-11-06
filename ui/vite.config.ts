import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

// Served by the server at /_/ (doc 15 §4). Dev: API calls proxy to a local server.
export default defineConfig({
  base: "/_/",
  plugins: [solid()],
  server: {
    port: 5173,
    proxy: { "/api": process.env.MONOLITH_URL ?? "http://localhost:8090" },
  },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
});
