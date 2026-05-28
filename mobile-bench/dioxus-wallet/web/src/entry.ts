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
      /** WebView-based QR scanner. Opens a full-viewport overlay with
       *  a back-facing camera preview, runs jsQR on every animation
       *  frame, resolves with `{ url }` on the first decode. Cancel
       *  button resolves with `{ error: "cancelled" }`; permission
       *  denial / no camera resolves with `{ error: "<reason>" }`.
       *  Idempotent — concurrent calls return `{ error: "busy" }`. */
      scanQr(params?: Record<string, unknown>):
        Promise<{ url?: string; error?: string }>;
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
  // `~/iohk/midnight-did/packages/contract/src/witnesses.ts`.
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

window.midnightDidBundle = {
  version: "0.1.0",
  did: midnightDid,
  didDomain: midnightDidDomain,
  ready: true,
  loadContractLayer,
  bridgeProbe,
  bridgeWitnessTest,
  prepareUnprovenCallTx,
  scanQr,
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
