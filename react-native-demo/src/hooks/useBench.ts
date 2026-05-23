/**
 * useBench — state machine for the Benchmark screen.
 *
 * Manages a per-k row state + the "Run all" sweep coordination.
 * Calls into @midnight-ntwrk/react-native-prover for the actual
 * prove. Maps the FFI errors back into the `BenchOutcome` shape
 * the UI consumes.
 */

import { useCallback, useReducer, useRef } from "react";
import { proveAsync, isProverError } from "@midnight-ntwrk/react-native-prover";

import {
  MAX_K,
  MIN_K,
  emptyBenchRows,
  type BenchRow,
} from "../types/bench";

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

  const runOne = useCallback(async (k: number) => {
    if (sweepInFlightRef.current) return;
    if (k < MIN_K || k > MAX_K) return;

    const startedAtMs = Date.now();
    dispatch({ type: "start", k, startedAtMs });
    try {
      const result = await proveAsync(k, {
        // The default seed (0x42) is fine for the bench tab —
        // the goal is reproducible timings, not nonce uniqueness.
        seed: 0,
        // Verifier params only cover k ≤ 14; the FFI silently
        // skips verify at higher k regardless of this flag.
        verifyAfter: true,
        cacheKeys: true,
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

  const runAll = useCallback(
    async (upTo: number) => {
      if (sweepInFlightRef.current) return;
      sweepInFlightRef.current = true;
      try {
        for (let k = MIN_K; k <= upTo; k++) {
          // Run sequentially — every prove pegs CPU + memory at
          // high k, so parallelising would just thrash. This
          // matches the Dioxus wallet's behaviour.
          await runOne(k);
        }
      } finally {
        sweepInFlightRef.current = false;
      }
    },
    [runOne],
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
