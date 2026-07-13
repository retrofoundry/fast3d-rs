import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import tailwindcss from "@tailwindcss/vite";
import wasm from "vite-plugin-wasm";
import topLevelAwait from "vite-plugin-top-level-await";
import { fileURLToPath, URL } from "node:url";

export default defineConfig({
  plugins: [tailwindcss(), svelte(), wasm(), topLevelAwait()],
  resolve: { alias: { "@toys": fileURLToPath(new URL("../examples/toys", import.meta.url)) } },
  build: { target: "esnext" },
});
