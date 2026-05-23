// Public TypeScript surface for @midnight-ntwrk/react-native-prover.
//
// Mirrors the §13.7 sketch in the midnight-mobile architecture doc.
// Generated JSI Turbo Module bindings live in `./NativeMidnightProver`
// and are compiled by `uniffi-bindgen-react-native` into a thin C++
// layer the host RN app autolinks via the package's Podspec /
// build.gradle.
//
// The Rust core is `crates/prover-ffi/`. Build artefacts:
//   iOS:     ios/MidnightProver.xcframework
//   Android: android/src/main/jniLibs/arm64-v8a/libmidnight_prover_ffi.so
//
// Architecture decision (per `midnight-mobile-architecture.md` §13.5):
// **Option A — UniFFI same-process on BOTH platforms.** No Android
// `:proverProcess` Service. A prover OOM crashes the host RN app on
// both platforms. Documented in the README; the user explicitly opted
// for this simpler shape over the hybrid (`Option B`) one.

import { NativeMidnightProver } from "./NativeMidnightProver";

/**
 * Options for a prove() call. All fields have sensible defaults if
 * omitted (passed-through via TS-side defaults to the FFI dictionary).
 */
export interface ProveOptions {
  /** RNG seed. `0` = use the library default (0x42, for reproducible runs). */
  seed?: number | bigint;
  /** Skip verifier check after proving. Default `true`. */
  verifyAfter?: boolean;
  /**
   * Override the SRS cache directory. Empty / undefined = use the
   * platform default (set by the wallet's `start_app()` / `main()`
   * to `Library/Caches/midnight-pp` on iOS or
   * `/data/data/<APP_ID>/cache/midnight-pp` on Android).
   */
  cacheDir?: string;
  /** Reuse (ProverKey, VerifierKey) pairs across proves. Default `true`. */
  cacheKeys?: boolean;
}

/**
 * Outcome of a single prove call. Durations are in milliseconds.
 */
export interface ProveResult {
  k: number;
  realizedK: number;
  hashChainLen: number;
  rows: bigint;
  keygenMs: bigint;
  proveMs: bigint;
  /** `null` when `k > 14` (no embedded verifier) or `verifyAfter=false`. */
  verifyMs: bigint | null;
  /** Whether the verification succeeded. `null` if not attempted. */
  verified: boolean | null;
  proofBytes: bigint;
}

/**
 * Error codes emitted by the prover. Maps to `ProverError` variants
 * in the Rust crate.
 */
export type ProverErrorCode =
  | "OutOfRange"
  | "ParamsUnavailable"
  | "KeygenFailed"
  | "ProveFailed"
  | "VerifyFailed"
  | "Io"
  | "Internal";

/**
 * Run a single bench prove at the requested `k`. Synchronous in Rust;
 * the platform layer wraps it in a Promise so the JS thread never
 * blocks during the multi-minute prove at high `k`.
 *
 * @param k log-domain-size of the circuit (1..=21)
 * @param opts optional knobs (see {@link ProveOptions})
 * @returns timings + proof metadata; the proof bytes themselves are
 *          available via a follow-up `getProofBytes()` if needed —
 *          omitted from the result to keep the FFI payload small for
 *          the common "I only need the timing" case (the benchmark
 *          screen). Pass `cacheKeys: false` if you want to time a
 *          cold keygen run.
 */
export async function prove(
  k: number,
  opts: ProveOptions = {},
): Promise<ProveResult> {
  // Default-fill the options dictionary so the FFI side gets a fully-
  // populated struct (UniFFI dictionaries don't support optional fields
  // in the same way TS does).
  const filled = {
    seed: BigInt(opts.seed ?? 0),
    verifyAfter: opts.verifyAfter ?? true,
    cacheDir: opts.cacheDir ?? "",
    cacheKeys: opts.cacheKeys ?? true,
  };
  // The native side is sync but we wrap in Promise.resolve so the JS
  // thread never blocks while the prover grinds. The platform shim
  // (Swift Combine future / Kotlin coroutine) handles the actual
  // threading off the JS bridge.
  return await NativeMidnightProver.prove(k, filled);
}

/**
 * SemVer + crate-name string identifying which Rust core this RN
 * package is bound to. Useful for matching native dylib version
 * against the npm package version in support logs.
 */
export function libraryVersion(): string {
  return NativeMidnightProver.libraryVersion();
}

// Re-export the type guard for downstream telemetry consumers.
export function isProverError(e: unknown): e is { code: ProverErrorCode; message: string } {
  return (
    typeof e === "object" &&
    e !== null &&
    "code" in e &&
    typeof (e as { code: unknown }).code === "string"
  );
}
