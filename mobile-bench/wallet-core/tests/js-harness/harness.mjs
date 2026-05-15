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

/** Strict hex → Uint8Array. Throws on odd length or invalid chars. */
function hexToBytes(hex) {
  const clean = typeof hex === "string" && hex.startsWith("0x") ? hex.slice(2) : hex;
  if (typeof clean !== "string") {
    throw new Error(`hex must be a string, got ${typeof clean}`);
  }
  if (clean.length % 2 !== 0) {
    throw new Error(`hex length must be even, got ${clean.length}`);
  }
  const out = new Uint8Array(clean.length / 2);
  for (let i = 0; i < out.length; i++) {
    const byte = parseInt(clean.slice(i * 2, i * 2 + 2), 16);
    if (Number.isNaN(byte)) throw new Error(`bad hex at offset ${i * 2}`);
    out[i] = byte;
  }
  return out;
}

function bytesToHex(b) {
  let s = "";
  for (let i = 0; i < b.length; i++) {
    s += b[i].toString(16).padStart(2, "0");
  }
  return s;
}

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
   * Run a single DID circuit against a caller-supplied
   * `ContractState`. Used by Rust integration tests to verify the
   * Compact-runtime pipeline without involving the chain.
   *
   * Inputs:
   * - `circuit`: entry-point name (e.g. "deactivate").
   * - `contractStateHex`: tagged-serialized `ContractState<DefaultDB>`,
   *   from `wallet_core::testing_initial_deploy_state_hex` or
   *   pulled from the indexer.
   * - `contractAddressHex`: 32-byte address (DID's contract address).
   * - `controllerSecretHex`: 64-char hex; bound to `localSecretKey`
   *   witness. Must match the `controllerPublicKey` baked into
   *   `contractStateHex`, otherwise the circuit's controller
   *   assertion will fail.
   * - `circuitArgs`: array of args for circuits that take any
   *   (empty for deactivate). Step 4 wires inputs for the other 10.
   *
   * Output (Compact runtime → ProofData → SCALE preimage):
   * - `circuit`: echoed circuit name.
   * - `publicTranscriptLen`, `privateTranscriptLen`: integer counts
   *   so Rust can sanity-check before deserialising the preimage.
   * - `preimageHex`: SCALE bytes the Rust side decodes into
   *   `transient_crypto::proofs::ProofPreimage`.
   * - `elapsedMs`: wall-clock duration in ms.
   */
  inspectCircuit: async (params) => {
    const t0 = Date.now();
    const { contract: c, compactRuntime: cr } = await loadContractLayer();
    const skBytes = hexToBytes(params.controllerSecretHex);
    if (skBytes.length !== 32) {
      throw new Error(`controllerSecretHex must be 32 bytes, got ${skBytes.length}`);
    }
    const witnesses = {
      localSecretKey: (ctx) => [ctx.privateState, skBytes],
      currentTimestamp: (ctx) => [ctx.privateState, BigInt(Date.now())],
      // `verifyJubjub*` are the only circuits that touch this — we
      // don't drive those from the inspector, but Witnesses requires
      // every member declared in the contract spec.
      getSchnorrReduction: (ctx) => [ctx.privateState, [0n, 0n]],
    };
    const contractInstance = new c.DIDContract.Contract(witnesses);

    const stateBytes = hexToBytes(params.contractStateHex);
    const contractState = cr.ContractState.deserialize(stateBytes);

    // `ContractAddress` and `CoinPublicKey` are *string* aliases at
    // this runtime version — they get passed through wasm-bindgen's
    // `passStringToWasm0` which expects a string with `.charCodeAt`.
    // We accept hex input from Rust and pass it through unchanged.
    if (typeof params.contractAddressHex !== "string"
      || params.contractAddressHex.length !== 64) {
      throw new Error(
        `contractAddressHex must be a 64-char hex string, got ${typeof params.contractAddressHex}/${params.contractAddressHex?.length}`,
      );
    }
    const dummyCoinPk = "00".repeat(32);
    const circuitContext = cr.createCircuitContext(
      params.contractAddressHex,
      dummyCoinPk,
      contractState,
      null, // privateState: this contract has no private state field
    );

    const fn = contractInstance.impureCircuits[params.circuit];
    if (typeof fn !== "function") {
      throw new Error(`unknown circuit: ${params.circuit}`);
    }
    const args = Array.isArray(params.circuitArgs) ? params.circuitArgs : [];
    const results = fn(circuitContext, ...args);
    const proofData = results.proofData;

    const preimage = cr.proofDataIntoSerializedPreimage(
      proofData.input,
      proofData.output,
      proofData.publicTranscript,
      proofData.privateTranscriptOutputs,
      `midnight/did/${params.circuit}`,
    );

    return {
      circuit: params.circuit,
      publicTranscriptLen: proofData.publicTranscript.length,
      privateTranscriptLen: proofData.privateTranscriptOutputs.length,
      preimageHex: bytesToHex(preimage),
      elapsedMs: Date.now() - t0,
    };
  },

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
