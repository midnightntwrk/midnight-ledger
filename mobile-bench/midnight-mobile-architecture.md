# Midnight Mobile Wallet — Documentation index

This file used to contain the full design + implementation reference
in a single ~4 500-line document. Concerns grew interleaved (high-level
architecture, opt landing details, RN packaging, raw benchmark tables)
and the file became hard to read for any single audience.

**The content has been split into four focused docs.** Pick the one
that matches what you need.

## [`architecture.md`](./architecture.md) — design + integration reference

How the mobile-bench Dioxus wallet hangs together end-to-end. The
Rust ↔ TS ↔ Rust bridge, the `mn-pkg://` WebView asset pipeline, the
proof-generation flow, Android / iOS host integration, the build
matrix, the sizing context (what `k` means in production ZK), and the
open questions / known gaps list.

→ Start here if you're new to the codebase.

Maps to old sections: §1-§6 + §5b + §8 + §12.

## [`optimization-phases.md`](./optimization-phases.md) — opt landing log

Every memory- and prove-time optimisation we landed during the
mobile-bench effort, in chronological landing order, with the
marginal delta each one contributed. Includes the proposed
prebuilt-PK-on-disk follow-up, the loose-end fix list, and the
PR map.

→ Start here if you want to understand *why* each patch exists
or judge whether a particular optimisation is worth porting
upstream.

Maps to old sections: §10.2, §10.4-§10.7, §10.9-§10.11.

## [`react-native-adoption.md`](./react-native-adoption.md) — RN packaging + wasm contrast

The packaging options for moving the Rust prover into a React Native
host (Option A / B / D comparison, recommended Option B), the
detailed embedding instructions for downstream RN-wallet teams, the
implemented Option A scaffold (`@midnight-ntwrk/react-native-prover`
+ `react-native-demo`), the integration landmines hit while wiring
end-to-end, and a self-contained section on why the same
optimisations cannot ship to web wasm.

→ Start here if you're integrating the prover into an RN app,
evaluating wasm as an alternative, or debugging an
"TurboModuleRegistry.getEnforcing" / aws-lc-rs SIGSEGV class
of issue.

Maps to old sections: §7, §11, §13, §14.

## [`benchmark.md`](./benchmark.md) — measured prove timings

Raw per-k sweep results in chronological order (slowest → final),
the 30-second headline (k = 20 on S24 Ultra in 3 m 29 s, 4 393 MiB
peak HWM), the k = 10..20 before/after comparison table for both
CPU and memory, the per-phase k = 20 trace, and the k = 21 ceiling
test that found the next architectural unlock (row-streaming
`evaluate_h`).

→ Start here if you want to know "how fast is it" or "how much RAM
does k = N need".

Maps to old sections: §9, §10.1, §10.1.1, §10.3, §10.8.

---

The full original file is preserved at `/tmp/midnight-mobile-architecture.md.bak`
on the machine where the split was performed. Future edits should target
one of the four files above, not this index. If a topic genuinely doesn't
fit any of them, that's a signal we need a fifth doc — open a discussion
before adding content back here.
