/**
 * useDid — state for the DID CRUD screen.
 *
 * **Status: DID resolve + writes call the FFI, which returns
 * `WalletError::NotImplemented` until the upstream-TS
 * `prepareUnprovenCallTx` bridge is ported to RN's Hermes.**
 * The previous stubbed fake-responses are gone — calls now hit
 * the real FFI surface and surface the structured
 * NotImplemented error, so the UI accurately reflects which
 * flows are wired vs which are pending.
 *
 * Integration plan reminder (architecture doc §14.2):
 *   (a) Embed the upstream TS+WASM bundle in Hermes via a
 *       JS-side WASM shim or a custom JSI host-object.
 *   (b) Port the contract layer to Rust + UniFFI alongside
 *       the prover.
 */

import { useCallback, useState } from "react";
import {
  didResolve as ffiDidResolve,
  didDeploy as ffiDidDeploy,
  didUpdateAka as ffiDidUpdateAka,
  didDeactivate as ffiDidDeactivate,
} from "@midnight-ntwrk/react-native-prover";

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
    setState((s) => ({
      ...s,
      inFlight: { kind: "resolve", startedAtMs: Date.now() },
      lastResult: null,
    }));
    const start = Date.now();
    try {
      // FFI is sync; wrap in microtask to yield.
      await Promise.resolve();
      const json = ffiDidResolve("preprod", did);
      const doc = JSON.parse(json) as DidDocument;
      const elapsedMs = Date.now() - start;
      setState({
        resolved: doc,
        inFlight: null,
        lastResult: { kind: "resolve", ok: true, did, elapsedMs },
      });
    } catch (e) {
      const elapsedMs = Date.now() - start;
      setState({
        resolved: null,
        inFlight: null,
        lastResult: {
          kind: "resolve",
          ok: false,
          did,
          elapsedMs,
          error: errorMessage(e),
        },
      });
    }
  }, []);

  const deploy = useCallback(async () => {
    setState((s) => ({
      ...s,
      inFlight: { kind: "deploy", startedAtMs: Date.now() },
      lastResult: null,
    }));
    const start = Date.now();
    try {
      await Promise.resolve();
      // This will throw WalletError::NotImplemented until the
      // upstream-TS prepareUnprovenCallTx bridge is wired up.
      // Surface the structured error to the UI.
      ffiDidDeploy(undefined as never, "demo-did");
    } catch (e) {
      const elapsedMs = Date.now() - start;
      setState((s) => ({
        ...s,
        inFlight: null,
        lastResult: {
          kind: "deploy",
          ok: false,
          elapsedMs,
          error: errorMessage(e),
        },
      }));
    }
  }, []);

  const update = useCallback(
    async (newAlsoKnownAs: string) => {
      if (!state.resolved) return;
      setState((s) => ({
        ...s,
        inFlight: { kind: "update", startedAtMs: Date.now() },
        lastResult: null,
      }));
      const start = Date.now();
      try {
        await Promise.resolve();
        ffiDidUpdateAka(
          undefined as never,
          state.resolved.did,
          newAlsoKnownAs,
        );
      } catch (e) {
        const elapsedMs = Date.now() - start;
        setState((s) => ({
          ...s,
          inFlight: null,
          lastResult: {
            kind: "update",
            ok: false,
            did: state.resolved?.did,
            elapsedMs,
            error: errorMessage(e),
          },
        }));
      }
    },
    [state.resolved],
  );

  const deactivate = useCallback(async () => {
    if (!state.resolved) return;
    setState((s) => ({
      ...s,
      inFlight: { kind: "deactivate", startedAtMs: Date.now() },
      lastResult: null,
    }));
    const start = Date.now();
    try {
      await Promise.resolve();
      ffiDidDeactivate(undefined as never, state.resolved.did);
    } catch (e) {
      const elapsedMs = Date.now() - start;
      setState((s) => ({
        ...s,
        inFlight: null,
        lastResult: {
          kind: "deactivate",
          ok: false,
          did: state.resolved?.did,
          elapsedMs,
          error: errorMessage(e),
        },
      }));
    }
  }, [state.resolved]);

  const clear = useCallback(() => setState(INITIAL), []);

  return { ...state, resolve, deploy, update, deactivate, clear };
}

function errorMessage(e: unknown): string {
  if (e instanceof Error) return e.message;
  if (typeof e === "object" && e !== null) {
    const eo = e as { code?: string; message?: string };
    if (eo.code && eo.message) return `[${eo.code}] ${eo.message}`;
    if (eo.message) return eo.message;
  }
  return String(e);
}
