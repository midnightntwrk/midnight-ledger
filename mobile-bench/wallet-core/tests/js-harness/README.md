# wallet-core JS test harness

Node-driven JSON-RPC harness used by `wallet-core` integration tests
to exercise the Compact runtime + `@midnight-ntwrk/midnight-did-contract`
from `cargo test`. Required because the production transport
(Dioxus desktop's embedded WebView) can't be driven from `cargo test`:
tao panics off the main thread on macOS, and wry/webkitgtk needs a
display on Linux. See the design notes in `src/js_bridge.rs`.

## One-time setup

```
cd mobile-bench/wallet-core/tests/js-harness
npm install --ignore-scripts
```

`--ignore-scripts` is load-bearing — without it, npm runs upstream
`compact-runtime`'s `clean` script which wipes the vendored `dist/`
directory under `dioxus-wallet/assets/web/pkg/`.

If you ever lose the dist trees (this happens on accidental
`npm install` without the flag), re-vendor them:

```
cd mobile-bench/dioxus-wallet/web
node vendor.mjs
```

## What's in here

- `harness.mjs` — JSON-RPC dispatcher driven by stdin/stdout. Loads
  the contract layer lazily on first method that needs it.
- `package.json` — `file:` deps point at the vendored packages
  under `dioxus-wallet/assets/web/pkg/`. Same packages the wallet
  ships in production.
- `node_modules/`, `package-lock.json` — gitignored, populated by
  `npm install`.

## Calling it directly

```
$ node --preserve-symlinks harness.mjs
[harness] ready (v0.1.0)
{"id":1,"method":"contractLayerInfo"}
{"id":1,"result":{"contractExports":["DIDContract", ...], ...}}
```

`--preserve-symlinks` is required so Node module resolution stays
inside the harness's own `node_modules` instead of following each
file: symlink back into the WebView assets tree.
