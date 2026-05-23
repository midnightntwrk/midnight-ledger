/**
 * Bench-screen state types. Mirrors the Dioxus wallet's
 * `BenchOutcome` / `BenchRow` shapes from `mobile-bench/dioxus-
 * wallet/src/app.rs` so the RN port reads like a 1:1 translation.
 */

import type { ProveResult, ProverErrorCode } from "@midnight-ntwrk/react-native-prover";

export const MIN_K = 1;
export const MAX_K = 21;
export const MAX_VERIFIABLE_K = 14;

/** Hash-chain length per `k`, copied from `contract-benchmark`'s
 * `HASHES_FOR_K` table. The TS side displays it but the actual
 * value comes back from the Rust prover (which re-computes it).
 */
export const HASHES_FOR_K: ReadonlyArray<number> = [
  0, 0, 1, 1, 1, 1, 2, 3, 6, 12, 24, 49, 98, 195, 390, 780, 1560, 3121, 6242, 12484, 24967, 49935,
];

export type BenchOutcome =
  | { kind: "idle" }
  | { kind: "running"; startedAtMs: number }
  | { kind: "ok"; result: ProveResult }
  | { kind: "error"; code: ProverErrorCode | "Unknown"; message: string };

export interface BenchRow {
  k: number;
  outcome: BenchOutcome;
}

export function emptyBenchRows(): BenchRow[] {
  const rows: BenchRow[] = [];
  for (let k = MIN_K; k <= MAX_K; k++) {
    rows.push({ k, outcome: { kind: "idle" } });
  }
  return rows;
}
