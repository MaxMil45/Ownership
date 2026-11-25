/// <reference types="vitest/config" />
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// The dev server listens on localhost:5173 only. Start the API with
//   oe serve --allow-origin http://127.0.0.1:5173
export default defineConfig({
  plugins: [react()],
  server: { host: "127.0.0.1", port: 5173, strictPort: true },
  test: { environment: "node" },
});
