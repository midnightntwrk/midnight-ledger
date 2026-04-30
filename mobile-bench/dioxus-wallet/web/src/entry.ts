// Bundle entry. Static imports here are resolved by esbuild at
// build time. Dynamic imports of `@midnight-ntwrk/...` resolve at
// runtime through the import map → `mn-pkg://` custom protocol →
// vendored `assets/web/pkg/<name>/...`.

import * as midnightDid from "@midnight-ntwrk/midnight-did";
import * as midnightDidDomain from "@midnight-ntwrk/midnight-did-domain";

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
    };
    MIDNIGHT_PROOF_SERVER?: string;
    MIDNIGHT_NETWORK?: string;
  }
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

window.midnightDidBundle = {
  version: "0.1.0",
  did: midnightDid,
  didDomain: midnightDidDomain,
  ready: true,
  loadContractLayer,
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
