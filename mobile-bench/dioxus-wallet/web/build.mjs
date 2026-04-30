// Bundle midnight-did + non-WASM dependencies for the dioxus-wallet
// WebView.
//
// **Architecture.** The WASM-touching packages are *not* bundled —
// they're vendored to `../assets/web/pkg/` by `vendor.mjs` and served
// at runtime via the `mn-pkg://` custom protocol with native module
// resolution. esbuild marks them as `external` so the bundle keeps
// the package specifiers (`@midnight-ntwrk/...`) intact; the import
// map injected from `dioxus-wallet/src/lib.rs::head_html` rewrites
// those specifiers to `mn-pkg://...` URLs at module load time.
//
// **Output format**: ESM. The bundle is injected via
// `<script type="module">` in `<head>` so static + dynamic
// `import()` resolve through the import map.

import { build, context } from "esbuild";
import { nodeModulesPolyfillPlugin } from "esbuild-plugins-node-modules-polyfill";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
import { argv } from "node:process";

const __dirname = dirname(fileURLToPath(import.meta.url));
const outdir = resolve(__dirname, "..", "assets", "web");

// Keep in lockstep with `vendor.mjs::PACKAGES` and the import map in
// `dioxus-wallet/src/lib.rs::head_html`.
const VENDORED_EXTERNALS = [
  "@midnight-ntwrk/midnight-did-contract",
  "@midnight-ntwrk/compact-runtime",
  "@midnight-ntwrk/compact-js",
  "@midnight-ntwrk/onchain-runtime-v3",
  "@midnight-ntwrk/ledger-v8",
];

const config = {
  entryPoints: [resolve(__dirname, "src", "entry.ts")],
  bundle: true,
  // ESM so the bundle can `import { ... } from "@midnight-ntwrk/foo"`
  // and have it resolve through the import map at runtime.
  format: "esm",
  platform: "browser",
  conditions: ["browser", "module", "import", "default"],
  mainFields: ["browser", "module", "main"],
  alias: {
    // Named-export-compatible WebSocket shim.
    ws: resolve(__dirname, "src", "ws-shim.js"),
    // Genuinely unsupported in the WebView (Node-only crypto / fs).
    // We provide our own equivalents Rust-side.
    "@midnight-ntwrk/midnight-did-secret-storage": resolve(__dirname, "src", "unsupported-stub.js"),
    "@midnight-ntwrk/midnight-js-level-private-state-provider": resolve(__dirname, "src", "unsupported-stub.js"),
    "@midnight-ntwrk/wallet-sdk-hd": resolve(__dirname, "src", "unsupported-stub.js"),
  },
  external: [...VENDORED_EXTERNALS, "pino-pretty"],
  plugins: [
    nodeModulesPolyfillPlugin({
      modules: {
        path: true,
        crypto: true,
        assert: true,
        util: true,
        events: true,
        stream: true,
        buffer: true,
        fs: "empty",
        "fs/promises": "empty",
        os: "empty",
      },
    }),
  ],
  outfile: resolve(outdir, "midnight-did.js"),
  loader: { ".wasm": "file" }, // not really used now (externals handle WASM)
  sourcemap: true,
  logLevel: "info",
  define: {
    "process.env.NODE_ENV": '"production"',
    global: "globalThis",
  },
  inject: [resolve(__dirname, "src", "buffer-shim.js")],
};

if (argv.includes("--watch")) {
  const ctx = await context(config);
  await ctx.watch();
  console.log("[esbuild] watching…");
} else {
  await build(config);
  console.log("[esbuild] bundle written to", config.outfile);
}
