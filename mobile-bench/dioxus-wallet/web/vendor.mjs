// Copy upstream WASM-touching packages into ../assets/web/pkg/<name>/
// at build time. Those files are then served by Wry's `mn-pkg://`
// custom protocol (see `dioxus-wallet/src/protocol.rs`) and resolved
// by the WebView's native module + WebAssembly machinery via the
// import map injected from `lib.rs`.
//
// Why vendor instead of bundle: the upstream packages do
//   import * as wasm from "./xxx_bg.wasm";
// which esbuild's `file` loader handles by emitting a URL string,
// breaking wbindgen's module-namespace contract. Native browser
// resolution gets the import right.
//
// **Keep this list in sync** with the `external` list in `build.mjs`
// and the `imports` map in `dioxus-wallet/src/lib.rs::head_html`.

import { build as esbuildBuild } from "esbuild";
import {
  copyFileSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));

// Built checkout of the midnight-did repo (submodule or standalone clone)
// with `pnpm install && pnpm build` already run.
// Must be set — there is no default.
const SOURCE = resolve(
  (() => {
    const src = process.env.MIDNIGHT_DID_SRC;
    if (!src) {
      console.error(
        "[vendor] MIDNIGHT_DID_SRC is not set. Export it to point at the" +
          " midnight-did repo checkout (with pnpm install && pnpm build" +
          " already run), then re-run this script.",
      );
      process.exit(1);
    }
    return src;
  })(),
);
// Where Wry's `mn-pkg://` handler reads from.
const DEST = resolve(__dirname, "..", "assets", "web", "pkg");

// `node_modules/@midnight-ntwrk/<name>` is the resolved tree we copy.
// We pick this over the workspace `<source>/<dir>/` so transitive
// resolution is consistent — every dep already points at the
// resolved version, not a peer that might be a symlink.
// Pure-JS packages with their transitive Effect / wallet-sdk closure
// (`midnight-js-contracts`, `midnight-js-network-id`) are *not*
// vendored — esbuild bundles them into `midnight-did.js` so the
// import-map needs only the WASM-bearing modules below. The
// vendored DID contract tree carries the `.prover` / `.verifier`
// blobs that the WebView ZK config provider fetches over `mn-pkg://`.
//
// 2026-05-28 schema refresh: the redesigned DID contract's
// `witnesses.js` adds a runtime `import { TWO_248 } from
// "@midnight-ntwrk/midnight-did-jubjub-schnorr"`, so we now have to
// vendor that package too — the import is loaded over `mn-pkg://`
// at WebView runtime, not statically bundled by esbuild.
const PACKAGES = [
  "midnight-did-contract",
  "midnight-did-jubjub-schnorr",
  "compact-runtime",
  "compact-js",
  "onchain-runtime-v3",
  "ledger-v8",
];

function copyDirRecursive(src, dst) {
  mkdirSync(dst, { recursive: true });
  for (const entry of readdirSync(src)) {
    // Skip dev artifacts and nested node_modules; we resolve those
    // via the import map at runtime, not via package-internal trees.
    if (
      entry === "node_modules" ||
      entry === "coverage" ||
      entry === "reports" ||
      entry === ".turbo" ||
      entry.endsWith(".tgz")
    ) {
      continue;
    }
    const s = join(src, entry);
    const d = join(dst, entry);
    if (statSync(s).isDirectory()) {
      copyDirRecursive(s, d);
    } else {
      copyFileSync(s, d);
    }
  }
}

console.log(`[vendor] source: ${SOURCE}`);
console.log(`[vendor] dest:   ${DEST}`);

rmSync(DEST, { recursive: true, force: true });
mkdirSync(DEST, { recursive: true });

let total = 0;
for (const pkg of PACKAGES) {
  const src = resolve(SOURCE, "node_modules", "@midnight-ntwrk", pkg);
  const dst = resolve(DEST, pkg);
  try {
    statSync(src);
  } catch (_) {
    console.error(`[vendor] missing source: ${src}`);
    process.exit(1);
  }
  copyDirRecursive(src, dst);
  total++;
  console.log(`[vendor]   ✓ ${pkg}`);
}

// Post-copy patch: `midnight-did-jubjub-schnorr/dist/signing.js`
// does a top-level `import { createHash, randomBytes } from "node:crypto"`
// which the WebView's native ESM loader can't resolve → silently
// aborts the entire module graph that transitively imports it
// (witnesses.js → midnight-did-contract → entry.ts), so
// `window.midnightDidBundle` never gets set.
//
// The wallet's actual flow only consumes `TWO_248` (a BigInt constant)
// from this file — none of the Node-crypto-dependent helpers
// (`hashToScalar`, `randomScalar`, `signJubjub`) are called from
// either the WebView or Rust paths. Drop the import; functions
// that referenced the removed names will throw `ReferenceError` if
// ever called, which is the correct failure mode.
const SIGNING_JS = resolve(
  DEST, "midnight-did-jubjub-schnorr", "dist", "signing.js",
);
try {
  const before = readFileSync(SIGNING_JS, "utf-8");
  const after = before.replace(
    /^import \{ createHash, randomBytes \} from "node:crypto";$/m,
    '// [vendor.mjs patch] removed `node:crypto` import — WebView has no Node modules. Helpers using these will throw if called.',
  );
  if (after === before) {
    console.warn(
      `[vendor]   ! no node:crypto line found in ${SIGNING_JS} ` +
      "— signing.js may have changed shape upstream; bundle init may fail.",
    );
  } else {
    writeFileSync(SIGNING_JS, after);
    console.log("[vendor]   ↻ patched signing.js (dropped node:crypto import)");
  }
} catch (e) {
  console.warn(`[vendor]   ! could not patch signing.js: ${e.message}`);
}

// Some upstream packages (compact-runtime → object-inspect) reach
// for third-party CJS modules the WebView's native ESM loader can't
// load directly. We esbuild-bundle each into an ESM wrapper, place
// it under pkg/<name>/index.js, and route the import map at it.
//
// Keep the list in sync with the import map in
// `dioxus-wallet/src/lib.rs::head_html`.
const CJS_DEPS = [
  "object-inspect",
];

for (const dep of CJS_DEPS) {
  const src = resolve(SOURCE, "node_modules", dep);
  const dst = resolve(DEST, dep);
  try {
    statSync(src);
  } catch (_) {
    console.error(`[vendor] missing CJS dep: ${src}`);
    process.exit(1);
  }
  mkdirSync(dst, { recursive: true });
  await esbuildBuild({
    entryPoints: [resolve(src, "index.js")],
    bundle: true,
    format: "esm",
    platform: "browser",
    outfile: resolve(dst, "index.js"),
    logLevel: "warning",
  });
  total++;
  console.log(`[vendor]   ✓ ${dep} (CJS→ESM)`);
}

// Replace the wbindgen entry file with a browser-compatible loader.
// The default `xxx.js` file does `import * as wasm from "./xxx.wasm"`,
// which requires WebAssembly ES Module Integration (a stage-4 spec
// not yet enabled in WKWebView). We swap it for a manual loader that
// fetches the .wasm via `import.meta.url`, instantiates with the
// `_bg.js`-supplied imports table, then calls `__wbg_set_wasm` +
// `__wbindgen_start` exactly like the upstream Node loader does
// (`xxx_fs.js`).
function rewriteWbindgenEntry(packageDir, entryStem) {
  const entryPath = resolve(packageDir, `${entryStem}.js`);
  const bgFile = `./${entryStem}_bg.js`;
  const wasmFile = `./${entryStem}_bg.wasm`;
  // wasm-bindgen splits inline JS snippets into
  // `./snippets/<pkg>-<hash>/inlineN.js`. The wasm module imports
  // each one by relative path, so the import table we build needs
  // a key per snippet. Mirror the `xxx_fs.js` shape exactly —
  // enumerate the snippets directory and emit an import + table
  // entry for every file. Without these, `WebAssembly.instantiate`
  // errors with `Import #0 module="…/inline0.js": module is not
  // an object or function`.
  const snippetsRoot = resolve(packageDir, "snippets");
  const snippetEntries = [];
  try {
    for (const pkgDir of readdirSync(snippetsRoot)) {
      const inner = resolve(snippetsRoot, pkgDir);
      if (!statSync(inner).isDirectory()) continue;
      for (const file of readdirSync(inner)) {
        if (!file.endsWith(".js")) continue;
        const rel = `./snippets/${pkgDir}/${file}`;
        const id = `snippet_${pkgDir.replace(/[^a-zA-Z0-9_]/g, "_")}_${file.replace(/[^a-zA-Z0-9_]/g, "_")}`;
        snippetEntries.push({ id, rel });
        // wasm-bindgen snippets contain `import * as wasm from '#self'`
        // which relies on Node's package-internal `imports` map to
        // resolve back to `midnight_ledger_wasm.js`. The browser
        // doesn't honour `#`-prefixed specifiers, so we rewrite them
        // to a relative path to the wbindgen entry file. Snippets
        // live two levels deep (`./snippets/<pkg>/inlineN.js`), so
        // `../../<entry>.js` reaches the package root.
        const path = resolve(inner, file);
        const src = readFileSync(path, "utf8");
        const rewritten = src.replace(
          /(['"])#self\1/g,
          `'../../${entryStem}.js'`,
        );
        if (rewritten !== src) {
          writeFileSync(path, rewritten);
        }
      }
    }
  } catch (_) {
    // No snippets/ dir — older or no-snippet packages. Fine.
  }
  const snippetImports = snippetEntries
    .map((s) => `import * as ${s.id} from "${s.rel}";`)
    .join("\n");
  const snippetTable = snippetEntries
    .map((s) => `imports["${s.rel}"] = ${s.id};`)
    .join("\n");

  const browserLoader = `// Auto-generated by web/vendor.mjs at build time.
// Replaces upstream's \`import * as wasm from "${wasmFile}"\` (which
// needs WebAssembly ESM Integration) with a fetch + manual
// instantiation. Mirrors xxx_fs.js for Node, but fetches over the
// \`mn-pkg://\` protocol via \`import.meta.url\` resolution. Snippet
// imports (\`./snippets/<pkg>-<hash>/inlineN.js\`) are enumerated at
// vendor time and wired into the import table the wasm module
// expects.

import * as bgExports from "${bgFile}";
export * from "${bgFile}";
import { __wbg_set_wasm } from "${bgFile}";
${snippetImports}

const imports = {};
imports["${bgFile}"] = bgExports;
${snippetTable}

const wasmUrl = new URL("${wasmFile}", import.meta.url);
const response = await fetch(wasmUrl);
let instance;
if (typeof WebAssembly.instantiateStreaming === "function") {
  try {
    ({ instance } = await WebAssembly.instantiateStreaming(response, imports));
  } catch (_e) {
    const bytes = await (await fetch(wasmUrl)).arrayBuffer();
    ({ instance } = await WebAssembly.instantiate(bytes, imports));
  }
} else {
  const bytes = await response.arrayBuffer();
  ({ instance } = await WebAssembly.instantiate(bytes, imports));
}
const wasm = instance.exports;

__wbg_set_wasm(wasm);
wasm.__wbindgen_start();
`;
  writeFileSync(entryPath, browserLoader);
  console.log(`[vendor]   ↻ ${entryPath.split("/").slice(-2).join("/")} (wbindgen rewrite, ${snippetEntries.length} snippets)`);
}

const WBINDGEN_ENTRIES = [
  ["onchain-runtime-v3", "midnight_onchain_runtime_wasm"],
  ["ledger-v8", "midnight_ledger_wasm"],
];
for (const [pkg, stem] of WBINDGEN_ENTRIES) {
  rewriteWbindgenEntry(resolve(DEST, pkg), stem);
}

console.log(`[vendor] copied ${total} package(s).`);
