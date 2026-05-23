/**
 * useDid — state for the DID CRUD screen.
 *
 * **Status: contract calls are stubbed.** The Dioxus wallet's DID
 * tab depends on the upstream TS contract layer
 * (`@midnight-ntwrk/midnight-did-contract`, `@midnight-ntwrk/
 * compact-runtime`, the `onchain-runtime-v3` + `ledger-v8` WASM
 * blobs) running in a WebView. Porting that to React Native means
 * either:
 *
 *   (a) Running the same TS+WASM bundle inside RN's Hermes engine
 *       — non-trivial because Hermes lacks WASM support today;
 *       requires a JS-side WASM shim (e.g. `@iden3/wasm-shim`,
 *       `wabt`-via-JS) or a custom JSI host-object that exposes
 *       the WASM exports as plain JS values.
 *
 *   (b) Porting the contract layer to Rust and exposing it via
 *       UniFFI alongside the prover. Cleaner but a multi-month
 *       project (the contract layer is the bulk of the wallet's
 *       intelligence — keygen-time circuit assembly, balance
 *       arithmetic, indexer synchronisation, etc.).
 *
 * (a) is the cheaper path for a "make it work as an RN demo"
 * goal; (b) is the right path for production. Picking between
 * them is its own design phase.
 *
 * For now this hook:
 *   1. Renders the UI shapes the user will need.
 *   2. Calls deterministic stub implementations that return
 *      fake-but-realistic responses after a delay.
 *   3. Clearly labels every stub with a TODO pointing at the
 *      integration plan.
 */

import { useCallback, useState } from "react";

import type {
  DidDocument,
  DidOpInFlight,
  DidOpResult,
} from "../types/did";

interface State {
  resolved: DidDocument | null;
  inFlight: DidOpInFlight | null;
  lastResult: DidOpResult | null;
}

const INITIAL: State = {
  resolved: null,
  inFlight: null,
  lastResult: null,
};

export function useDid() {
  const [state, setState] = useState<State>(INITIAL);

  const resolve = useCallback(async (did: string) => {
    // TODO(integration): replace with a call to the upstream TS
    // contract bridge's `resolve_did_full` (see Dioxus wallet
    // `mobile-bench/dioxus-wallet/src/app.rs::ChainOps::resolve_did`).
    setState((s) => ({
      ...s,
      inFlight: { kind: "resolve", startedAtMs: Date.now() },
      lastResult: null,
    }));
    const start = Date.now();
    await delay(1_200);
    const stub: DidDocument = {
      did,
      publicKey: "0xPLACEHOLDER_PUBKEY_HEX_ED25519_32_BYTES",
      alsoKnownAs: ["https://example.org/profile/alice"],
      services: [],
      deactivated: false,
      lastModifiedBlock: 12345678,
    };
    const elapsedMs = Date.now() - start;
    setState({
      resolved: stub,
      inFlight: null,
      lastResult: { kind: "resolve", ok: true, did, elapsedMs },
    });
  }, []);

  const deploy = useCallback(async () => {
    // TODO(integration): the wallet generates an Ed25519 seed,
    // constructs the deploy UnprovenTransaction via the upstream
    // TS bridge, then proves it via `@midnight-ntwrk/react-native-
    // prover`'s `prove()`. The prove+submit step is what the prover
    // package is designed for; the deploy tx-construction is what
    // currently lacks an RN-side bridge.
    setState((s) => ({
      ...s,
      inFlight: {
        kind: "deploy",
        startedAtMs: Date.now(),
        status: "constructing unproven tx (stub)",
      },
      lastResult: null,
    }));
    const start = Date.now();
    await delay(2_000);
    const fakeAddress = `did:midnight:${randHex(40)}`;
    const elapsedMs = Date.now() - start;
    setState({
      resolved: {
        did: fakeAddress,
        publicKey: "0xPLACEHOLDER_PUBKEY_HEX_ED25519_32_BYTES",
        alsoKnownAs: [],
        services: [],
        deactivated: false,
        lastModifiedBlock: 0,
      },
      inFlight: null,
      lastResult: {
        kind: "deploy",
        ok: true,
        did: fakeAddress,
        elapsedMs,
      },
    });
  }, []);

  const update = useCallback(
    async (newAlsoKnownAs: string) => {
      // TODO(integration): port the addAlsoKnownAs flow from the
      // Dioxus wallet. The proof generation step is wired (calls
      // into the RN prover); the unproven-tx construction is stub.
      if (!state.resolved) return;
      setState((s) => ({
        ...s,
        inFlight: {
          kind: "update",
          startedAtMs: Date.now(),
          status: "adding alsoKnownAs (stub)",
        },
        lastResult: null,
      }));
      const start = Date.now();
      await delay(1_500);
      const elapsedMs = Date.now() - start;
      setState((prev) => ({
        resolved: prev.resolved
          ? {
              ...prev.resolved,
              alsoKnownAs: [...prev.resolved.alsoKnownAs, newAlsoKnownAs],
              lastModifiedBlock: prev.resolved.lastModifiedBlock + 1,
            }
          : null,
        inFlight: null,
        lastResult: {
          kind: "update",
          ok: true,
          did: prev.resolved?.did,
          elapsedMs,
        },
      }));
    },
    [state.resolved],
  );

  const deactivate = useCallback(async () => {
    // TODO(integration): port the deactivate flow. Same shape as
    // update — bridge to contract layer + prove + submit.
    if (!state.resolved) return;
    setState((s) => ({
      ...s,
      inFlight: {
        kind: "deactivate",
        startedAtMs: Date.now(),
        status: "deactivating (stub)",
      },
      lastResult: null,
    }));
    const start = Date.now();
    await delay(1_500);
    const elapsedMs = Date.now() - start;
    setState((prev) => ({
      resolved: prev.resolved
        ? { ...prev.resolved, deactivated: true }
        : null,
      inFlight: null,
      lastResult: {
        kind: "deactivate",
        ok: true,
        did: prev.resolved?.did,
        elapsedMs,
      },
    }));
  }, [state.resolved]);

  const clear = useCallback(() => setState(INITIAL), []);

  return { ...state, resolve, deploy, update, deactivate, clear };
}

function delay(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

function randHex(len: number): string {
  const chars = "0123456789abcdef";
  let s = "";
  for (let i = 0; i < len; i++) s += chars[Math.floor(Math.random() * 16)];
  return s;
}
