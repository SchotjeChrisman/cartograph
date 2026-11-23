// Bundles src/ into dist/, which the map crate embeds. Run with `npm run build`.
import { cpSync, mkdirSync, rmSync } from "node:fs"
import { build } from "esbuild"

rmSync("dist", { recursive: true, force: true })
mkdirSync("dist")
await build({
  entryPoints: { main: "src/main.jsx" },
  bundle: true,
  format: "esm",
  minify: true,
  jsx: "automatic",
  outdir: "dist",
  loader: { ".css": "css" },
  define: { "process.env.NODE_ENV": '"production"' },
})
cpSync("index.html", "dist/index.html")
