import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  base: "/",
  plugins: [solid()],
  server: {
    port: 5180,
    proxy: { "/api": process.env.MONOLITH_URL ?? "http://localhost:8090" },
  },
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
});

