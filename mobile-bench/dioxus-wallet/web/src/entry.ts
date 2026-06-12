// Bundle entry. Static imports here are resolved by esbuild at
// build time. Dynamic imports of `@midnight-ntwrk/...` resolve at
// runtime through the import map → `mn-pkg://` custom protocol →
// vendored `assets/web/pkg/<name>/...`.

import * as midnightDid from "@midnight-ntwrk/midnight-did";
import * as midnightDidDomain from "@midnight-ntwrk/midnight-did-domain";
import {
  createUnprovenCallTxFromInitialStates,
} from "@midnight-ntwrk/midnight-js-contracts";
import { setNetworkId } from "@midnight-ntwrk/midnight-js-network-id";
import {
  ZKConfigProvider,
  createProverKey,
  createVerifierKey,
  createZKIR,
} from "@midnight-ntwrk/midnight-js-types";
// Pure-JS QR decoder. ~40 KB minified — esbuild inlines it straight
// into the bundle so no extra `mn-pkg://` entry is needed. Used by
// `scanQr()` below to decode frames captured from the device camera
// via `navigator.mediaDevices.getUserMedia`.
import jsQR from "jsqr";

declare global {
  interface Window {
    midnightDidBundle: {
      version: string;
      did: typeof midnightDid;
      didDomain: typeof midnightDidDomain;
      ready: boolean;
      /** Lazy-load the WASM-touching contract layer + dependencies.
       *  First call pays the WebAssembly compile cost (typically a
       *  few hundred ms cold). Subsequent calls return the cached
       *  module reference. */
      loadContractLayer(): Promise<{
        contract: typeof import("@midnight-ntwrk/midnight-did-contract");
        compactRuntime: typeof import("@midnight-ntwrk/compact-runtime");
      }>;
      /** Round-trip probe: callable from Rust via Dioxus `eval`,
       *  reports what's loaded in the bundle. Used as the first
       *  step toward the ContractCall bridge — verifies that Rust
       *  can drive JS and get a structured result back. */
      bridgeProbe(params: { message: string }): Promise<{
        echoed: string;
        version: string;
        bundleReady: boolean;
        contractLayerLoaded: boolean;
        contractExports: string[];
        compactRuntimeExports: string[];
        timeMs: number;
      }>;
      /** Nested round-trip: Rust → JS → (back to Rust) → JS → Rust.
       *  Exercises the witness-callback chain we need for circuit
       *  execution. Returns the public hash of the controller's
       *  secret key (so the secret never leaves the WebView in the
       *  return path either), plus an `originHex` field that's the
       *  raw secret hex — only useful in this spike for verifying
       *  the round-trip; production circuit calls feed the bytes
       *  directly into Compact's witness slot and never log them. */
      bridgeWitnessTest(params: { did: string }): Promise<{
        sourceLength: number;
        controllerPkPublic: string;
        secretHexFirst8: string;
        elapsedMs: number;
      }>;
      /** Produce a SCALE-serialised `UnprovenTransaction` that calls
       *  a DID circuit on a deployed contract. Mirrors the
       *  `prepareUnprovenCallTx` handler in
       *  `mobile-bench/wallet-core/tests/js-harness/harness.mjs` but
       *  runs entirely inside the embedded WebView so Android (no
       *  Node) can drive `Wallet::call_did_circuit`.
       *  Returns `{ unprovenTxHex, elapsedMs }` for the Rust side to
       *  deserialise, balance, prove, and submit. */
      prepareUnprovenCallTx(params: PrepareUnprovenCallTxParams):
        Promise<PrepareUnprovenCallTxResult>;
      /** Passport-vault analogue of `prepareUnprovenCallTx`. Builds an
       *  unproven `depositFunds` / `claimFunds` (etc.) call tx for the
       *  passport-vault contract; the Rust side balances/proves/submits
       *  via the identical downstream pipeline. */
      prepareVaultCallTx(params: PrepareVaultCallTxParams):
        Promise<PrepareVaultCallTxResult>;
      /** High-level passport-vault claim compose: deserialise the
       *  credential bundle, read the on-chain policy, build the
       *  selective-disclosure presentation + age proof, and compose
       *  `claimFunds`. Returns the unproven tx for Rust to
       *  balance/prove/submit. */
      prepareVaultClaim(params: PrepareVaultClaimParams):
        Promise<PrepareVaultCallTxResult>;
      /** Decode the passport-vault contract's ledger fields from a
       *  serialised `ContractState` hex (as returned by the indexer).
       *  Used by the wallet's `vaultTotalLocked` bridge verb to show the
       *  live locked total in the embedded dApp without a Compact
       *  runtime in the browser. `totalLockedBaseUnits` is the current
       *  escrow value (`escrowVault.value` when `hasDeposit`, else 0);
       *  `totalDepositedBaseUnits` is the cumulative lifetime deposit. */
      readVaultLedger(params: { contractStateHex: string }): Promise<{
        totalLockedBaseUnits: string;
        totalDepositedBaseUnits: string;
        hasDeposit: boolean;
      }>;
      /** Enumerate the multi-lock vault's `locks` map + `lockCount` for
       *  the dApp's lock list + claim selector. */
      readVaultLocks(params: { contractStateHex: string }): Promise<{
        lockCount: string;
        locks: Array<Record<string, unknown>>;
      }>;
      /** WebView-based QR scanner. Opens a full-viewport overlay with
       *  a back-facing camera preview, runs jsQR on every animation
       *  frame, resolves with `{ url }` on the first decode. Cancel
       *  button resolves with `{ error: "cancelled" }`; permission
       *  denial / no camera resolves with `{ error: "<reason>" }`.
       *  Idempotent — concurrent calls return `{ error: "busy" }`. */
      scanQr(params?: Record<string, unknown>):
        Promise<{ url?: string; error?: string }>;
      /** Read the system clipboard's text content via
       *  `navigator.clipboard.readText()`. Used by the Identity
       *  Centre's "📋 Paste" buttons because iOS WKWebView's
       *  long-press / Cmd-V paste affordance into `<textarea>`
       *  is unreliable (works fine on desktop Wry but the
       *  iOS sim + real-device path silently no-ops). The button
       *  click is the required user gesture; iOS may show a
       *  one-time "Allow Paste from <App>?" prompt before the
       *  first call. */
      pasteText(): Promise<{ text?: string; error?: string }>;
      /** Decode a Compact-value-encoded digital-passport credential.
       *  Takes an `EncodedCompactValue` object `{ encoding, payload }`
       *  and returns the structured JSON fields (issuer DID contract
       *  address, holder binding, schema, issuedAt, etc.). */
      decodeDigitalPassportCredential(params: {
        encoded?: { encoding: string; payload: string };
        encoding?: string;
        payload?: string;
        network?: string;
      }): Promise<{ credential: Record<string, unknown>; issuerDid: string }>;
      /** Decode a Compact-value-encoded digital-passport proof.
       *  Takes an `EncodedCompactValue` object `{ encoding, payload }`
       *  and returns the structured JSON fields
       *  (signerVerificationMethodRef, publicKey, signature, etc.). */
      decodeDigitalPassportProof(params: {
        encoded?: { encoding: string; payload: string };
        encoding?: string;
        payload?: string;
      }): Promise<{ proof: Record<string, unknown> }>;
      /** Verify a digital-passport issuance proof against a credential.
       *  Decodes both compact values, computes the credential body root,
       *  then checks the proof via `pureCircuits.assertValidIssuanceContextProof`.
       *  Returns `{ valid: true }` on success or `{ valid: false, error }` on failure. */
      verifyDigitalPassportIssuanceProof(params: {
        credentialEncoded: { encoding: string; payload: string };
        proofEncoded: { encoding: string; payload: string };
      }): Promise<{ valid: boolean; error?: string; elapsedMs?: number }>;
    };
    __qrScanInProgress?: boolean;
    MIDNIGHT_PROOF_SERVER?: string;
    MIDNIGHT_NETWORK?: string;
  }
}

/** Mirrors the params the `wallet-core::wallet::call_prepare_unproven`
 *  shim builds. Hex values are SCALE-tagged blobs; bigint placeholders
 *  inside `circuitArgs` use `{ $bigint: "<decimal>" }` per
 *  `reviveBigints`. */
export interface PrepareUnprovenCallTxParams {
  did: string;
  circuit: string;
  circuitArgs: unknown[];
  contractStateHex: string;
  contractAddressHex: string;
  zswapChainStateHex?: string | null;
  ledgerParametersHex?: string | null;
  controllerSecretHex: string;
  coinPublicKeyHex: string;
  encryptionPublicKeyHex: string;
  networkId: string;
}

export interface PrepareUnprovenCallTxResult {
  circuit: string;
  unprovenTxHex: string;
  unprovenTxBytes: number;
  elapsedMs: number;
}

let contractLayerPromise:
  | Promise<{
      contract: typeof import("@midnight-ntwrk/midnight-did-contract");
      compactRuntime: typeof import("@midnight-ntwrk/compact-runtime");
    }>
  | null = null;

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

/**
 * Nested round-trip helper. Touches the contract layer (so the
 * Compact runtime is loaded), then calls back into Rust via the
 * existing JSON-RPC bridge to fetch the controller secret bytes,
 * then computes `publicKey(sk)` via the bundled `pureCircuits`
 * helper to verify the bytes round-trip is faithful.
 */
async function bridgeWitnessTest(params: { did: string }) {
  const t0 = Date.now();
  const layer = await loadContractLayer();
  const bridge = (window as any).midnightWallet;
  if (!bridge?.getControllerSecretKey) {
    throw new Error("midnightWallet.getControllerSecretKey not exposed by the bridge");
  }
  const { secretKeyHex } = await bridge.getControllerSecretKey(params.did);
  if (typeof secretKeyHex !== "string" || secretKeyHex.length !== 64) {
    throw new Error(`unexpected secret length: ${secretKeyHex?.length}`);
  }
  // Hex → Uint8Array(32).
  const sk = new Uint8Array(32);
  for (let i = 0; i < 32; i++) {
    sk[i] = parseInt(secretKeyHex.slice(i * 2, i * 2 + 2), 16);
  }
  const pk = layer.contract.DIDContract.pureCircuits.publicKey(sk);
  const pkHex = Array.from(pk, (b) => b.toString(16).padStart(2, "0")).join("");
  return {
    sourceLength: sk.length,
    controllerPkPublic: pkHex,
    secretHexFirst8: secretKeyHex.slice(0, 8),
    elapsedMs: Date.now() - t0,
  };
}

async function bridgeProbe(params: { message: string }) {
  // Touch the contract layer so the probe also reports its load
  // status. If the dynamic import has already happened (smoke
  // test ran on startup) this is a no-op cache hit.
  let layer: Awaited<ReturnType<typeof loadContractLayer>> | null = null;
  try {
    layer = await loadContractLayer();
  } catch (e) {
    console.warn("[bridgeProbe] contract layer load failed", e);
  }
  return {
    echoed: params.message,
    version: "0.1.0",
    bundleReady: true,
    contractLayerLoaded: layer !== null,
    contractExports: layer ? Object.keys(layer.contract).slice(0, 16) : [],
    compactRuntimeExports: layer
      ? Object.keys(layer.compactRuntime).slice(0, 16)
      : [],
    timeMs: Date.now(),
  };
}

/**
 * `ZKConfigProvider` that fetches the per-circuit prover key, verifier
 * key, and zkIR over the `mn-pkg://` custom protocol (rewritten to
 * `http://mn-pkg.localhost/...` on Android) instead of via Node's
 * `fs`. Mirrors upstream `NodeZkConfigProvider` byte-for-byte except
 * for the transport. The blobs come from the embedded
 * `assets/web/pkg/midnight-did-contract/dist/managed/did/{keys,zkir}/`
 * tree so the prover/verifier/zkir layout is identical to what the
 * Node harness reads.
 */
class WebViewZkConfigProvider extends ZKConfigProvider<string> {
  readonly baseUrl: string;
  constructor(baseUrl: string) {
    super();
    this.baseUrl = baseUrl.replace(/\/+$/, "");
  }
  private async fetchBytes(subDir: string, circuitId: string, ext: string):
    Promise<Uint8Array> {
    const url = `${this.baseUrl}/${subDir}/${circuitId}${ext}`;
    const resp = await fetch(url);
    if (!resp.ok) {
      throw new Error(
        `fetch ${url} failed: ${resp.status} ${resp.statusText}`,
      );
    }
    return new Uint8Array(await resp.arrayBuffer());
  }
  async getProverKey(circuitId: string) {
    return createProverKey(await this.fetchBytes("keys", circuitId, ".prover"));
  }
  async getVerifierKey(circuitId: string) {
    return createVerifierKey(
      await this.fetchBytes("keys", circuitId, ".verifier"),
    );
  }
  async getZKIR(circuitId: string) {
    return createZKIR(await this.fetchBytes("zkir", circuitId, ".bzkir"));
  }
}

/** Pick the right scheme for the current host. Wry-Android rewrites
 *  custom-protocol URLs to `http://{name}.{authority}/...` so the
 *  Chromium WebView's intercept callback can match them; desktop
 *  Wry registers the custom scheme directly with the platform
 *  WebView. We mirror the import-map heuristic from `lib.rs`. */
function pkgBaseUrlFor(packagePath: string): string {
  const isAndroidHost = (typeof location !== "undefined")
    && location.protocol.startsWith("http")
    && location.host.endsWith(".localhost");
  if (isAndroidHost) {
    return `http://mn-pkg.localhost/${packagePath}`;
  }
  // Desktop: dioxus-desktop loads the page over the custom `dioxus://`
  // scheme, which is opaque to URL parsing. Use the mn-pkg:// form
  // that's registered as a custom protocol.
  return `mn-pkg://localhost/${packagePath}`;
}

/** Walk a JSON value, replacing `{ $bigint: "<dec>" }` objects with
 *  the corresponding JS bigint. Mirrors the harness convention so
 *  Rust serialises bigint args the same way for both transports. */
function reviveBigints(value: unknown): unknown {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map(reviveBigints);
  const obj = value as Record<string, unknown>;
  if (typeof obj.$bigint === "string") return BigInt(obj.$bigint);
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(obj)) out[k] = reviveBigints(v);
  return out;
}

function hexToBytes(hex: string): Uint8Array {
  const clean = hex.startsWith("0x") ? hex.slice(2) : hex;
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

function bytesToHex(b: Uint8Array): string {
  let s = "";
  for (let i = 0; i < b.length; i++) {
    s += b[i].toString(16).padStart(2, "0");
  }
  return s;
}

/**
 * Port of `methods.prepareUnprovenCallTx` from the Node harness
 * (`mobile-bench/wallet-core/tests/js-harness/harness.mjs`). Runs the
 * upstream `createUnprovenCallTxFromInitialStates` pipeline inside the
 * WebView with a `fetch`-backed `ZKConfigProvider`. Mirrors the same
 * input/output shape so the Rust side of the bridge doesn't care
 * which transport executed it.
 */
async function prepareUnprovenCallTx(
  params: PrepareUnprovenCallTxParams,
): Promise<PrepareUnprovenCallTxResult> {
  const t0 = Date.now();
  const { contract: c, compactRuntime: cr } = await loadContractLayer();
  // Dynamic import of `ledger-v8` so the WASM module is only fetched
  // when actually exercised (the bundle's other code paths already
  // pull `compact-runtime` and `midnight-did-contract` on the cold
  // path; ledger-v8 is an extra ~MB of WASM only this flow needs).
  const ledgerV8 = await import("@midnight-ntwrk/ledger-v8");
  const compactJs = await import("@midnight-ntwrk/compact-js");

  setNetworkId((params.networkId ?? "undeployed") as Parameters<typeof setNetworkId>[0]);

  const skBytes = hexToBytes(params.controllerSecretHex);
  if (skBytes.length !== 32) {
    throw new Error(
      `controllerSecretHex must be 32 bytes, got ${skBytes.length}`,
    );
  }

  // Witnesses for the DID contract. Mirrors the upstream
  // `witnesses.ts` shape from `midnight-did-contract`. Rust
  // already supplies `controllerSecretHex` for the DID we're
  // calling — `localSecretKey` returns that. `currentTimestamp`
  // is read by the contract's `recordUpdate` helper. The
  // `getSchnorrReduction` witness is invoked by Schnorr-using
  // circuits with a `challengeHash` bigint; it splits the hash
  // into high/low halves around 2^248 (= JubjubSchnorr digest
  // reduction). The old stub `[0n, 0n]` worked under the
  // pre-redesign contract because no circuit then exercised
  // this witness, but the redesigned contract's
  // `verifySchnorrJubjubDigestSignature` (and likely
  // `assertControllerCanUpdate` via Schnorr signature paths)
  // pulls it for every controller-gated write. See
  // `<midnight-did-source>/packages/contract/src/witnesses.ts`.
  const TWO_248 = 452312848583266388373324160190187140051835877600158453279131187530910662656n;
  const witnesses = {
    localSecretKey: (ctx: { privateState: unknown }) => [ctx.privateState, skBytes],
    currentTimestamp: (ctx: { privateState: unknown }) => [ctx.privateState, BigInt(Date.now())],
    getSchnorrReduction: (
      ctx: { privateState: unknown },
      challengeHash: bigint,
    ) => [ctx.privateState, [challengeHash / TWO_248, challengeHash % TWO_248]],
  };

  const compiledContract = (compactJs as any).CompiledContract.make(
    "did",
    (c as any).DIDContract.Contract,
  ).pipe(
    (compactJs as any).CompiledContract.withWitnesses(witnesses),
    // Path is consumed only by API surfaces that build their own
    // zkConfigProvider from disk; `createUnprovenCallTxFromInitialStates`
    // uses the provider we pass explicitly so the value is a marker.
    (compactJs as any).CompiledContract.withCompiledFileAssets("did"),
  );

  const zkConfigProvider = new WebViewZkConfigProvider(
    pkgBaseUrlFor("midnight-did-contract/dist/managed/did"),
  );

  const contractState = (cr as any).ContractState.deserialize(
    hexToBytes(params.contractStateHex),
  );
  let zswapChainState: unknown;
  if (params.zswapChainStateHex) {
    zswapChainState = (ledgerV8 as any).ZswapChainState.deserialize(
      hexToBytes(params.zswapChainStateHex),
    );
  } else {
    zswapChainState = new (ledgerV8 as any).ZswapChainState();
  }
  let ledgerParameters: unknown;
  if (params.ledgerParametersHex) {
    ledgerParameters = (ledgerV8 as any).LedgerParameters.deserialize(
      hexToBytes(params.ledgerParametersHex),
    );
  } else {
    ledgerParameters = (ledgerV8 as any).LedgerParameters.initialParameters();
  }

  const args = Array.isArray(params.circuitArgs)
    ? params.circuitArgs.map(reviveBigints)
    : [];

  let callTxData;
  try {
    callTxData = await (createUnprovenCallTxFromInitialStates as any)(
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
  } catch (e) {
    // The default Rust-side error wrap (`e.stack || e.message
    // || e`) loses the message because WebKit's `e.stack` is
    // bare stack frames with no message prefix. Surface a
    // structured `bundleError` event first so the message +
    // call context land in the host log; then re-throw so the
    // outer Rust caller still propagates the failure.
    const err = e instanceof Error ? e : new Error(String(e));
    try {
      await window.midnightWallet.bundleError({
        kind: "prepareUnprovenCallTxFailed",
        message: `${err.name}: ${err.message} (circuit=${params.circuit}, args=${args.length})`,
        stack: err.stack || "",
      });
    } catch (_) {}
    throw err;
  }

  const unprovenBytes: Uint8Array = callTxData.private.unprovenTx.serialize();
  return {
    circuit: params.circuit,
    unprovenTxHex: bytesToHex(unprovenBytes),
    unprovenTxBytes: unprovenBytes.length,
    elapsedMs: Date.now() - t0,
  };
}

/**
 * WebView-based QR scanner. Runs entirely in the WebView — no native
 * camera bridge — so a single code path works on both Android and
 * iOS. The overlay element is appended to `document.body` (not the
 * Dioxus render tree) so the framework can't yank it on rerender;
 * the `<video>` element is mounted into that overlay and the stream
 * is torn down on success / cancel / error.
 *
 * On every animation frame we draw the video to an offscreen
 * `<canvas>` and feed its imageData to `jsQR`. On the first hit we
 * stop the loop and resolve. `inversionAttempts: "dontInvert"` is
 * the default; we skip inversion for ~2× decode speed since OID4VP
 * / OID4VCI QRs are always dark-on-light.
 */
async function scanQr(
  _params?: Record<string, unknown>,
): Promise<{ url?: string; error?: string }> {
  if (window.__qrScanInProgress) {
    return { error: "busy" };
  }
  window.__qrScanInProgress = true;

  let stream: MediaStream | null = null;
  let overlay: HTMLDivElement | null = null;
  let rafId: number | null = null;
  let settled = false;

  const teardown = () => {
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
    if (stream) {
      for (const track of stream.getTracks()) {
        try {
          track.stop();
        } catch (_) {}
      }
      stream = null;
    }
    if (overlay && overlay.parentNode) {
      overlay.parentNode.removeChild(overlay);
    }
    overlay = null;
    window.__qrScanInProgress = false;
  };

  return new Promise((resolve) => {
    const settle = (out: { url?: string; error?: string }) => {
      if (settled) return;
      settled = true;
      teardown();
      resolve(out);
    };

    // ── Acquire camera stream ──────────────────────────────────────
    if (!navigator.mediaDevices?.getUserMedia) {
      settle({ error: "getUserMedia not available" });
      return;
    }

    // ── Build overlay DOM ──────────────────────────────────────────
    overlay = document.createElement("div");
    overlay.setAttribute("data-midnight-qr-overlay", "1");
    overlay.style.cssText = [
      "position: fixed",
      "inset: 0",
      "z-index: 2147483647",
      "background: rgba(0, 0, 0, 0.85)",
      "display: flex",
      "flex-direction: column",
      "align-items: center",
      "justify-content: center",
      "padding: 16px",
      "box-sizing: border-box",
    ].join("; ");

    const title = document.createElement("div");
    title.textContent = "Scan QR code";
    title.style.cssText = [
      "color: #fff",
      "font-family: -apple-system, system-ui, sans-serif",
      "font-size: 16px",
      "margin-bottom: 12px",
    ].join("; ");
    overlay.appendChild(title);

    const video = document.createElement("video");
    // `playsinline` is required on iOS to keep the stream embedded
    // (without it iOS would fullscreen-promote the element and steal
    // the WebView's view tree).
    video.setAttribute("playsinline", "true");
    video.setAttribute("autoplay", "true");
    video.muted = true;
    video.style.cssText = [
      "max-width: 100%",
      "max-height: 70vh",
      "background: #000",
      "border-radius: 8px",
    ].join("; ");
    overlay.appendChild(video);

    const status = document.createElement("div");
    status.textContent = "Point the camera at a QR code…";
    status.style.cssText = [
      "color: #ccc",
      "font-family: -apple-system, system-ui, sans-serif",
      "font-size: 13px",
      "margin-top: 12px",
      "text-align: center",
    ].join("; ");
    overlay.appendChild(status);

    const cancelBtn = document.createElement("button");
    cancelBtn.textContent = "Cancel";
    cancelBtn.style.cssText = [
      "margin-top: 16px",
      "padding: 10px 24px",
      "background: #fff",
      "color: #000",
      "border: 0",
      "border-radius: 8px",
      "font-family: -apple-system, system-ui, sans-serif",
      "font-size: 15px",
      "cursor: pointer",
    ].join("; ");
    cancelBtn.onclick = () => settle({ error: "cancelled" });
    overlay.appendChild(cancelBtn);

    document.body.appendChild(overlay);

    // Offscreen canvas — kept outside the DOM so layout doesn't
    // recompute for it. `willReadFrequently: true` hints the browser
    // to use a software-readable backing store, which is what we
    // want since we call `getImageData` every frame.
    const canvas = document.createElement("canvas");
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) {
      settle({ error: "could not allocate 2d canvas context" });
      return;
    }

    const tick = () => {
      if (settled) return;
      if (
        video.readyState === video.HAVE_ENOUGH_DATA &&
        video.videoWidth > 0 &&
        video.videoHeight > 0
      ) {
        canvas.width = video.videoWidth;
        canvas.height = video.videoHeight;
        ctx.drawImage(video, 0, 0, canvas.width, canvas.height);
        const imageData = ctx.getImageData(0, 0, canvas.width, canvas.height);
        const code = jsQR(imageData.data, imageData.width, imageData.height, {
          inversionAttempts: "dontInvert",
        });
        if (code && typeof code.data === "string" && code.data.length > 0) {
          settle({ url: code.data });
          return;
        }
      }
      rafId = requestAnimationFrame(tick);
    };

    navigator.mediaDevices
      .getUserMedia({
        video: { facingMode: { ideal: "environment" } },
        audio: false,
      })
      .then((s) => {
        if (settled) {
          // Cancel pressed while permission prompt was up.
          for (const t of s.getTracks()) {
            try {
              t.stop();
            } catch (_) {}
          }
          return;
        }
        stream = s;
        video.srcObject = s;
        video
          .play()
          .catch((e) =>
            console.warn("[scanQr] video.play() rejected", e),
          );
        rafId = requestAnimationFrame(tick);
      })
      .catch((e) => {
        const msg =
          e instanceof Error
            ? e.name === "NotAllowedError"
              ? "camera permission denied"
              : `${e.name}: ${e.message}`
            : String(e);
        settle({ error: msg });
      });
  });
}

async function pasteText(): Promise<{ text?: string; error?: string }> {
  // Modern iOS WKWebView + macOS WebKit expose
  // `navigator.clipboard.readText()` when called from a user
  // gesture. The bridge driver only invokes us via a button
  // click, which counts as a gesture, so the Promise resolves
  // without an explicit permission API call. iOS may still
  // surface a one-time system prompt before the first read.
  try {
    if (!navigator?.clipboard?.readText) {
      return { error: "navigator.clipboard.readText unavailable" };
    }
    const text = await navigator.clipboard.readText();
    return { text };
  } catch (e) {
    return { error: e instanceof Error ? e.message : String(e) };
  }
}

/**
 * Recursively convert a value to a JSON-safe form:
 * - BigInt → decimal string
 * - Uint8Array → hex string (0x-prefixed)
 * - plain objects/arrays → recurse
 */
function bigIntSafeEntry(value: unknown): unknown {
  if (value === null || value === undefined) return value;
  if (typeof value === "bigint") return value.toString(10);
  if (value instanceof Uint8Array) return "0x" + bytesToHex(value as Uint8Array);
  if (Array.isArray(value)) return value.map(bigIntSafeEntry);
  if (typeof value === "object") {
    const out: Record<string, unknown> = {};
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      out[k] = bigIntSafeEntry(v);
    }
    return out;
  }
  return value;
}

/**
 * Decode a Compact-value-encoded digital-passport credential.
 * Takes an `EncodedCompactValue` object `{ encoding, payload }` and
 * returns the structured JSON fields (issuer DID contract address,
 * holder binding, schema, issuedAt, etc.).
 */
async function decodeDigitalPassportCredential(params: {
  encoded?: { encoding: string; payload: string };
  encoding?: string;
  payload?: string;
  network?: string;
}): Promise<{ credential: Record<string, unknown>; issuerDid: string }> {
  const dpCred = await import(
    "@midnight-ntwrk/midnight-did-credentials-digital-passport"
  );
  const encoded = normaliseEncodedCompactValue(params);
  const credential = dpCred.decodeDigitalPassportCredential(encoded);
  const safe = bigIntSafeEntry(credential) as Record<string, any>;
  // Derive the issuer DID string (VC-store metadata) from the issuer
  // verification-method ref's contract address + the caller-supplied
  // network tag. The on-chain claim trusts the issuer KEY (not this
  // string), so a missing network only affects display.
  const addrHex = String(
    (safe?.issuerVerificationMethodRef as any)?.didContractAddress?.bytes ?? "",
  ).replace(/^0x/, "");
  const net = params.network ?? "undeployed";
  const issuerDid = addrHex
    ? `did:midnight:${net}:${addrHex}`
    : "did:midnight:unknown";
  return { credential: safe, issuerDid };
}

/** Normalise the various shapes callers pass for a Compact-value blob into
 *  the `{ encoding, payload }` the digital-passport codec expects:
 *  `{ encoded: {encoding,payload} }` | `{ encoding, payload }` | `{ payload }`. */
function normaliseEncodedCompactValue(params: {
  encoded?: { encoding: string; payload: string };
  encoding?: string;
  payload?: string;
}): { encoding: string; payload: string } {
  if (params.encoded && typeof params.encoded.payload === "string") {
    return {
      encoding: params.encoded.encoding ?? "compact-value-v1.base64url",
      payload: params.encoded.payload,
    };
  }
  if (typeof params.payload === "string") {
    return {
      encoding: params.encoding ?? "compact-value-v1.base64url",
      payload: params.payload,
    };
  }
  throw new Error(
    "decodeDigitalPassportCredential: expected { encoded: { encoding, payload } } or { payload }",
  );
}

/**
 * Decode a Compact-value-encoded digital-passport proof.
 * Takes an `EncodedCompactValue` object `{ encoding, payload }` and
 * returns the structured JSON fields (signerVerificationMethodRef,
 * publicKey, signature, etc.).
 */
async function decodeDigitalPassportProof(params: {
  encoded?: { encoding: string; payload: string };
  encoding?: string;
  payload?: string;
}): Promise<{ proof: Record<string, unknown> }> {
  const dpCred = await import(
    "@midnight-ntwrk/midnight-did-credentials-digital-passport"
  );
  const proof = dpCred.decodeDigitalPassportProof(
    normaliseEncodedCompactValue(params),
  );
  return { proof: bigIntSafeEntry(proof) as Record<string, unknown> };
}

/**
 * Verify a digital-passport issuance proof against a credential.
 * Decodes both compact values, computes the credential body root via
 * `pureCircuits.digitalPassportCredentialBodyRoot`, then checks the
 * proof via `pureCircuits.assertValidIssuanceContextProof`.
 * Returns `{ valid: true }` on success or `{ valid: false, error }`.
 */
async function verifyDigitalPassportIssuanceProof(params: {
  credentialEncoded: { encoding: string; payload: string };
  proofEncoded: { encoding: string; payload: string };
}): Promise<{ valid: boolean; error?: string; elapsedMs?: number }> {
  const t0 = Date.now();
  try {
    const dpCred = await import(
      "@midnight-ntwrk/midnight-did-credentials-digital-passport"
    );
    const credential = dpCred.decodeDigitalPassportCredential(params.credentialEncoded);
    const proof = dpCred.decodeDigitalPassportProof(params.proofEncoded);
    const { pureCircuits } = dpCred;
    const bodyRoot = pureCircuits.digitalPassportCredentialBodyRoot(credential);
    pureCircuits.assertValidIssuanceContextProof(bodyRoot, proof);
    return { valid: true, elapsedMs: Date.now() - t0 };
  } catch (e) {
    return {
      valid: false,
      error: e instanceof Error ? e.message : String(e),
      elapsedMs: Date.now() - t0,
    };
  }
}

// ───────────────────────────────────────────────────────────────────
// Passport-vault contract compose layer.
//
// Generalises the DID-specific `prepareUnprovenCallTx` above to the
// passport-vault contract so the wallet can build `depositFunds`
// (lock) and `claimFunds` (unlock) call transactions in-WebView, then
// hand the unproven tx to Rust for balance -> prove -> submit (the
// same downstream pipeline `Wallet::call_did_circuit` already uses).
//
// The deposit shielded coin and the claim selective-disclosure
// presentation are built by the caller (Rust / the dApp relay) and
// passed in `circuitArgs`, encoded with `{ $bigint }` /
// `{ $bytes: "<hex>" }` markers (see `reviveVaultValue`).
// ───────────────────────────────────────────────────────────────────

/** Params for `prepareVaultCallTx`. */
export interface PrepareVaultCallTxParams {
  /** Circuit id, e.g. "depositFunds" | "claimFunds" | "adminWithdraw". */
  circuit: string;
  circuitArgs: unknown[];
  contractStateHex: string;
  contractAddressHex: string;
  zswapChainStateHex?: string | null;
  ledgerParametersHex?: string | null;
  coinPublicKeyHex: string;
  encryptionPublicKeyHex: string;
  networkId: string;
  /** Private-state fields the circuit's witnesses read. `claimFunds`
   *  supplies the holder date-of-birth witness; deposit uses the empty
   *  state. Bytes use `{ $bytes }`, bigints `{ $bigint }`. */
  privateState?: Record<string, unknown> | null;
}

export type PrepareVaultCallTxResult = PrepareUnprovenCallTxResult;

/** Revive `{ $bigint: "<dec>" }` and `{ $bytes: "<hex>" }` markers into
 *  JS `bigint` / `Uint8Array`. Generalises `reviveBigints` for the
 *  richer passport-vault arg shapes (shielded coin, credential,
 *  presentation, recipient). */
function reviveVaultValue(value: unknown): unknown {
  if (value === null || typeof value !== "object") return value;
  if (Array.isArray(value)) return value.map(reviveVaultValue);
  const obj = value as Record<string, unknown>;
  if (typeof obj.$bigint === "string") return BigInt(obj.$bigint);
  if (typeof obj.$bytes === "string") return hexToBytes(obj.$bytes);
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(obj)) out[k] = reviveVaultValue(v);
  return out;
}

let vaultContractLayerPromise: Promise<{
  contractMod: Record<string, unknown>;
  witnessesMod: Record<string, unknown>;
  compactRuntime: typeof import("@midnight-ntwrk/compact-runtime");
}> | null = null;

function loadVaultContractLayer() {
  if (!vaultContractLayerPromise) {
    vaultContractLayerPromise = (async () => {
      const [contractMod, witnessesMod, compactRuntime] = await Promise.all([
        import(
          "@input-output-hk/passport-vault-contract/managed/passport-vault/contract/index.js" as string
        ),
        import("@input-output-hk/passport-vault-contract/witnesses.js" as string),
        import("@midnight-ntwrk/compact-runtime"),
      ]);
      return { contractMod, witnessesMod, compactRuntime };
    })();
  }
  return vaultContractLayerPromise;
}

async function prepareVaultCallTx(
  params: PrepareVaultCallTxParams,
): Promise<PrepareVaultCallTxResult> {
  const t0 = Date.now();
  const { contractMod, witnessesMod, compactRuntime: cr } =
    await loadVaultContractLayer();
  const ledgerV8 = await import("@midnight-ntwrk/ledger-v8");
  const compactJs = await import("@midnight-ntwrk/compact-js");

  setNetworkId(
    (params.networkId ?? "undeployed") as Parameters<typeof setNetworkId>[0],
  );

  const compiledContract = (compactJs as any).CompiledContract.make(
    "passport-vault",
    (contractMod as any).Contract,
  ).pipe(
    (compactJs as any).CompiledContract.withWitnesses(
      (witnessesMod as any).witnesses,
    ),
    (compactJs as any).CompiledContract.withCompiledFileAssets("passport-vault"),
  );

  const zkConfigProvider = new WebViewZkConfigProvider(
    pkgBaseUrlFor("passport-vault-contract/dist/managed/passport-vault"),
  );

  const contractState = (cr as any).ContractState.deserialize(
    hexToBytes(params.contractStateHex),
  );
  let zswapChainState: unknown;
  if (params.zswapChainStateHex) {
    zswapChainState = (ledgerV8 as any).ZswapChainState.deserialize(
      hexToBytes(params.zswapChainStateHex),
    );
  } else {
    zswapChainState = new (ledgerV8 as any).ZswapChainState();
  }
  let ledgerParameters: unknown;
  if (params.ledgerParametersHex) {
    ledgerParameters = (ledgerV8 as any).LedgerParameters.deserialize(
      hexToBytes(params.ledgerParametersHex),
    );
  } else {
    ledgerParameters = (ledgerV8 as any).LedgerParameters.initialParameters();
  }

  const args = Array.isArray(params.circuitArgs)
    ? (reviveVaultValue(params.circuitArgs) as unknown[])
    : [];

  // Deposit uses the empty private state; claim supplies the holder
  // date-of-birth witness via `privateState`.
  const initialPrivateState =
    params.privateState != null
      ? (reviveVaultValue(params.privateState) as Record<string, unknown>)
      : (witnessesMod as any).emptyPassportVaultPrivateState();

  let callTxData;
  try {
    callTxData = await (createUnprovenCallTxFromInitialStates as any)(
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
  } catch (e) {
    const err = e instanceof Error ? e : new Error(String(e));
    try {
      await window.midnightWallet.bundleError({
        kind: "prepareVaultCallTxFailed",
        message: `${err.name}: ${err.message} (circuit=${params.circuit}, args=${args.length})`,
        stack: err.stack || "",
      });
    } catch (_) {}
    throw err;
  }

  const unprovenBytes: Uint8Array = callTxData.private.unprovenTx.serialize();
  return {
    circuit: params.circuit,
    unprovenTxHex: bytesToHex(unprovenBytes),
    unprovenTxBytes: unprovenBytes.length,
    elapsedMs: Date.now() - t0,
  };
}

/** Params for `prepareVaultClaim` — the high-level claim compose that
 *  builds the digital-passport presentation in-WebView from a credential
 *  bundle, then composes `claimFunds`. */
export interface PrepareVaultClaimParams {
  /** Which lock to claim from. Decimal string (Uint<64>). */
  lockId: string;
  /** The v3 credential bundle JSON assembled by the wallet from its
   *  stored digital-passport credential (`assemble_credential_bundle`
   *  in `bridge.rs`): `credential` + `credentialProof` (compact-value
   *  encoded) + `privateParts` (claim values + openings). The holder
   *  presentation key is derived in-WebView from the credential, not
   *  carried in the bundle (explicit-DID holder binding). */
  bundle: Record<string, any>;
  contractStateHex: string;
  contractAddressHex: string;
  zswapChainStateHex?: string | null;
  ledgerParametersHex?: string | null;
  coinPublicKeyHex: string;
  encryptionPublicKeyHex: string;
  /** Recipient of the released UNSHIELDED NIGHT: the running wallet's
   *  unshielded `UserAddress` as raw 32-byte payload hex. */
  recipientUserAddressHex: string;
  networkId: string;
  /** Requested claim amount in base units (decimal string). */
  amountBaseUnits: string;
  /** Caller-supplied "current day" (days since epoch). Defaults to today. */
  currentDay?: string;
}

/**
 * Claim compose: this is what a wallet does at claim time. Deserialise
 * the holder's credential bundle, read the vault's on-chain policy,
 * build a digital-passport presentation that discloses exactly what the
 * policy requires + proves the age predicate, sign it with the holder
 * key, then compose the `claimFunds` call. Returns the unproven tx hex
 * for the Rust side to balance/prove/submit. Ports the runner's
 * `presentation.ts` + `credential-bundle.ts` + `claim-funds.ts` logic
 * into the WebView so no browser dApp needs the Compact runtime.
 */
async function prepareVaultClaim(
  params: PrepareVaultClaimParams,
): Promise<PrepareVaultCallTxResult> {
  const t0 = Date.now();
  // NOTE: we deliberately do NOT import the bare
  // `@input-output-hk/passport-vault-contract` entry. Its `index.js`
  // re-exports `fixtures.js` + `simulator.js`, which pull Node built-ins
  // (`node:crypto`, `node:util`, `node:buffer`) the WebView's native ESM
  // loader can't resolve — that surfaces as "Importing a module script
  // failed". The claim only needs the compiled circuits (`contract.js`)
  // and `witnesses.js`; the single fixture helper we use
  // (`signPassportProof`) is inlined below.
  const [pvContractMod, pvWitnessesMod, dp, cr, compactJs, ledgerV8] =
    await Promise.all([
      import("@input-output-hk/passport-vault-contract/contract.js" as string),
      import("@input-output-hk/passport-vault-contract/witnesses.js" as string),
      import("@midnight-ntwrk/midnight-did-credentials-digital-passport"),
      import("@midnight-ntwrk/compact-runtime"),
      import("@midnight-ntwrk/compact-js"),
      import("@midnight-ntwrk/ledger-v8"),
    ]);
  setNetworkId(
    (params.networkId ?? "undeployed") as Parameters<typeof setNetworkId>[0],
  );

  // 1. Deserialise the bundle (port of credential-bundle.ts).
  const b = params.bundle as any;
  if (b?.version !== 3) {
    throw new Error(
      `unsupported credential bundle version ${b?.version}; re-issue with the current runner`,
    );
  }
  const credential = (dp as any).decodeDigitalPassportCredential(b.credential);
  const credentialProof = (dp as any).decodeDigitalPassportProof(b.credentialProof);
  const cv = b.privateParts.claimValues;
  const op = b.privateParts.openings;
  const claimValues = {
    firstNameValuePadded: hexToBytes(cv.firstNameValuePaddedHex),
    lastNameValuePadded: hexToBytes(cv.lastNameValuePaddedHex),
    dateOfBirthDays: BigInt(cv.dateOfBirthDays),
    documentNumberValue: hexToBytes(cv.documentNumberValueHex),
    issuingStateValue: hexToBytes(cv.issuingStateValueHex),
  };
  const openings = {
    firstNameOpening: hexToBytes(op.firstNameOpeningHex),
    lastNameOpening: hexToBytes(op.lastNameOpeningHex),
    dateOfBirthOpening: hexToBytes(op.dateOfBirthOpeningHex),
    documentNumberOpening: hexToBytes(op.documentNumberOpeningHex),
    issuingStateOpening: hexToBytes(op.issuingStateOpeningHex),
  };
  // Holder presentation key. digital-passport uses explicit-DID holder
  // binding (no committed holder key on-chain), so the presentation only
  // needs a self-consistent Schnorr keypair whose signerVerificationMethodRef
  // matches the credential's holder binding — the redeem capability is
  // possession of the credential + openings, not this key. Derive a stable
  // scalar from the credential so the same credential always presents the
  // same key, and pull the ref straight from the decoded credential.
  const JUBJUB_SUBGROUP_ORDER =
    6554484396890773809930967563523245729705921265872317281365359162392183254199n;
  let holderScalar =
    BigInt("0x" + bytesToHex(credential.claimRoot)) % JUBJUB_SUBGROUP_ORDER;
  if (holderScalar === 0n) holderScalar = 1n;
  const holderSigner = {
    label: "holder",
    secretKey: holderScalar,
    publicKey: (cr as any).ecMulGenerator(holderScalar),
    verificationMethodRef: credential.holderBinding.holderVerificationMethodRef,
  };

  // 2. Read the chosen lock's on-chain policy, build the request.
  const lockIdBig = BigInt(params.lockId);
  const contractState = (cr as any).ContractState.deserialize(
    hexToBytes(params.contractStateHex),
  );
  const view = (pvContractMod as any).ledger((contractState as any).data);
  if (!view.locks.member(lockIdBig)) {
    throw new Error(`lock ${params.lockId} does not exist on this vault`);
  }
  const lock = view.locks.lookup(lockIdBig);
  const request = (pvContractMod as any).pureCircuits.passportPolicyRequestFor(
    view.trustedIssuer,
    lock.verifierChallengeHash,
    lock.requireIssuingState,
    lock.requireDocumentNumber,
    lock.minimumAgeYears,
  );

  // 3. Build the selective-disclosure presentation + proof (port of
  //    presentation.ts::buildPassportPresentation).
  const ZERO_32 = new Uint8Array(32);
  const ZERO_64 = new Uint8Array(64);
  const presentation = {
    version: 1n,
    schema: credential.schema,
    credentialClaimRoot: credential.claimRoot,
    issuerVerificationMethodRef: credential.issuerVerificationMethodRef,
    holderBinding: credential.holderBinding,
    disclosed: {
      revealFirstName: request.requireFirstNameDisclosure,
      firstNameValuePadded: request.requireFirstNameDisclosure
        ? claimValues.firstNameValuePadded
        : ZERO_64,
      firstNameOpening: request.requireFirstNameDisclosure
        ? openings.firstNameOpening
        : ZERO_32,
      revealLastName: request.requireLastNameDisclosure,
      lastNameValuePadded: request.requireLastNameDisclosure
        ? claimValues.lastNameValuePadded
        : ZERO_64,
      lastNameOpening: request.requireLastNameDisclosure
        ? openings.lastNameOpening
        : ZERO_32,
      // Follow the lock's policy: only prove the age predicate when the lock
      // actually requires it (minAge > 0 → requireAgeOverThreshold). A zero
      // threshold with the predicate on is rejected by the credential library.
      proveAgeOverThreshold: request.requireAgeOverThreshold,
      ageThresholdYears: request.requestedAgeThresholdYears,
      revealDocumentNumber: request.requireDocumentNumberDisclosure,
      documentNumberValue: request.requireDocumentNumberDisclosure
        ? claimValues.documentNumberValue
        : ZERO_32,
      documentNumberOpening: request.requireDocumentNumberDisclosure
        ? openings.documentNumberOpening
        : ZERO_32,
      revealIssuingState: request.requireIssuingStateDisclosure,
      issuingStateValue: request.requireIssuingStateDisclosure
        ? claimValues.issuingStateValue
        : ZERO_32,
      issuingStateOpening: request.requireIssuingStateDisclosure
        ? openings.issuingStateOpening
        : ZERO_32,
    },
  };
  // Inline port of passport-vault-contract `fixtures.ts::signPassportProof`
  // (presentation context) so the claim path doesn't need the vault
  // package's bare `index.js` (see the import note above). Same raw-Schnorr
  // recipe: build a partial proof with s=0, derive the presentation
  // challenge over the body root, then finalise
  // s = nonce + challenge * holderSecret (mod subgroup order). Both the
  // body root and the challenge use the bundled `dp.pureCircuits`, so they
  // are computed against one consistent digital-passport build.
  const presentationBodyRoot = (dp as any).pureCircuits.digitalPassportPresentationBodyRoot(
    presentation,
  );
  const presentationNonce = 17n;
  const partialPresentationProof = {
    signerVerificationMethodRef: holderSigner.verificationMethodRef,
    createdAt: BigInt(Math.floor(Date.now() / 1000)),
    challengeHash: request.verifierChallengeHash,
    publicKey: holderSigner.publicKey,
    signature: { r: (cr as any).ecMulGenerator(presentationNonce), s: 0n },
  };
  const presentationChallenge = (dp as any).pureCircuits.presentationProofChallenge(
    presentationBodyRoot,
    partialPresentationProof,
  );
  let presentationS =
    (presentationNonce + presentationChallenge * holderSigner.secretKey) %
    JUBJUB_SUBGROUP_ORDER;
  if (presentationS < 0n) presentationS += JUBJUB_SUBGROUP_ORDER;
  const presentationProof = {
    ...partialPresentationProof,
    signature: { r: partialPresentationProof.signature.r, s: presentationS },
  };

  // 4. Assemble claimFunds args + private state, then compose.
  const MS_PER_DAY = 86_400_000;
  const currentDay = params.currentDay
    ? BigInt(params.currentDay)
    : BigInt(Math.floor(Date.now() / MS_PER_DAY));
  const requestedAmount = BigInt(params.amountBaseUnits);
  // claimFunds releases UNSHIELDED NIGHT to a `UserAddress` (32-byte payload),
  // not to a zswap coin key.
  const recipient = { bytes: hexToBytes(params.recipientUserAddressHex) };
  const args = [
    lockIdBig,
    credential,
    credentialProof,
    presentation,
    presentationProof,
    currentDay,
    requestedAmount,
    recipient,
  ];

  const compiledContract = (compactJs as any).CompiledContract.make(
    "passport-vault",
    (pvContractMod as any).Contract,
  ).pipe(
    (compactJs as any).CompiledContract.withWitnesses(
      (pvWitnessesMod as any).witnesses,
    ),
    (compactJs as any).CompiledContract.withCompiledFileAssets("passport-vault"),
  );
  const zkConfigProvider = new WebViewZkConfigProvider(
    pkgBaseUrlFor("passport-vault-contract/dist/managed/passport-vault"),
  );
  let zswapChainState: unknown;
  if (params.zswapChainStateHex) {
    zswapChainState = (ledgerV8 as any).ZswapChainState.deserialize(
      hexToBytes(params.zswapChainStateHex),
    );
  } else {
    zswapChainState = new (ledgerV8 as any).ZswapChainState();
  }
  let ledgerParameters: unknown;
  if (params.ledgerParametersHex) {
    ledgerParameters = (ledgerV8 as any).LedgerParameters.deserialize(
      hexToBytes(params.ledgerParametersHex),
    );
  } else {
    ledgerParameters = (ledgerV8 as any).LedgerParameters.initialParameters();
  }

  const callTxData = await (createUnprovenCallTxFromInitialStates as any)(
    zkConfigProvider,
    {
      compiledContract,
      circuitId: "claimFromLock",
      contractAddress: params.contractAddressHex,
      args,
      coinPublicKey: params.coinPublicKeyHex,
      initialContractState: contractState,
      initialZswapChainState: zswapChainState,
      ledgerParameters,
      initialPrivateState: {
        holderDateOfBirthDays: claimValues.dateOfBirthDays,
        holderDateOfBirthOpening: openings.dateOfBirthOpening,
      },
    },
    params.encryptionPublicKeyHex,
  );
  const unprovenBytes: Uint8Array = callTxData.private.unprovenTx.serialize();
  return {
    circuit: "claimFromLock",
    unprovenTxHex: bytesToHex(unprovenBytes),
    unprovenTxBytes: unprovenBytes.length,
    elapsedMs: Date.now() - t0,
  };
}

/** Decode the passport-vault ledger from a serialised `ContractState`
 *  hex blob. The browser has no Compact runtime, so the wallet fetches
 *  the contract state from the indexer (Rust) and asks the bundle to
 *  decode it via the vendored contract module's `ledger(...)` reader —
 *  the same decoder the runner/integration tests use. */
async function readVaultLedger(params: {
  contractStateHex: string;
}): Promise<{
  totalLockedBaseUnits: string;
  totalDepositedBaseUnits: string;
  hasDeposit: boolean;
}> {
  const { contractMod, compactRuntime: cr } = await loadVaultContractLayer();
  const contractState = (cr as any).ContractState.deserialize(
    hexToBytes(params.contractStateHex),
  );
  // `ledger()` accepts a `StateValue | ChargedState`; `ContractState`
  // exposes the charged state under `.data` (matches the runner +
  // integration-test usage `ledger(state.data)`).
  const led = (contractMod as any).ledger(contractState.data);
  // Unshielded vault: the contract's NIGHT balance lives in the ledger, not a
  // contract field. Locked = totalDeposited − totalReleased (both audit
  // counters maintained by deposit/claim/withdraw).
  const totalDeposited: bigint =
    led.totalDeposited != null ? BigInt(led.totalDeposited) : 0n;
  const totalReleased: bigint =
    led.totalReleased != null ? BigInt(led.totalReleased) : 0n;
  const locked: bigint =
    totalDeposited > totalReleased ? totalDeposited - totalReleased : 0n;
  return {
    totalLockedBaseUnits: locked.toString(),
    totalDepositedBaseUnits: totalDeposited.toString(),
    hasDeposit: locked > 0n,
  };
}

/** Enumerate the multi-lock vault's `locks` map + `lockCount` from a
 *  serialised `ContractState` hex. Drives the dApp's lock list (each
 *  lock's policy + remaining pool) and the claim selector. All numeric
 *  fields are decimal strings; `Bytes<32>` fields are hex. */
async function readVaultLocks(params: {
  contractStateHex: string;
}): Promise<{
  lockCount: string;
  locks: Array<{
    lockId: string;
    lockerHex: string;
    minimumAgeYears: string;
    requireIssuingState: boolean;
    requiredIssuingStateHex: string;
    requireDocumentNumber: boolean;
    requiredDocumentNumberHex: string;
    maxClaimAmount: string;
    verifierChallengeHashHex: string;
    totalDeposited: string;
    totalReleased: string;
    lockedRemaining: string;
  }>;
}> {
  const { contractMod, compactRuntime: cr } = await loadVaultContractLayer();
  const contractState = (cr as any).ContractState.deserialize(
    hexToBytes(params.contractStateHex),
  );
  const led = (contractMod as any).ledger(contractState.data);
  const locks: any[] = [];
  // Reading `locks` / `lockCount` against a contract whose on-chain ledger
  // layout isn't the multi-lock one (e.g. an OLD single-lock passport-vault
  // address) throws a cryptic `asMap()` error. Surface a clear, actionable
  // message instead so the dApp can tell the user to redeploy / repoint.
  try {
  for (const [lockId, rec] of led.locks) {
    const dep = BigInt(rec.totalDeposited ?? 0n);
    const rel = BigInt(rec.totalReleased ?? 0n);
    const remaining = dep > rel ? dep - rel : 0n;
    locks.push({
      lockId: lockId.toString(),
      lockerHex: bytesToHex(rec.locker),
      minimumAgeYears: BigInt(rec.minimumAgeYears).toString(),
      requireIssuingState: !!rec.requireIssuingState,
      requiredIssuingStateHex: bytesToHex(rec.requiredIssuingState),
      requireDocumentNumber: !!rec.requireDocumentNumber,
      requiredDocumentNumberHex: bytesToHex(rec.requiredDocumentNumber),
      maxClaimAmount: BigInt(rec.maxClaimAmount).toString(),
      verifierChallengeHashHex: bytesToHex(rec.verifierChallengeHash),
      totalDeposited: dep.toString(),
      totalReleased: rel.toString(),
      lockedRemaining: remaining.toString(),
    });
  }
  // Sort by lock id ascending for a stable UI order.
  locks.sort((a, b) =>
    BigInt(a.lockId) < BigInt(b.lockId) ? -1 : BigInt(a.lockId) > BigInt(b.lockId) ? 1 : 0,
  );
  const lockCount =
    led.lockCount != null ? BigInt(led.lockCount).toString() : locks.length.toString();
  return { lockCount, locks };
  } catch (e) {
    throw new Error(
      "not a multi-lock passport vault at this address (on-chain ledger layout " +
        "mismatch — likely an old single-lock contract). Deploy the current " +
        "passport-vault and set MIDNIGHT_VAULT_CONTRACT_ADDRESS to the new address. " +
        `(${e instanceof Error ? e.message : String(e)})`,
    );
  }
}

window.midnightDidBundle = {
  version: "0.1.0",
  did: midnightDid,
  didDomain: midnightDidDomain,
  ready: true,
  loadContractLayer,
  bridgeProbe,
  bridgeWitnessTest,
  prepareUnprovenCallTx,
  prepareVaultCallTx,
  prepareVaultClaim,
  readVaultLedger,
  readVaultLocks,
  scanQr,
  pasteText,
  decodeDigitalPassportCredential,
  decodeDigitalPassportProof,
  verifyDigitalPassportIssuanceProof,
};

console.log(
  "[midnight-did bundle] static-loaded",
  "did:",
  Object.keys(midnightDid),
  "domain:",
  Object.keys(midnightDidDomain)
);

// End-to-end smoke: wait for the bridge, ping it, then attempt the
// dynamic contract-layer load. Any failure is reported through the
// `bundleError` RPC so we see it in the Rust log without DevTools.
async function smoke() {
  for (let i = 0; i < 600; i++) {
    if (window.midnightWallet?.ping) break;
    await new Promise((r) => setTimeout(r, 50));
  }
  if (!window.midnightWallet?.ping) {
    console.warn("[smoke] bridge never appeared");
    return;
  }
  try {
    await window.midnightWallet.ping();
    console.log("[smoke] bridge ping ok");
  } catch (e) {
    console.error("[smoke] bridge ping failed", e);
    return;
  }
  try {
    const layer = await loadContractLayer();
    const exported = {
      contract: Object.keys(layer.contract).slice(0, 10),
      compactRuntime: Object.keys(layer.compactRuntime).slice(0, 10),
    };
    console.log("[smoke] contract layer loaded", exported);
    // Surface success through the bridge so the Rust log shows it.
    await window.midnightWallet.bundleError({
      kind: "info",
      message: `contract layer loaded: ${JSON.stringify(exported)}`,
      stack: "",
    });
  } catch (e) {
    const err = e instanceof Error ? e : new Error(String(e));
    console.error("[smoke] contract layer load failed", err);
    try {
      await window.midnightWallet.bundleError({
        kind: "contractLoadFailed",
        message: err.message,
        stack: err.stack || "",
      });
    } catch (_) {}
  }
}

smoke();
