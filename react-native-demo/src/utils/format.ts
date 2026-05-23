/**
 * Display helpers ported from `mobile-bench/dioxus-wallet/src/
 * format.rs`. Keep the formatting identical between the Dioxus
 * and RN wallets so screenshots are directly comparable.
 */

/** Format milliseconds as either "Nms" (<1s) or "N.NNs". */
export function formatMs(ms: bigint | number | null | undefined): string {
  if (ms === null || ms === undefined) return "—";
  const n = typeof ms === "bigint" ? Number(ms) : ms;
  if (n < 1_000) return `${n}ms`;
  if (n < 60_000) return `${(n / 1_000).toFixed(2)}s`;
  const minutes = Math.floor(n / 60_000);
  const seconds = Math.floor((n % 60_000) / 1_000);
  return `${minutes}m ${String(seconds).padStart(2, "0")}s`;
}

/** Render a byte count, e.g. "2933 B" or "1.5 KiB". */
export function formatBytes(b: bigint | number | null | undefined): string {
  if (b === null || b === undefined) return "—";
  const n = typeof b === "bigint" ? Number(b) : b;
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KiB`;
  return `${(n / 1024 / 1024).toFixed(1)} MiB`;
}
