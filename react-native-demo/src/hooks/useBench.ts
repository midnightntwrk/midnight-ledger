/**
 * useBench — state machine for the Benchmark screen.
 *
 * Manages a per-k row state + the "Run all" sweep coordination.
 * Calls into @midnight-ntwrk/react-native-prover for the actual
 * prove. Maps the FFI errors back into the `BenchOutcome` shape
 * the UI consumes.
 */

import { useCallback, useReducer, useRef } from "react";
import { Platform } from "react-native";
import { proveAsync, isProverError } from "@midnight-ntwrk/react-native-prover";

import {
  MAX_K,
  MIN_K,
  emptyBenchRows,
  type BenchRow,
} from "../types/bench";

// Android apps run in a sandboxed env without $HOME / $XDG_CACHE_HOME /
// $MIDNIGHT_PP, so the prover's default cache-dir lookup fails immediately
// with "Could not determine $HOME, $XDG_CACHE_HOME, or $MIDNIGHT_PP". Point
// it at the app's internal files dir (writable, persisted across launches).
// iOS Simulator inherits $HOME from the host shell so empty-string =>
// default works there. Real iPhone will need similar plumbing once we test
// on-device — likely via a native init call that resolves
// NSCachesDirectory + reflects it back into MIDNIGHT_PP.
const CACHE_DIR =
  Platform.OS === "android"
    ? "/data/data/com.midnightdemoapp/files/midnight-pp"
    : "";

type Action =
  | { type: "start"; k: number; startedAtMs: number }
  | { type: "done"; k: number; row: BenchRow }
  | { type: "reset" };

interface State {
  rows: BenchRow[];
  /** k currently being processed, if any (running state). */
  runningK: number | null;
  /** Last `Run all` upper bound the user entered; persisted across renders. */
  maxK: number;
}

function reducer(state: State, action: Action): State {
  switch (action.type) {
    case "start": {
      const rows = state.rows.map((r) =>
        r.k === action.k
          ? { ...r, outcome: { kind: "running" as const, startedAtMs: action.startedAtMs } }
          : r,
      );
      return { ...state, rows, runningK: action.k };
    }
    case "done": {
      const rows = state.rows.map((r) => (r.k === action.k ? action.row : r));
      return { ...state, rows, runningK: null };
    }
    case "reset":
      return { ...state, rows: emptyBenchRows(), runningK: null };
    default:
      return state;
  }
}

export function useBench(initialMaxK: number = 14) {
  const [state, dispatch] = useReducer(reducer, undefined as unknown as State, () => ({
    rows: emptyBenchRows(),
    runningK: null,
    maxK: initialMaxK,
  }));

  // Tracks whether a "Run all" sweep is in progress so the per-row
  // Run buttons can disable themselves to avoid double-launch.
  const sweepInFlightRef = useRef(false);

  // Internal — performs one prove with no guards. Used by both
  // the per-row Run button (via `runOne`) and the Run-all sweep
  // (via `runAll`). The earlier version of `runOne` short-
  // circuited when `sweepInFlightRef.current === true`, which
  // meant the sweep — which itself sets that flag — would always
  // skip every iteration immediately. Splitting Internal from
  // the public guard fixes the bug.
  const runOneInternal = useCallback(async (k: number) => {
    if (k < MIN_K || k > MAX_K) return;
    const startedAtMs = Date.now();
    dispatch({ type: "start", k, startedAtMs });
    // Force at least one real timer tick so the "Running" state can
    // paint before proveSync re-blocks the JS thread. `Promise.resolve()`
    // alone is a microtask, and React's renderer doesn't run between
    // microtasks — without this setTimeout the row goes straight from
    // "Run" to a final number with no spinner ever visible.
    await new Promise<void>((resolve) => setTimeout(resolve, 16));
    try {
      const result = await proveAsync(k, {
        seed: 0,
        verifyAfter: true,
        cacheKeys: true,
        cacheDir: CACHE_DIR,
      });
      dispatch({
        type: "done",
        k,
        row: { k, outcome: { kind: "ok", result } },
      });
    } catch (e) {
      const { code, message } = parseError(e);
      dispatch({
        type: "done",
        k,
        row: { k, outcome: { kind: "error", code, message } },
      });
    }
  }, []);

  // Public — per-row Run button. Refuses to overlap with a sweep.
  const runOne = useCallback(
    async (k: number) => {
      if (sweepInFlightRef.current) return;
      await runOneInternal(k);
    },
    [runOneInternal],
  );

  const runAll = useCallback(
    async (upTo: number) => {
      if (sweepInFlightRef.current) return;
      sweepInFlightRef.current = true;
      try {
        for (let k = MIN_K; k <= upTo; k++) {
          // Run sequentially — every prove pegs CPU + memory at
          // high k, so parallelising would just thrash.
          await runOneInternal(k);
        }
      } finally {
        sweepInFlightRef.current = false;
      }
    },
    [runOneInternal],
  );

  const reset = useCallback(() => dispatch({ type: "reset" }), []);

  return {
    rows: state.rows,
    runningK: state.runningK,
    isSweeping: sweepInFlightRef.current,
    runOne,
    runAll,
    reset,
  };
}

function parseError(e: unknown): { code: import("@midnight-ntwrk/react-native-prover").ProverErrorCode | "Unknown"; message: string } {
  if (isProverError(e)) {
    return { code: e.code, message: e.message };
  }
  if (e instanceof Error) {
    return { code: "Unknown", message: e.message };
  }
  return { code: "Unknown", message: String(e) };
}
