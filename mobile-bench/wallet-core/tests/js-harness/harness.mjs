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
import * as path from "node:path";
import { fileURLToPath } from "node:url";

const HARNESS_VERSION = "0.1.0";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

/**
 * Path to the bundled DID circuit artifacts (prover keys, verifier
 * keys, IR). Same `<contract>/dist/managed/did/` layout the upstream
 * `NodeZkConfigProvider` expects.
 */
const DID_ZK_ASSETS_PATH = path.resolve(
  __dirname,
  "node_modules",
  "@midnight-ntwrk",
  "midnight-did-contract",
  "dist",
  "managed",
  "did",
);

/**
 * Path to the bundled passport-vault circuit artifacts (prover keys,
 * verifier keys, IR). Same `<contract>/dist/managed/passport-vault/`
 * layout the upstream `NodeZkConfigProvider` expects. Used by
 * `prepareVaultCallTx`.
 */
const PV_ZK_ASSETS_PATH = path.resolve(
  __dirname,
  "node_modules",
  "@midnight-ntwrk",
  "passport-vault-contract",
  "dist",
  "managed",
  "passport-vault",
);

/**
 * Walk a JSON value, replacing objects of shape `{ "$bigint": "123" }`
 * with the equivalent JS `BigInt`. JSON has no native bigint type;
 * the Compact runtime's `Field` / `Uint<N>` args need BigInt. The
 * tagged-object convention keeps the wire format pure JSON.
 */
function reviveBigints(value) {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map(reviveBigints);
  if (typeof value.$bigint === "string") return BigInt(value.$bigint);
  // `Bytes<N>` fields in the contract schema (e.g. PublicKeyJwk.x/y
  // after the 2026-05-27 `Field → Bytes<32>` refactor) get encoded
  // by the Rust caller as `{"$bytes": "0x<2*N hex chars>"}`. The
  // contract runtime expects a `Uint8Array` of exactly N bytes;
  // hexToBytes returns one of arbitrary length, so the caller is
  // responsible for sending the right hex width.
  if (typeof value.$bytes === "string") return hexToBytes(value.$bytes);
  const out = {};
  for (const [k, v] of Object.entries(value)) {
    out[k] = reviveBigints(v);
  }
  return out;
}

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

/**
 * Recursively convert a value to a JSON-safe form:
 * - BigInt → decimal string
 * - Uint8Array → hex string (0x-prefixed)
 * - plain objects/arrays → recurse
 * This mirrors the Rust side's deserialisation expectations
 * (bigint fields arrive as strings, byte fields as hex).
 */
function bigIntSafe(value) {
  if (value === null || value === undefined) return value;
  if (typeof value === "bigint") return value.toString(10);
  if (value instanceof Uint8Array) return "0x" + bytesToHex(value);
  if (Array.isArray(value)) return value.map(bigIntSafe);
  if (typeof value === "object") {
    const out = {};
    for (const [k, v] of Object.entries(value)) {
      out[k] = bigIntSafe(v);
    }
    return out;
  }
  return value;
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

    // Optional setup: run a chain of circuits first to evolve the
    // state (e.g. `addAlsoKnownAs` before testing `removeAlsoKnownAs`).
    // Each step's post-state `circuitContext` feeds the next. The
    // ProofData from these setup runs is discarded — only the final
    // (under-test) circuit's preimage gets returned.
    let ctx = circuitContext;
    const setup = Array.isArray(params.setup) ? params.setup : [];
    for (const step of setup) {
      const stepFn = contractInstance.impureCircuits[step.circuit];
      if (typeof stepFn !== "function") {
        throw new Error(`unknown setup circuit: ${step.circuit}`);
      }
      const stepArgs = Array.isArray(step.args) ? step.args.map(reviveBigints) : [];
      const stepResult = stepFn(ctx, ...stepArgs);
      ctx = stepResult.context;
    }

    const fn = contractInstance.impureCircuits[params.circuit];
    if (typeof fn !== "function") {
      throw new Error(`unknown circuit: ${params.circuit}`);
    }
    // Revive any `{ "$bigint": "123" }` placeholders to JS BigInt
    // before passing to the circuit (Compact expects BigInt for
    // `Field`, `Uint<N>`, etc.).
    const rawArgs = Array.isArray(params.circuitArgs) ? params.circuitArgs : [];
    const args = rawArgs.map(reviveBigints);
    const results = fn(ctx, ...args);
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
   * Produce a SCALE-serialised `UnprovenTransaction` that calls a
   * DID circuit on a deployed contract. Step 6 of the bridge plan
   * — the Rust side will deserialise this, balance dust, prove,
   * and submit via the existing pipeline.
   *
   * The upstream `createUnprovenCallTxFromInitialStates` does the
   * heavy lifting: runs the circuit, partitions the transcript,
   * commits the inputs, wraps in a `Transaction<PreProof,…>`.
   *
   * Inputs (all bytes encoded as hex strings):
   * - `did`, `circuit`, `circuitArgs` — what to call.
   * - `contractStateHex` — current `ContractState` from indexer.
   * - `zswapChainStateHex` — current `ZswapChainState` (use the
   *   ledger-state-derived form; DID contracts have an empty one
   *   in practice but the API still wants it).
   * - `ledgerParametersHex` — chain LedgerParameters.
   * - `controllerSecretHex` — 32-byte controller sk that fulfils
   *   `localSecretKey()` witness.
   * - `coinPublicKeyHex`, `encryptionPublicKeyHex` — the user's
   *   Zswap public keys (the resulting tx will be balanced/spent
   *   against this wallet).
   * - `networkId` — "undeployed" / "testnet" / etc.
   */
  prepareUnprovenCallTx: async (params) => {
    const t0 = Date.now();
    const { contract: c, compactRuntime: cr } = await loadContractLayer();
    const [jsContracts, ledgerV8, compactJs, networkIdMod, zkProvMod] = await Promise.all([
      import("@midnight-ntwrk/midnight-js-contracts"),
      import("@midnight-ntwrk/ledger-v8"),
      import("@midnight-ntwrk/compact-js"),
      import("@midnight-ntwrk/midnight-js-network-id"),
      import("@midnight-ntwrk/midnight-js-node-zk-config-provider"),
    ]);
    networkIdMod.setNetworkId(params.networkId ?? "undeployed");

    const skBytes = hexToBytes(params.controllerSecretHex);
    if (skBytes.length !== 32) {
      throw new Error(`controllerSecretHex must be 32 bytes, got ${skBytes.length}`);
    }
    // Witnesses for the DID contract — `localSecretKey` returns the
    // wallet's 32-byte controller sk; `currentTimestamp` returns
    // wall-clock ms; `getSchnorrReduction` is only used by
    // `verifyJubjub*` circuits which aren't drivable from this path.
    const witnesses = {
      localSecretKey: (ctx) => [ctx.privateState, skBytes],
      currentTimestamp: (ctx) => [ctx.privateState, BigInt(Date.now())],
      getSchnorrReduction: (ctx) => [ctx.privateState, [0n, 0n]],
    };

    const compiledContract = compactJs.CompiledContract.make(
      "did",
      c.DIDContract.Contract,
    ).pipe(
      compactJs.CompiledContract.withWitnesses(witnesses),
      compactJs.CompiledContract.withCompiledFileAssets(DID_ZK_ASSETS_PATH),
    );

    const zkConfigProvider = new zkProvMod.NodeZkConfigProvider(DID_ZK_ASSETS_PATH);

    // Materialise on-chain inputs. The upstream
    // `createUnprovenCallTxFromInitialStates` runs the contract
    // state through `coerceToChargedState` (compact-runtime), which
    // accepts `onchain-runtime`'s `ContractState` — NOT
    // `ledger-v8`'s same-named class. `cr.ContractState` (re-exported
    // from `compact-runtime`) is the right one.
    // Empty / initial fallbacks are legal for `zswapChainState`
    // (DID contracts touch no Zswap state) and `ledgerParameters`
    // (use chain initial params).
    const contractState = cr.ContractState.deserialize(
      hexToBytes(params.contractStateHex),
    );
    // Diagnostic: confirm we received the live state hex from Rust
    // (the Path A fix). If either is missing we silently fall back to
    // initial/empty values and the chain rejects the resulting proof
    // with `Invalid Transaction (1010)`.
    const zswapHexLen = params.zswapChainStateHex ? params.zswapChainStateHex.length : 0;
    const ledgerHexLen = params.ledgerParametersHex ? params.ledgerParametersHex.length : 0;
    console.error(
      `[prepareUnprovenCallTx] zswapHex=${zswapHexLen}ch ledgerHex=${ledgerHexLen}ch`,
    );
    let zswapChainState;
    try {
      zswapChainState = params.zswapChainStateHex
        ? ledgerV8.ZswapChainState.deserialize(hexToBytes(params.zswapChainStateHex))
        : new ledgerV8.ZswapChainState();
    } catch (e) {
      console.error(`[prepareUnprovenCallTx] ZswapChainState.deserialize FAILED: ${e}`);
      throw e;
    }
    let ledgerParameters;
    try {
      ledgerParameters = params.ledgerParametersHex
        ? ledgerV8.LedgerParameters.deserialize(hexToBytes(params.ledgerParametersHex))
        : ledgerV8.LedgerParameters.initialParameters();
    } catch (e) {
      console.error(`[prepareUnprovenCallTx] LedgerParameters.deserialize FAILED: ${e}`);
      throw e;
    }
    console.error(
      `[prepareUnprovenCallTx] zswap=${zswapChainState ? 'ok' : 'null'} ledgerParameters=${ledgerParameters ? 'ok' : 'null'}`,
    );

    // `args` may carry `{ $bigint: "n" }` placeholders for Field /
    // Uint args (same convention as `inspectCircuit`).
    const args = Array.isArray(params.circuitArgs)
      ? params.circuitArgs.map(reviveBigints)
      : [];

    const callTxData = await jsContracts.createUnprovenCallTxFromInitialStates(
      zkConfigProvider,
      {
        compiledContract,
        circuitId: params.circuit,
        contractAddress: params.contractAddressHex,
        args,
        coinPublicKey: params.coinPublicKeyHex,
        initialContractState: contractState,
        initialZswapChainState: zswapChainState,
        ledgerParameters,
        initialPrivateState: { secretKey: skBytes },
      },
      params.encryptionPublicKeyHex,
    );
    // Diagnostic: dump the partition the SDK chose. If [0] (guaranteed)
    // is empty and [1] (fallible) has content, our pipeline lands the
    // whole circuit in the fallible slot — which the chain rejects on
    // submit. Upstream's manager lands it in guaranteed. Same SDK
    // version (4.0.2), same compact-runtime (0.15.0), same inputs
    // (verified above) — so this exposes whichever input is still
    // wrong.
    try {
      const pt = callTxData.public.partitionedTranscript;
      const desc = (t) => {
        if (!t) return "null";
        const prog = t.program ? t.program.length : 0;
        const eff = t.effects ? Object.keys(t.effects).length : 0;
        return `program=${prog}ops effects=${eff}keys`;
      };
      console.error(
        `[prepareUnprovenCallTx] partition[0/guaranteed]: ${desc(pt?.[0])}`,
      );
      console.error(
        `[prepareUnprovenCallTx] partition[1/fallible]:  ${desc(pt?.[1])}`,
      );
    } catch (e) {
      console.error(`[prepareUnprovenCallTx] partition dump failed: ${e}`);
    }

    // `UnsubmittedCallTxData = CallResult & { private: UnsubmittedTxData }`
    // where the `unprovenTx` lives under `.private`. CallResultPublic
    // has the post-state + transcript; we don't need those here.
    const unprovenBytes = callTxData.private.unprovenTx.serialize();
    return {
      circuit: params.circuit,
      unprovenTxHex: bytesToHex(unprovenBytes),
      unprovenTxBytes: unprovenBytes.length,
      elapsedMs: Date.now() - t0,
    };
  },

  /**
   * Passport-vault analogue of `prepareUnprovenCallTx`. Builds an
   * unproven multi-lock call tx (`createLock` / `depositToLock` /
   * `withdrawFromLock`) for the passport-vault contract; the Rust side
   * balances/proves/submits via the identical downstream pipeline. The
   * circuit args are passed in `circuitArgs` ({ $bigint } / { $bytes }
   * markers). `privateState` carries the holder date-of-birth witness
   * for claim paths; create/deposit omit it (empty private state).
   * (`claimFromLock` is composed by the WebView's higher-level
   * `prepareVaultClaim`, not this generic path.)
   */
  prepareVaultCallTx: async (params) => {
    const t0 = Date.now();
    const [
      pvContract,
      pvWitnesses,
      cr,
      jsContracts,
      ledgerV8,
      compactJs,
      networkIdMod,
      zkProvMod,
    ] = await Promise.all([
      import(
        "@input-output-hk/passport-vault-contract/managed/passport-vault/contract/index.js"
      ),
      import("@input-output-hk/passport-vault-contract/witnesses.js"),
      import("@midnight-ntwrk/compact-runtime"),
      import("@midnight-ntwrk/midnight-js-contracts"),
      import("@midnight-ntwrk/ledger-v8"),
      import("@midnight-ntwrk/compact-js"),
      import("@midnight-ntwrk/midnight-js-network-id"),
      import("@midnight-ntwrk/midnight-js-node-zk-config-provider"),
    ]);
    networkIdMod.setNetworkId(params.networkId ?? "undeployed");

    const compiledContract = compactJs.CompiledContract.make(
      "passport-vault",
      pvContract.Contract,
    ).pipe(
      compactJs.CompiledContract.withWitnesses(pvWitnesses.witnesses),
      compactJs.CompiledContract.withCompiledFileAssets(PV_ZK_ASSETS_PATH),
    );
    const zkConfigProvider = new zkProvMod.NodeZkConfigProvider(
      PV_ZK_ASSETS_PATH,
    );

    const contractState = cr.ContractState.deserialize(
      hexToBytes(params.contractStateHex),
    );
    const zswapChainState = params.zswapChainStateHex
      ? ledgerV8.ZswapChainState.deserialize(
          hexToBytes(params.zswapChainStateHex),
        )
      : new ledgerV8.ZswapChainState();
    const ledgerParameters = params.ledgerParametersHex
      ? ledgerV8.LedgerParameters.deserialize(
          hexToBytes(params.ledgerParametersHex),
        )
      : ledgerV8.LedgerParameters.initialParameters();

    const args = Array.isArray(params.circuitArgs)
      ? params.circuitArgs.map(reviveBigints)
      : [];
    const initialPrivateState =
      params.privateState != null
        ? reviveBigints(params.privateState)
        : pvWitnesses.emptyPassportVaultPrivateState();

    const callTxData = await jsContracts.createUnprovenCallTxFromInitialStates(
      zkConfigProvider,
      {
        compiledContract,
        circuitId: params.circuit,
        contractAddress: params.contractAddressHex,
        args,
        coinPublicKey: params.coinPublicKeyHex,
        initialContractState: contractState,
        initialZswapChainState: zswapChainState,
        ledgerParameters,
        initialPrivateState,
      },
      params.encryptionPublicKeyHex,
    );
    const unprovenBytes = callTxData.private.unprovenTx.serialize();
    return {
      circuit: params.circuit,
      unprovenTxHex: bytesToHex(unprovenBytes),
      unprovenTxBytes: unprovenBytes.length,
      elapsedMs: Date.now() - t0,
    };
  },

  /**
   * Smoke test for the contract layer. Returns a summary of which
   * DID circuits + Compact runtime helpers are exposed by the
   * vendored package. The Rust side asserts on circuit names so a
   * future upstream rename is caught in CI.
   */
  /**
   * Bridge into `@midnight-ntwrk/midnight-did-jubjub-schnorr`'s
   * `pureCircuits.schnorrChallengeDigest`. Returns the
   * JS-computed challenge as a decimal-string bigint so the
   * Rust test can compare it to the Rust `transient_hash`
   * implementation. No reduction is applied here — the caller
   * mods by 2^248 on whichever side it prefers.
   *
   * Inputs are five bigints (announcement.x, announcement.y,
   * pk.x, pk.y, digest[0..4]) carried over the JSON wire as
   * `{ "$bigint": "<decimal>" }` tagged objects (see
   * `reviveBigints`).
   */
  schnorrChallenge: async (params) => {
    const t0 = Date.now();
    const schnorrLib = await import(
      "@midnight-ntwrk/midnight-did-jubjub-schnorr"
    );
    const ann = reviveBigints(params.announcement);
    const pk = reviveBigints(params.publicKey);
    const digest = reviveBigints(params.digest);
    if (!Array.isArray(digest) || digest.length !== 4) {
      throw new Error(
        `digest must be a 4-element array, got ${digest?.length}`,
      );
    }
    const challenge = schnorrLib.pureCircuits.schnorrChallengeDigest(
      ann.x,
      ann.y,
      pk.x,
      pk.y,
      digest,
    );
    return {
      challenge: challenge.toString(10),
      elapsedMs: Date.now() - t0,
    };
  },

  /**
   * Drive the impure `schnorrVerifyDigest` circuit from the
   * upstream `jubjub-schnorr` package. The witness
   * `getSchnorrReduction(cFull) -> [q, c]` is provided here:
   * `cFull = q * 2^248 + c` with `c < 2^248`. The Rust side
   * passes the Rust-computed signature (announcement + response)
   * + public key + digest; we evaluate the circuit and return
   * either `{ verified: true }` (no `assert` fired) or
   * `{ verified: false, error: <msg> }` (assert caught at the
   * circuit boundary).
   *
   * No contract address / on-chain state is involved — the
   * circuit just executes against an in-memory contract instance
   * with a placeholder `privateState`.
   */
  schnorrVerify: async (params) => {
    const t0 = Date.now();
    const schnorrLib = await import(
      "@midnight-ntwrk/midnight-did-jubjub-schnorr"
    );
    const { compactRuntime: cr } = await loadContractLayer();
    const ann = reviveBigints(params.announcement);
    const pk = reviveBigints(params.publicKey);
    const digest = reviveBigints(params.digest);
    const response = reviveBigints(params.response);
    if (!Array.isArray(digest) || digest.length !== 4) {
      throw new Error(
        `digest must be a 4-element array, got ${digest?.length}`,
      );
    }
    if (typeof response !== "bigint") {
      throw new Error("response must be a $bigint");
    }
    // 2^248 — module constant used by the witness.
    const TWO_248 = schnorrLib.TWO_248;
    const Contract = schnorrLib.JubjubSchnorrContract.Contract;
    const witnesses = {
      // `cFull` is the un-reduced challenge; split into a
      // 248-bit residue + the high quotient.
      getSchnorrReduction: (ctx, cFull) => {
        const c = cFull % TWO_248;
        const q = (cFull - c) / TWO_248;
        return [ctx.privateState, [q, c]];
      },
    };
    const contract = new Contract(witnesses);

    // Build a circuit context from a fresh constructor run. The
    // schnorr module's `Ledger` is `{}` so the initial state is
    // effectively a placeholder — we just need *something*
    // shaped like a ContractState so `createCircuitContext`
    // accepts it.
    const dummyCoinPk = "00".repeat(32);
    const dummyAddress = "00".repeat(32);
    const initial = contract.initialState(
      cr.createConstructorContext({}, dummyCoinPk),
    );
    const ctx = cr.createCircuitContext(
      dummyAddress,
      dummyCoinPk,
      initial.currentContractState,
      initial.currentPrivateState,
    );
    let result;
    try {
      result = contract.impureCircuits.schnorrVerifyDigest(
        ctx,
        digest,
        { announcement: { x: ann.x, y: ann.y }, response },
        { x: pk.x, y: pk.y },
      );
    } catch (e) {
      return {
        verified: false,
        error: String(e?.message ?? e),
        elapsedMs: Date.now() - t0,
      };
    }
    // `schnorrVerifyDigest` returns `[]` (unit) on success; if
    // we got this far, no assert fired.
    return {
      verified: true,
      result: Array.isArray(result?.result)
        ? `[${result.result.length} items]`
        : "ok",
      elapsedMs: Date.now() - t0,
    };
  },

  /**
   * Decode an upstream-encoded Jubjub Schnorr signature (96
   * bytes: `ann.x BE || ann.y BE || response BE`) via
   * `decodeJubjubSignature`, then run the on-chain
   * `schnorrVerifyDigest` circuit on it. The Rust test passes
   * the hex of the upstream wire form to assert that the Rust
   * port's `encode_upstream` matches upstream's
   * `encodeJubjubSignature` byte-for-byte — if the JS lib
   * decodes our bytes into a valid signature object and the
   * circuit accepts it, the two implementations are wire-
   * compatible.
   *
   * Inputs:
   *   - signatureHex: 192-char hex (96 bytes), upstream form
   *   - publicKey:  { x: $bigint, y: $bigint }
   *   - digest:     Array<$bigint>(4)
   *
   * Returns `{ verified, decoded: { announcement, response }, error?, elapsedMs }`.
   */
  schnorrVerifyUpstreamEncoded: async (params) => {
    const t0 = Date.now();
    const schnorrLib = await import(
      "@midnight-ntwrk/midnight-did-jubjub-schnorr"
    );
    const { compactRuntime: cr } = await loadContractLayer();

    const sigBytes = hexToBytes(params.signatureHex);
    if (sigBytes.length !== 96) {
      throw new Error(
        `signatureHex must encode 96 bytes (got ${sigBytes.length})`,
      );
    }
    const sig = schnorrLib.decodeJubjubSignature(sigBytes);

    const pk = reviveBigints(params.publicKey);
    const digest = reviveBigints(params.digest);
    if (!Array.isArray(digest) || digest.length !== 4) {
      throw new Error(
        `digest must be a 4-element array, got ${digest?.length}`,
      );
    }

    const TWO_248 = schnorrLib.TWO_248;
    const Contract = schnorrLib.JubjubSchnorrContract.Contract;
    const witnesses = {
      getSchnorrReduction: (ctx, cFull) => {
        const c = cFull % TWO_248;
        const q = (cFull - c) / TWO_248;
        return [ctx.privateState, [q, c]];
      },
    };
    const contract = new Contract(witnesses);
    const dummyCoinPk = "00".repeat(32);
    const dummyAddress = "00".repeat(32);
    const initial = contract.initialState(
      cr.createConstructorContext({}, dummyCoinPk),
    );
    const ctx = cr.createCircuitContext(
      dummyAddress,
      dummyCoinPk,
      initial.currentContractState,
      initial.currentPrivateState,
    );
    let verified = false;
    let error;
    try {
      contract.impureCircuits.schnorrVerifyDigest(
        ctx,
        digest,
        sig,
        { x: pk.x, y: pk.y },
      );
      verified = true;
    } catch (e) {
      error = String(e?.message ?? e);
    }
    return {
      verified,
      decoded: {
        announcement: {
          x: sig.announcement.x.toString(10),
          y: sig.announcement.y.toString(10),
        },
        response: sig.response.toString(10),
      },
      error,
      elapsedMs: Date.now() - t0,
    };
  },

  /**
   * Decode a Compact-value-encoded digital-passport credential.
   *
   * Takes an `EncodedCompactValue` object of shape
   *   `{ encoding: "compact-value-v1.base64url", payload: "..." }`
   * as returned by the Rust HTTP response (or a prior encode call),
   * and returns the structured JSON fields
   * (issuerVerificationMethodRef, holderBinding, schema, issuedAt, etc.).
   *
   * Uses `decodeDigitalPassportCredential` from the
   * `@midnight-ntwrk/midnight-did-credentials-digital-passport` package,
   * which internally calls `decodeCompactPayload` from
   * `@midnight-ntwrk/midnight-did-credentials-openid` to unwrap the
   * base64url encoding, then applies the credential descriptor to
   * parse the binary fields into a plain JS object.
   */
  decodeDigitalPassportCredential: async (params) => {
    const dpCred = await import(
      "@midnight-ntwrk/midnight-did-credentials-digital-passport"
    );
    const credential = dpCred.decodeDigitalPassportCredential(params.encoded);
    // Convert bigint/Uint8Array values to JSON-safe strings so the
    // Rust side can deserialise over JSON-RPC.
    return { credential: bigIntSafe(credential) };
  },

  /**
   * Decode a Compact-value-encoded digital-passport proof.
   *
   * Takes an `EncodedCompactValue` object of shape
   *   `{ encoding: "compact-value-v1.base64url", payload: "..." }`
   * and returns the structured JSON fields
   * (signerVerificationMethodRef, createdAt, challengeHash, publicKey,
   * signature).
   */
  decodeDigitalPassportProof: async (params) => {
    const dpCred = await import(
      "@midnight-ntwrk/midnight-did-credentials-digital-passport"
    );
    const proof = dpCred.decodeDigitalPassportProof(params.encoded);
    return { proof: bigIntSafe(proof) };
  },

  /**
   * Verify a digital-passport issuance proof against a credential.
   *
   * Takes two `EncodedCompactValue` objects — one for the credential,
   * one for the proof — decodes both, then runs pureCircuit checks:
   *   1. `digitalPassportCredentialBodyRoot(credential)` → bodyRoot
   *   2. `assertValidIssuanceContextProof(bodyRoot, proof)`
   *
   * Returns `{ valid: true }` if neither circuit assertion throws.
   * Returns `{ valid: false, error: "..." }` if any assertion fails.
   */
  verifyDigitalPassportIssuanceProof: async (params) => {
    const t0 = Date.now();
    try {
      const dpCred = await import(
        "@midnight-ntwrk/midnight-did-credentials-digital-passport"
      );
      const credential = dpCred.decodeDigitalPassportCredential(params.credentialEncoded);
      const proof = dpCred.decodeDigitalPassportProof(params.proofEncoded);
      const { pureCircuits } = dpCred;
      // Compute the Merkle root of the credential body fields.
      const bodyRoot = pureCircuits.digitalPassportCredentialBodyRoot(credential);
      // Verify that the proof is a valid Schnorr issuance proof
      // signed over this body root.
      pureCircuits.assertValidIssuanceContextProof(bodyRoot, proof);
      return { valid: true, elapsedMs: Date.now() - t0 };
    } catch (e) {
      return {
        valid: false,
        error: String(e?.message ?? e),
        elapsedMs: Date.now() - t0,
      };
    }
  },

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
