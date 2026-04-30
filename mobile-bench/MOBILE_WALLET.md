# Mobile wallet — plan, use cases, and dark theme spec

Drafted 2026-04-29. Status: **plan**. Not started.

This document is the mobile-focused companion to
[`WALLET_PLAN.md`](WALLET_PLAN.md). It pins down the four flows the
user just signed off on, the screens that carry them, and a
minimalistic dark theme to apply consistently. Implementation lands in
the same `dioxus-wallet` crate (already cross-platform — `cdylib` on
Android, `bin` on desktop) — there is no separate "mobile" crate.

## Goals

- **Phone-first layout** that still renders well on desktop (single
  column ≤ 480 px, comfortable touch targets).
- **Demo flow loop**: launch → wallet present → pick network →
  connect → wallet address visible → balances visible. End to end in
  ≤ 4 taps.
- **Minimalistic dark UI** — small, deliberate palette; no
  decoration; type carries the hierarchy.
- **No mock data**. Every value on screen is either pulled from
  `wallet-core` or rendered as an explicit "—" placeholder when the
  source is unavailable.

## Non-goals (this slice)

- Multi-wallet management UI — single demo wallet only.
- Sending transactions — that's `WALLET_PLAN.md` iter-1 step-3.
- Real persistence — sled lands in iter-2.
- iOS — Dioxus 0.6 iOS support is rough; not targeted yet.
- Bottom-sheet, modal, drawer animation flourishes — keep the iter-1
  surface flat. Add motion later if it adds clarity.

## Use cases

Each case lists the **trigger**, the **outcome the user should
observe**, the **wallet-core surface used** (existing or to-add), and
**edge cases** we explicitly handle this slice.

### UC-1 — Wallet exists on first launch

**Trigger.** App is opened.
**Outcome.** A wallet card is already visible. The user does not see
"empty state / generate seed" — the demo wallet is loaded from
`Wallet::demo()` (hardcoded `DEMO_SEED_HEX`). Card shows: network
(default `PreProd`), short address, "Wallet ready" subtitle.
**Surface.** `wallet_core::Wallet::demo(network)` (exists).
**Edge cases.**
- Network change before connect: re-derive demo against the new
  network so the displayed address stays consistent with what the
  user will top up. *(Already wired in step-2 UI.)*

### UC-2 — Connect + sync indexer state

**Trigger.** User taps **Connect** on the wallet screen.
**Outcome.** Sequenced status updates in a single
"Sync" pill: `Probing → Connecting → Syncing X% → Synced (height N)`.
On `Synced`, the address card un-greys and the balance row populates.
On any failure, pill shows `Stalled` with a one-line reason; tap
retries.
**Surface.** New methods on `wallet-core`:
- `IndexerClient::chain_tip()` — already shipped (uses for the
  "height N" badge).
- `IndexerClient::subscribe_unshielded(address)` — *to add* — opens
  the `graphql-transport-ws` subscription `unshieldedTransactions`,
  yields events as `UnshieldedTxEvent`. Reuses
  `indexer-tests/src/graphql_ws_client.rs` from midnight-indexer.
- `IndexerClient::subscribe_dust_ledger()` — *to add* — same
  `graphql-transport-ws`, subscribes to `dustLedgerEvents`.
- `Wallet::start_sync(network) → SyncHandle` — *to add* — owns the
  two subscriptions, plus a tokio task that folds events into an
  in-memory `Balances` struct. Exposes `tokio::sync::watch::Receiver`
  for UI.
- The TS reference the user is sharing will inform the **fold logic**
  (which event fields go into NIGHT vs. Dust totals, how dust
  generation is progressively applied). *Open question — see end.*
**Edge cases.**
- Probe fails → skip queries; show `Stalled — endpoint unreachable`.
- WS subscription drops mid-sync → reconnect with backoff (1s, 2s,
  5s, then 5s every retry); pill returns to `Connecting…`.
- Network changed during a sync → cancel handle, restart.

### UC-3 — Display the wallet address (for top-up)

**Trigger.** Always visible after UC-1; tappable card.
**Outcome.** Address row shows the **unshielded address** in
abbreviated form (`mn1q9p…f9zg`). Tap reveals a full-width modal-less
panel with: full address (mono), copy-to-clipboard button, QR
placeholder text ("QR rendering — iter-2"). On mobile the panel slides
in below the wallet card; on desktop it renders inline.
**Why unshielded.** That's where freshly-faucet'd NIGHT lands —
matches gsd-wallet's "Receive (Unshielded)" tab. Shielded address
lives in a secondary "Advanced" disclosure (collapsed by default) so
the demo flow stays one screen.
**Surface.** `Wallet::unshielded_address(network) -> String` — *to
add*, returns bech32m string (`mn_addr_test1…` on preprod /
preview / qanet / devnet, `mn_addr1…` on mainnet,
`mn_addr_undeployed1…` on undeployed). Implementation: derive the
NIGHT external public key via BIP32 path
`m/44'/2400'/0'/0/0` (role `NightExternal=0`), then bech32m-encode
with HRP `mn_addr` + the per-network suffix. Source for both:
`midnight-wallet/packages/{hd,address-format}`.
**Edge cases.**
- Address derivation fails → row reads `Address unavailable —
  <reason>` in an error tone. Don't crash the screen.
- Long address on phone → use CSS `font-variant-numeric: tabular-nums`
  + fixed-width middle ellipsis; full value always available via Copy.

### UC-4 — Display NIGHT and Dust balances

**Trigger.** UC-2 completes successfully.
**Outcome.** Two balance rows below the address card:
```
NIGHT                                 1,234.567 890
Dust                                       42.000 spk
```
- NIGHT shown in stars formatted as decimal NIGHT
  (`1 NIGHT = 10⁶ stars`). Three-digit grouping; tabular nums.
- Dust shown in specks formatted as decimal DUST
  (`1 DUST = 10¹⁵ specks`). Same formatting.
- Pending events reflected as a `+x.x` overlay until indexer
  confirms.
**Surface.** `wallet_core::balance::Balances`:
```rust
struct Balances {
    night_stars: u128,
    night_pending_stars: i128,   // signed: in-flight outgoing
    dust_specks: u128,
    dust_pending_specks: i128,
    last_synced_block: u64,
}
```
Folded by the sync task. UI subscribes via `watch::Receiver<Balances>`.
**Edge cases.**
- Sync still in progress → show running totals + a tiny `syncing` dot
  next to each row.
- Wallet has zero balance → show `0` in muted tone + a hint "send
  test NIGHT to the address above to see funds appear".
- Numbers exceed display width → truncate decimals, show full value
  on tap.

## Information architecture

One screen for iter-1 mobile. Vertical stack from top to bottom:

```
┌───────────────────────────────────────┐
│  Midnight Wallet              [≡]     │  ← header
│  PreProd · synced · block 557 902     │  ← status sub-line
├───────────────────────────────────────┤
│  ┌─────────────────────────────────┐  │
│  │ Address (NIGHT receive)         │  │
│  │ mn_addr_test1qx…f9zghfd2 [⧉]    │  │  ← address pill
│  └─────────────────────────────────┘  │
│                                       │
│  ┌─────────────────────────────────┐  │
│  │ NIGHT             1,234.567 890 │  │  ← balance rows
│  │ Dust                   42.000   │  │
│  └─────────────────────────────────┘  │
│                                       │
│  [ Connect ]                          │  ← primary CTA
│                                       │
│  Network: ⌄ PreProd                   │  ← secondary control
│                                       │
│  ▾ Advanced  (collapsed)              │
└───────────────────────────────────────┘
```

The `≡` opens an inline panel with: seed hex (read-only with
copy + warning), "Reload demo wallet", "Generate random wallet",
network endpoint URLs, app version. On phones it slides over; on
desktop it's a side panel.

## Dark theme — minimalistic spec

Single palette across all surfaces. No gradients except a single
~3% noise overlay on the background to kill banding on OLED.

### Palette

Refined after surveying [1am.xyz](https://1am.xyz/) — they ship the
closest reference for an OLED-friendly minimalistic Midnight wallet.
We adopt their near-black background and five-step surface scale,
keep our own deep blue-violet accent (the "Midnight" tone), and stay
clear of their cyan/violet brand fills.

| Token              | Hex       | Use                                                |
|--------------------|-----------|----------------------------------------------------|
| `--bg`             | `#0a0b0d` | App background. Near-black; OLED-true.             |
| `--surface`        | `#13161b` | Card.                                              |
| `--surface-2`      | `#1a1d24` | Pressed/hover; modal-less panels.                  |
| `--surface-3`      | `#1d2939` | Inline insets (status pill, address pill).         |
| `--surface-4`      | `#252b37` | Disabled / pressed primary CTA fill.               |
| `--border`         | `#373a41` | 1 px hairlines.                                    |
| `--border-faint`   | `#252a35` | Card outer hairline (slightly inside `--border`).  |
| `--text`           | `#e6edf3` | Body, headings.                                    |
| `--text-soft`      | `#cecfd2` | Numbers, primary values.                           |
| `--text-muted`     | `#a4a7ae` | Captions, secondary label.                         |
| `--text-faint`     | `#717680` | Helper text, disabled.                             |
| `--accent`         | `#7c8cff` | Primary CTA fg, focus ring, address mono tint.     |
| `--accent-soft`    | `#2a3052` | Primary CTA bg at rest.                            |
| `--success`        | `#5fb27c` | "Synced", "Reachable", balance positive delta.     |
| `--warn`           | `#d8a94f` | "Stalled", "Connecting".                           |
| `--error`          | `#e57373` | "Unreachable", "Tx failed".                        |
| `--mono-tint`      | `#a3acc4` | Hex / address text (slightly cooler than soft).    |

Picked to:
- Pass WCAG AA against `--bg` for body text (`--text` 14.7:1,
  `--text-soft` 12.4:1) and for `--text-muted` (5.7:1). `--text-faint`
  intentionally fails AA — only used on disabled/helper rows.
- Land in the deep blue-violet that "Midnight" suggests without
  being literal navy. `--accent` is bright enough to be tappable on a
  phone in daylight; we never use it as a full-bleed fill (cf.
  1am.xyz's restraint).
- Keep success/warn/error desaturated so the dashboard doesn't feel
  like a status board.

### Typography

System stack (no web-font fetches on launch — important for mobile):

```css
font-family:
  -apple-system, BlinkMacSystemFont,
  "Inter", "Segoe UI", Roboto, sans-serif;
font-feature-settings: "tnum" on, "lnum" on;     /* tabular numerals */
```

Mono stack (addresses, hashes, seed):

```css
font-family: ui-monospace, SF Mono, "JetBrains Mono", Menlo, monospace;
```

Scale (mobile px / desktop px — desktop = mobile × 1.0; we don't
upsize on desktop):

| Token      | Size  | Weight | Use                              |
|------------|-------|--------|----------------------------------|
| `t-xs`     | 11 px | 500    | Caption, status pill.            |
| `t-sm`     | 13 px | 500    | Status sub-line, secondary label.|
| `t-base`   | 15 px | 400    | Body.                            |
| `t-row`    | 17 px | 500    | Balance numbers.                 |
| `t-h`      | 20 px | 600    | Screen heading.                  |

Address/hash text always at `t-base` mono with `--mono-tint`.

### Spacing + radius

Base = **4 px**. Use multiples (4, 8, 12, 16, 24, 32). Card radius
**12 px**. Inner row padding **16 px** vertical, **20 px** horizontal.
Hairline border = **1 px**, never 2.

### Components

- **Card**. `--surface`, 1 px `--border-faint`, 12 px radius. No
  shadow. No second-level border-radius for child rows.
- **Balance row**. Two columns: label left (`t-sm`, `--text-muted`),
  value right (`t-row`, `--text-soft`, mono). Right-aligned tabular
  nums. Top hairline between rows; no zebra striping. Optional
  third sub-line: secondary fiat or unit conversion in
  `--text-faint`, `t-xs`. *Pattern lifted from 1am.xyz — token
  symbol left / mono number right.*
- **Address pill**. Inline-flex, `--surface-3` background, 1 px
  `--border` outline, 999 px radius, 6 px / 12 px padding. Mono text
  with **truncated middle** (`mn_addr_test1qx…f9zg`) using
  CSS `direction: rtl; text-overflow: ellipsis;` on a fixed-width
  inner span, OR a manual head/tail split (recommended for
  predictable phone width). Tap → opens the inline reveal panel
  (full address + copy). Inline copy icon on the right of the pill.
  *Pattern lifted from 1am.xyz.*
- **Status indicator**. **Tiny dot + UPPERCASE label** — no pulse,
  no spinner. 6 × 6 px solid circle in success/warn/error tone +
  11 px `letter-spacing: 0.04em` uppercase label
  (`SYNCED · BLOCK 557 902`). For `Connecting`/`Syncing X%` we
  rotate a `·` `··` `···` triplet on a 600 ms interval to keep
  layout stable and DOM cheap. *Pattern lifted from 1am.xyz.*
- **Primary CTA**. **Full-width pill, 48 px tall, 16 px radius**,
  `--accent-soft` bg, `--accent` text, weight 600. Pressed state:
  `--accent` bg, `--bg` text. Press animation:
  `transform: scale(.97)` with
  `transition: transform 400ms cubic-bezier(.34, 1.56, .64, 1)` —
  the soft-bounce that 1am.xyz uses; almost imperceptible on
  desktop, satisfying on phones. Disabled: `--surface-4` bg,
  `--text-faint` text, `not-allowed` cursor.
- **DUST regeneration progress bar** (UC-4). Thin 4 px bar inside
  the Dust balance row, full width minus 16 px gutter,
  `--surface-3` track, `--accent` fill animated 0→100 % to indicate
  generation cap progress. *Lifted from 1am.xyz.* Only shown when
  the wallet has at least one registered NIGHT UTXO; hidden in the
  zero-funds empty state.
- **Network select**. Native `<select>` styled minimally — no
  custom dropdown. Saves implementation cost; matches a11y for free.
  Caret added via CSS `appearance: none` + a unicode glyph.
- **Copy button**. Icon-only on the pill (`⧉` glyph), 32 × 32 hit
  target. No toast; switch button label/icon to `✓ Copied` for 1 s,
  then revert.
- **Disclosure ("Advanced")**. Native `<details>` styled with the
  hairline + a `⌃` chevron rotated 180° on `[open]`. Animated via
  `content-visibility: auto` + a 200 ms `max-height` transition (no
  JS).
- **Skeleton loader**. `--surface-2` block with the `pulse`
  keyframe at 1 s ease-in-out. Used only during the sync gap, not
  on every state read.

### Anti-patterns we explicitly do not adopt

From 1am.xyz: scrolling marquee strips ("WASM POWERED · FAST PROOFS
…") jitter on phones; multi-color cyan/violet/pink brand chips fight
minimalism; `clamp(3rem,12vw,10rem)` hero type overflows narrow
viewports. We cap hero size at 20 px and stick to one accent.

### Layout breakpoints

- **`≤ 480 px`** (phone): single column, 16 px gutter, full-bleed
  CTA. No max content width.
- **`> 480 px`** (tablet/desktop): centered column, max-width
  **560 px**, 32 px gutter. Header and CTA respect the same width.

We do **not** add a sidebar layout for desktop in this iteration —
the same layout scales cleanly because the data set is small.

## Implementation phases

Each phase ends with a `cargo run -p dioxus-wallet` smoke test on
desktop and an `adb install` smoke test on the Pixel emulator (text
logs only — no screenshots).

### Phase A — Theme + UC-1 + UC-3 (no live network)

- Replace `assets/styles.css` with the dark theme tokens above
  (`--bg`, `--surface`, `--accent`, etc.). Add the press-bounce
  cubic-bezier transition on the primary CTA.
- Build the single-screen layout: header + address pill + balance
  card (placeholder values "—") + primary CTA + network select +
  advanced disclosure.
- Address pill renders with truncated middle + inline `⧉` copy.
- Add `wallet_core::hd` (BIP32 over the demo seed, path
  `m/44'/2400'/0'/0/0`) — depend on `bip32 = "0.5"`.
- Add `wallet_core::address::unshielded_bech32m(network, pubkey)`
  — depend on `bech32 = "0.11"`. Encodes with HRP `mn_addr` and
  per-network suffix.
- Add `Wallet::unshielded_address(network) -> Result<String, _>`.
- Connect CTA still runs existing `probe_connectivity` +
  `chain_tip` + node `system_health` — no sync yet. Show the
  status pill in dot+label form.

### Phase B — Sync drivers + UC-2

- Lift `graphql_ws_client.rs` from
  `midnight-indexer/indexer-tests/src/` (vendored with attribution
  + license header). Wrap in
  `wallet_core::indexer::ws::WalletSubscriptions`.
- Port `SyncProgress` from
  `midnight-wallet/packages/abstractions/src/SyncProgress.ts` —
  Rust struct with `applied_index`, `highest_relevant_index`,
  `is_connected`, plus `is_complete_within(gap)`.
- Port `unshielded` sync fold from
  `midnight-wallet/packages/unshielded-wallet/src/v1/Sync.ts`
  (createdUtxos / spentUtxos / status / progress).
- Port `dust` sync fold from
  `midnight-wallet/packages/dust-wallet/src/v1/Sync.ts`. DUST
  events arrive as hex-encoded SCALE-serialized
  `ledger::Event` — deserialize via the in-workspace `ledger`
  crate, no re-port.
- Add `Wallet::start_sync(network) -> SyncHandle` plus
  `watch::Receiver<SyncStatus>` and `watch::Receiver<Balances>`.
- Implement reconnect-with-backoff (1s, 2s, 5s, 5s) inside the
  sync task.
- UI: status pill driven by `SyncStatus`. `Connecting` /
  `Syncing X%` / `Synced (block N)` / `Stalled (reason)`.

### Phase C — UC-4 balances + Advanced disclosure

- Define `Balances` fold logic. Map from event types in the
  vendored TS reference (the user will share) to the `wallet-core`
  fields.
- Add NIGHT + Dust formatters (stars → decimal NIGHT, specks →
  decimal DUST).
- Disclosure panel: shielded address (hex), session id, seed hex
  with copy + warning.

### Phase D — Android polish

- APK assemble + install on emulator (no screenshots — confirm via
  `adb logcat`). Reuse the `dioxus-bench` Gradle scaffold.
- Verify safe-area insets on the emulator's display cutout.
- Sanity-check the WS endpoints reach over the emulator's NAT (they
  should — the bench app already proves outbound HTTPS works).

## Open questions

1. ~~**Unshielded address format**~~ **Resolved.** Bech32m. HRP
   `mn_<type>[_<network>]`. Source: `midnightntwrk/midnight-wallet`
   `packages/address-format/src/index.ts`. Per network:
   - **Mainnet**: `mn_addr1...` (network segment dropped)
   - **PreProd / Preview / QANet / DevNet**: `mn_addr_test1...`
   - **Undeployed**: `mn_addr_undeployed1...` (verify in package)

   Codecs we'll need this iter-1: `addr` only (unshielded). Later:
   `mn_shield-addr`, `mn_dust-...`, sub-keys.
2. ~~**TS sync reference**~~ **Resolved.** Source files in
   `midnightntwrk/midnight-wallet`:
   - `packages/dust-wallet/src/v1/Sync.ts` — fold for
     `dustLedgerEvents(id: appliedIndex)`. Each event is a
     hex-encoded SCALE `LedgerEvent` from the ledger crate. Batched
     (`groupedWithin` 10 events / 1ms / 4ms) and folded by
     `CoreWallet.applyEventsWithChanges`, advancing
     `appliedIndex → lastUpdate.id` and tracking
     `highestRelevantWalletIndex = lastUpdate.maxId`.
   - `packages/unshielded-wallet/src/v1/Sync.ts` — fold for
     `unshieldedTransactions(address, transactionId)`. Two payload
     variants: `UnshieldedTransaction`
     (createdUtxos / spentUtxos / status — applied with
     `CoreWallet.applyUpdate` or `applyFailedUpdate`) and
     `UnshieldedTransactionsProgress` (just bumps
     `highestTransactionId`).
   - `packages/abstractions/src/SyncProgress.ts` —
     `{appliedIndex, highestRelevantWalletIndex, highestIndex,
     highestRelevantIndex, isConnected}` plus
     `isStrictlyComplete()` and `isCompleteWithin(maxGap=50n)`.

   We **do not** translate the Effect/RxJS plumbing — we use
   `tokio::spawn` + `watch::Receiver<SyncStatus>` instead.
3. **Dust generation onboarding**. To actually accrue dust, the
   wallet must `registerNightUtxosForDustGeneration(utxos, pubKey,
   signFn)` — see `counter-cli/src/api.ts` `registerForDustGeneration`.
   For the demo flow we *display* dust as 0 until the user
   registers; registration UI is iter-2. Hint string in the
   zero-state row: *"Register your NIGHT UTXOs to start
   accumulating Dust."*
4. **Session id lifecycle**. Shielded subscriptions need a
   `sessionId` from `Mutation.connect(viewingKey)`. Decision:
   lazy-create on first shielded subscription, keep in memory only.
   iter-1 demo flow does **not** need shielded balance — UC-4 is
   NIGHT (unshielded) + Dust only. Shielded balance lands when we
   add the shielded-receive flow.
5. **Animation budget**. We adopt 1am.xyz's restraint — only the
   400 ms cubic-bezier press-bounce on the primary CTA, the 200 ms
   max-height on the disclosure, and the 1 s skeleton pulse during
   sync gap. Everything else is instant.

## Implementation references

Pinned external sources for porting. When the upstream API changes,
this is what we re-survey.

### iter-1 functional bar — `midnightntwrk/example-counter`

Minimal end-to-end TS wallet flow we mirror in Rust:
[`counter-cli/src/cli.ts`](https://github.com/midnightntwrk/example-counter/blob/main/counter-cli/src/cli.ts)
+ neighbor `api.ts` / `config.ts`. Flow:

```
buildWalletAndWaitForFunds:
   → HDWallet.fromSeed(seed)
   → deriveKeysFromSeed(seed) at account 0, indices 0
       roles: Zswap | NightExternal | Dust
   → WalletFacade.init({ configuration, shielded, unshielded, dust })
   → wallet.start(shieldedSecretKeys, dustSecretKey)
   → withStatus("Syncing with network", waitForSync(wallet))
       waits state.isSynced === true (RxJS observable, throttled 5s)
   → log unshieldedKeystore.getBech32Address()    // user funds via faucet
   → waitForFunds(wallet)
       waits state.unshielded.balances[unshieldedToken().raw] > 0n
   → registerForDustGeneration
       waits state.dust.balance(new Date()) > 0n
   → ready
```

For iter-1 mobile we drop the contract steps but keep the **wallet
construction → sync → wait-for-funds → register-for-dust** loop. The
"Register" button only exists once UC-4 shows a non-zero NIGHT
balance.

### Sync algorithm — `midnightntwrk/midnight-wallet`

Three independent sync streams, ported to Rust as `tokio::spawn`
tasks behind a single `Wallet::start_sync(network) → SyncHandle`
that returns `watch::Receiver<SyncStatus>` and
`watch::Receiver<Balances>`:

| Subsystem | Source                                       | Subscription                                      | Yields           |
|-----------|----------------------------------------------|---------------------------------------------------|------------------|
| DUST      | `packages/dust-wallet/src/v1/Sync.ts`        | `dustLedgerEvents(id: appliedIndex)`              | `LedgerEvent`    |
| NIGHT     | `packages/unshielded-wallet/src/v1/Sync.ts`  | `unshieldedTransactions(address, transactionId)`  | UTXO diffs       |
| Shielded  | `packages/shielded-wallet/src/v1/Sync.ts`    | `shieldedTransactions(sessionId, index)`          | encrypted events |

Each stream owns the `applyUpdate(state, update) → state` fold.
Phase derivation (our Rust `SyncStatus`) from
`(isConnected, applyGap, sourceGap)`:

```rust
match (probe_ok, ws_connected, applied, highest_relevant) {
    (false, _, _, _)                     => Stalled("endpoint unreachable"),
    (true, false, _, _)                  => Connecting,
    (true, true, a, h) if h == 0         => Connecting,        // no events yet
    (true, true, a, h) if h - a > 50     => Syncing(a as f32 / h as f32),
    (true, true, a, h)                   => Synced { block: a as u64 },
}
```

### What to port vs. depend on (consolidated)

| Source (TS)                                                            | Rust crate / approach                                         |
|------------------------------------------------------------------------|---------------------------------------------------------------|
| `wallet-sdk-address-format`                                            | New module `wallet_core::address` over `bech32 = "0.11"`      |
| `wallet-sdk-hd`                                                        | New module `wallet_core::hd` over `bip32` + `bip39` (slip10)  |
| `wallet-sdk-abstractions::SyncProgress`                                | New struct in `wallet_core::sync`                             |
| `wallet-sdk-{dust,unshielded,shielded}-wallet::v1::Sync`               | `wallet_core::sync::{dust,unshielded,shielded}` modules       |
| `wallet-sdk-indexer-client` GraphQL ops                                | Already vendored from midnight-indexer + `graphql_client`     |
| `wallet-sdk-node-client`                                               | Already in `wallet_core::node` (jsonrpsee phase-1)            |
| `wallet-sdk-prover-client`                                             | Already in `prover-core::http`                                |
| `wallet-sdk-facade` (RxJS orchestrator)                                | `wallet_core::Wallet::start_sync` w/ tokio + watch channels   |
| `@midnight-ntwrk/ledger-v8` (ledger types, dust math, SCALE)           | **Don't port** — already in `midnight-ledger` workspace       |
| `@midnight-ntwrk/wallet-sdk-{capabilities, runtime, simulation}`       | **Don't port** — Effect plumbing, replace with idiomatic Rust |
| Counter-cli `cli.ts` interactive TUI                                   | **Don't port** — we have a Dioxus UI instead                  |

## Reference index (mobile-focused)

- gsd-wallet receive flow (TS reference for address rendering):
  [`gsd-wallet/src/popup/...`](https://github.com/adamreynolds-io/gsd-wallet/tree/main/src/popup)
  (specific path to be filled when the user shares the link).
- midnight-indexer wallet-shaped subscriptions (already vendored
  schema): `mobile-bench/wallet-core/queries/midnight-indexer/`.
- midnight-indexer reference WS client to lift in Phase B:
  <https://github.com/midnightntwrk/midnight-indexer/blob/main/indexer-tests/src/graphql_ws_client.rs>
- Existing crate scaffolding: `mobile-bench/{wallet-core, dioxus-wallet}`.
- Bench-side mobile build/install instructions to mirror:
  [`DEPLOY_TO_DEVICE.md`](DEPLOY_TO_DEVICE.md).
