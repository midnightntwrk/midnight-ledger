// JSON-RPC harness driven by `NodeChildBridge` in Rust integration
// tests. Reads newline-delimited JSON requests from stdin, writes
// `{ id, result } | { id, error }` responses to stdout. Diagnostics
// go to stderr so they don't pollute the RPC channel.
//
// Method registry grows as we wire up the ContractCall pipeline.
// Today it carries:
// - `ping`, `echo` — transport sanity (no contract layer).
// - `contractLayerInfo` — loads `@midnight-ntwrk/midnight-did-contract`
//   + `@midnight-ntwrk/compact-runtime` and reports what's available.
//   Step 3 layers `inspectCircuit` / `prepareCircuit` for real
//   circuit execution.
//
// **Requires** `npm install --ignore-scripts` in this directory
// once before first use (resolves the vendored packages under
// `dioxus-wallet/assets/web/pkg/`). The `--ignore-scripts` is load-
// bearing: upstream `compact-runtime` has a `clean` script that
// would wipe its own `dist/` if npm ran lifecycle scripts.
//
// Run manually:
//   node mobile-bench/wallet-core/tests/js-harness/harness.mjs
//   {"id":1,"method":"ping"}
//   ← {"id":1,"result":{"ok":true,"version":"0.1.0"}}

import * as readline from "node:readline";

const HARNESS_VERSION = "0.1.0";

// Lazy-load the WASM-touching contract layer. First call pays the
// runtime import cost; subsequent calls hit the cached promise.
let contractLayerPromise = null;
function loadContractLayer() {
  if (!contractLayerPromise) {
    contractLayerPromise = (async () => {
      const [contract, compactRuntime] = await Promise.all([
        import("@midnight-ntwrk/midnight-did-contract"),
        import("@midnight-ntwrk/compact-runtime"),
      ]);
      return { contract, compactRuntime };
    })();
  }
  return contractLayerPromise;
}

/** Dispatch table — method name → async handler(params). */
const methods = {
  ping: async () => ({ ok: true, version: HARNESS_VERSION }),
  echo: async (params) => ({ echoed: params?.message ?? null }),

  /**
   * Smoke test for the contract layer. Returns a summary of which
   * DID circuits + Compact runtime helpers are exposed by the
   * vendored package. The Rust side asserts on circuit names so a
   * future upstream rename is caught in CI.
   */
  contractLayerInfo: async () => {
    const { contract, compactRuntime } = await loadContractLayer();
    const did = contract.DIDContract;
    // The Contract class needs witnesses to instantiate; we can
    // still introspect the prototype to list circuit names.
    const circuitNames = did.Contract
      ? Object.keys(new did.Contract({
          localSecretKey: () => [null, new Uint8Array(32)],
          currentTimestamp: () => [null, 0n],
          getSchnorrReduction: () => [null, [0n, 0n]],
        }).impureCircuits || {})
      : [];
    return {
      contractExports: Object.keys(contract).slice(0, 16),
      compactRuntimeExports: Object.keys(compactRuntime).slice(0, 16),
      circuitNames,
      hasProofDataIntoSerializedPreimage:
        typeof compactRuntime.proofDataIntoSerializedPreimage === "function",
      hasCreateCircuitContext:
        typeof compactRuntime.createCircuitContext === "function",
    };
  },
};

function reply(id, payload) {
  process.stdout.write(JSON.stringify({ id, ...payload }) + "\n");
}

const rl = readline.createInterface({ input: process.stdin });

rl.on("line", async (line) => {
  const trimmed = line.trim();
  if (trimmed === "") return;
  let req;
  try {
    req = JSON.parse(trimmed);
  } catch (e) {
    process.stderr.write(`[harness] bad JSON: ${e.message}\n`);
    return;
  }
  const { id, method, params } = req;
  const handler = methods[method];
  if (typeof handler !== "function") {
    reply(id, { error: `unknown method: ${method}` });
    return;
  }
  try {
    const result = await handler(params);
    reply(id, { result });
  } catch (e) {
    reply(id, { error: e?.stack || String(e?.message || e) });
  }
});

rl.on("close", () => {
  process.stderr.write("[harness] stdin closed; exiting\n");
  process.exit(0);
});

process.stderr.write(`[harness] ready (v${HARNESS_VERSION})\n`);
