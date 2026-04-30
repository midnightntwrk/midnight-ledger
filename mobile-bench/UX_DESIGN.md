# Midnight Wallet & Identity — UX design

Living document. Started 2026-04-30.

This is the **UX master** for the `dioxus-wallet` app: complete screen
catalog, navigation, components, cross-cutting interaction patterns,
and content rules. Visual treatment (palette, typography, spacing)
lives in [`MOBILE_WALLET.md`](MOBILE_WALLET.md) under "Dark theme —
minimalistic spec" — this document references it but doesn't repeat
it. The list of *features* the UX serves is defined in feature plans
([`WALLET_PLAN.md`](WALLET_PLAN.md), [`DID_PLAN.md`](DID_PLAN.md));
this document defines *how* the user reaches each one.

> **Maintenance rule.** Whenever a new screen, navigation change, or
> cross-cutting pattern lands in code, update this file in the same
> commit. PRs that change the user-visible surface without updating
> the design doc should be rejected at review.

## Goals and principles

Carried over from [`MOBILE_WALLET.md`](MOBILE_WALLET.md), made
explicit at the *app* level:

1. **Mobile-first**, desktop is a supercontainer. The 390×844 phone
   viewport is the primary canvas. Desktop window inherits the same
   layout up to a max-width of 560 px so the mental model survives.
2. **One screen, one job.** Every screen has a single primary action
   (top of "stuff above the fold"). Auxiliary actions live in
   secondary positions or behind a `≡` overflow menu.
3. **No mock data.** Empty states say "empty" with one-line guidance;
   loading says "loading"; failures say what failed. Never invent
   placeholder values.
4. **Native, not nested in WebView fictions.** The app is dark-themed
   end-to-end; every native control (`<select>`, scrollbar, focus
   ring) is dark too via `color-scheme: dark` on `:root`.
5. **Touch targets ≥ 44 × 44 px.** Spacing buys reach for thumbs;
   secondary actions sit ≥ 16 px apart.
6. **State changes are obvious.** `disabled`/`pressed`/`error`
   styles are part of the component contract — see "Component
   library" below — and the status pill always reflects the wallet
   sync phase.

## Sitemap

```
App (single window)
│
├── Tab: Wallet                  (default, primary path)
│   ├── Home                     ← landing
│   ├── Receive
│   ├── Send → Confirm → Result
│   └── Transaction detail
│
├── Tab: Identity                (DID + credentials)
│   ├── My DIDs                  ← list
│   ├── DID detail               ← view + manage one DID
│   │   ├── Verification methods (list + add/edit/remove)
│   │   ├── Services             (list + add/edit/remove)
│   │   └── Also-known-as        (list + add/remove)
│   ├── Create DID               ← stepper
│   ├── Resolve DID              ← input + read-only document view
│   └── Deactivate DID           (confirm sheet, not a screen)
│
├── Tab: Activity                (chain history + DID ops log)
│   ├── Activity list
│   └── Activity detail
│
└── Tab: Settings
    ├── Networks                 (selector + custom endpoints per env)
    ├── Wallets                  (multi-wallet — iter-2)
    │   ├── Wallet list
    │   ├── Wallet detail        (seed view, copy, export, delete)
    │   └── Add wallet           (import / generate / W0–W3 quick start)
    ├── Diagnostics              (sync state, tracing log, NDJSON export)
    └── About                    (version, links, license, build hash)
```

## Navigation

**Bottom tab bar** with four tabs (Wallet · Identity · Activity ·
Settings). Always visible at the bottom of the viewport, 56 px tall,
icon + 11 px uppercase label, current tab uses `--accent`.

Why bottom tabs:
- Matches the iOS/Android norm; users find features by thumb-reach.
- Survives the 390-px desktop window without requiring a sidebar.
- Each tab is its own navigation stack (push/pop within), so the
  user's mental "back" maps to the app's back gesture.

Within a tab, screens stack via push/pop. The header carries:
- Title (left, `t-h` 20 px / 600).
- A single overflow `≡` (right) — opens an inline panel with
  context actions (e.g. "Reload demo wallet", "Copy seed",
  "Disconnect").
- A back chevron `‹` (left, when stack depth > 1), 44 px tap target.

Modal flows (Send confirm, Deactivate DID confirm) appear as **action
sheets** sliding up from the bottom — full width, centred at 60 % of
viewport height — not as separate screens. They block the tab bar
while open.

## Screen catalog

Each entry has: **Purpose**, **Layout sketch**, **Components**,
**Empty/error states**, **Transitions in/out**.

---

### Wallet · Home `tab=wallet`

**Purpose.** "What do I have, and what can I do with it?" The
landing screen post-launch. Mostly read-only with two big actions.

**Layout sketch (top → bottom).**
```
┌─────────────────────────────────────────┐
│  Midnight Wallet                  [≡]   │  header
│  ● PREPROD · SYNCED · BLOCK 557 902     │  status sub-line
├─────────────────────────────────────────┤
│  ADDRESS (NIGHT receive)                │
│  ┌─────────────────────────────────────┐│
│  │ mn_addr_preprod1ahhcw…suryn3a   [⧉] ││  address pill
│  └─────────────────────────────────────┘│
│                                         │
│  BALANCES                               │
│  ┌─────────────────────────────────────┐│
│  │ NIGHT          1,234.567 890   NIGHT││
│  │ DUST                42.000      DUST ││  balance card
│  │ ════════ DUST regen progress  60%   ││
│  └─────────────────────────────────────┘│
│                                         │
│  ┌────────────────┐  ┌────────────────┐ │
│  │     Send       │  │    Receive     │ │  primary CTAs
│  └────────────────┘  └────────────────┘ │
│                                         │
│  Network ⌄ PreProd                      │
│                                         │
│  ▸ Advanced                             │
│                                         │
├─────────────────────────────────────────┤
│   Wallet · Identity · Activity · Set    │  tab bar
└─────────────────────────────────────────┘
```

**Components.** Address pill (full address visible, mono-typed, copy
icon flips to ✓ on success); two balance rows + DUST regeneration
bar; **two side-by-side primary CTAs** (Send + Receive, each 48 px
tall, half-width with 8 px gap). Network select drops the demo
wallet onto the picked env. Advanced disclosure carries the
current diagnostic surface (probe rows, finalized head, seed view,
resolve-DID input — until they migrate to dedicated screens).

**Empty/error states.**
- *Pre-connect*: balance values render `—`; status is "DISCONNECTED";
  CTAs disabled with tooltip "Connect to enable sending".
- *Connect failed*: status pill flashes "STALLED · <reason>" in
  error tone; tap retries.
- *Zero NIGHT*: balance row shows `0.000 000`; sub-line below the
  card reads "Send NIGHT to the address above to top up".

**Transitions.** Send → push `Wallet/Send`. Receive → push
`Wallet/Receive`. Network change → soft refresh (re-derives demo
address, keeps history).

---

### Wallet · Receive

**Purpose.** "Show me my address; let me share it."

**Layout sketch.**
```
‹ Receive                          [≡]
─────────────────────────────────────
ASSET   NIGHT (unshielded)  ⌄
        (toggle to "Dust", "Shielded NIGHT" later)

         ┌─────────────────┐
         │                 │
         │   QR code       │   ← 240×240, white-on-black
         │   (mn_addr_…)   │     for camera scanability
         │                 │
         └─────────────────┘

ADDRESS
┌─────────────────────────────────────┐
│ mn_addr_preprod1ahhcw…suryn3a   [⧉] │
└─────────────────────────────────────┘

[ Copy address ]   ← full-width

NETWORK   PreProd
DERIVATION   m/44'/2400'/0'/0/0   (mono)
```

**Components.** Asset selector (`<select>` initially; tab strip
later when shielded + dust addresses ship). QR generated on-screen
via `qrcode` crate output as inline SVG. Same address pill as Home.

**Empty states.** None — there's always an address to display.

**Transitions.** Back to Home.

---

### Wallet · Send → Confirm → Result

**Purpose.** Construct a transfer; review it; submit it.

Three steps share the same back-stack; **Send** is the form, **Confirm**
is an action sheet, **Result** replaces the form on success.

**Layout sketch — Send.**
```
‹ Send                                 [≡]
──────────────────────────────────────────
ASSET   NIGHT (unshielded)  ⌄

TO
┌──────────────────────────────────────┐
│ mn_addr_preprod1…                  [⌽] │  ← paste / scan QR
└──────────────────────────────────────┘
☑ Address valid · resolves on PreProd

AMOUNT
┌─────────────────────┐  ┌──────┐
│ 12.345              │  │ Max  │
└─────────────────────┘  └──────┘
≈ $0.00 USD           Available: 1,234.567 NIGHT

MESSAGE (optional, 280 chars)
┌──────────────────────────────────────┐
│ For brunch                            │
└──────────────────────────────────────┘

FEE   estimated 0.000 002 NIGHT       ⌃ details
                                      └ DUST budget: 0.0042
                                        Fallback Dust required: no

[                Continue                ]
```

**Confirm action sheet.** Slides up from bottom; full address;
amount; fee; **Hold to send** button (1.0 s long press → triggers
prove + submit) so accidental taps can't fire a tx.

**Result** screen replaces the form on success. Contains tx hash,
explorer link, timestamp, "Done" button → back to Home. On failure
shows the error chain + a "Download diagnostic JSON" link
(populates a downloadable bundle per
`MOBILE_WALLET.md`'s failed-tx export pattern).

**Empty/error states.**
- *Address invalid*: red strip under field with parser error.
- *Insufficient balance*: amount field gets red border, sub-line
  "Need 12.345 NIGHT, have 5.000".
- *Fee unavailable*: Continue disabled with "Fetching fee
  estimate…".
- *Prove failed* / *submit failed*: Result screen shows error,
  diagnostic export.

**Transitions.** `‹` returns to Home (form state cleared); Confirm
sheet dismissable by swipe-down. After Result, Done → Home.

---

### Wallet · Transaction detail

**Purpose.** Single tx receipt — what happened, when, who, how.

**Layout sketch.**
```
‹ Transaction                          [⧉]
──────────────────────────────────────────
☑ Confirmed · block 557 902 · 2 min ago

TYPE        Send · NIGHT · unshielded
AMOUNT      −12.345 NIGHT
TO          mn_addr_preprod1…
FROM        mn_addr_preprod1ahhcw…
FEE         0.000 002 NIGHT
HASH        0xa1b2c3…   [⧉]
SIGNATURE   schnorr · 64 B  [⧉]
NOTES       For brunch

[ View on explorer ↗ ]
[ Download diagnostic ]
```

Used both from Activity tab and from Wallet/Result.

---

### Identity · My DIDs

**Purpose.** "Show me the DIDs this wallet controls."

**Layout sketch.**
```
Midnight Identity                       [≡]
─────────────────────────────────────────
MY DIDs (1)

┌─────────────────────────────────────┐
│ ● Active                             │
│ did:midnight:preprod:abcd…f9ab        │
│ 4 verification methods · 1 service    │
│ Last updated 12 days ago              │
└─────────────────────────────────────┘

[      Create DID         ]
[      Resolve DID        ]
```

**Components.** DID row: status dot (green active, grey deactivated),
shortened DID (mono), summary count, last-updated relative time.
Two full-width primary CTAs at the bottom.

**Empty state.** "You don't have any DIDs yet." sub-line, then the
two CTAs (Create DID / Resolve DID).

**Loading state.** Skeleton row during indexer fetch.

**Transitions.** Tap row → `Identity/DID detail`. Create DID → push
`Identity/Create DID`. Resolve DID → push `Identity/Resolve DID`.

---

### Identity · DID detail

**Purpose.** "Show me everything about this one DID, let me edit it."

**Layout sketch.**
```
‹ DID                                  [≡]
──────────────────────────────────────────
● ACTIVE · v8 · updated 12 days ago

did:midnight:preprod:abcd…f9ab    [⧉]
controlled by this wallet

VERIFICATION METHODS (4)        [+ Add]
┌─────────────────────────────────────┐
│ #key-0   Ed25519 · auth, assert      │
│ #key-1   Jubjub  · keyAgreement      │
│ #key-2   P-256   · capInvocation     │
│ #key-3   Ed25519 · capDelegation     │
└─────────────────────────────────────┘

SERVICES (1)                    [+ Add]
┌─────────────────────────────────────┐
│ #service-0   LinkedDomains           │
│   https://example.com                │
└─────────────────────────────────────┘

ALSO KNOWN AS (0)               [+ Add]
(empty)

▾ Advanced
   ▸ Document JSON
   ▸ Operation history
   ▸ Deactivate DID

[≡] menu:
  · Copy DID
  · Copy bech32m alias
  · Open in explorer
  · Refresh
```

**Components.** Two-line section headers with `[+ Add]` button
(small, accent text, 28 px tall). VM row: fragment id (mono), curve
+ relations summary in muted tone. Service row: id + endpoint
(truncated). Long-press a row → action sheet (Edit / Remove).

**Empty subsections.** Render the section header even when empty,
with grey "(empty)" sub-line so the user sees the affordance to add
one. Better than hiding the section.

**Transitions.** [+ Add] → push `Identity/VM add` or
`Identity/Service add`. Tap row → push edit screen. Deactivate DID
→ confirm sheet (full-screen, centered, "Hold to deactivate"
button). Document JSON expands inline (mono code block).

---

### Identity · Create DID

**Purpose.** Stepper through everything we need to deploy a DID
contract: choose initial verification methods, choose initial
relations, sign the deploy.

**Layout sketch.**
```
‹ Create DID · Step 1/3                [≡]
──────────────────────────────────────────
INITIAL VERIFICATION METHOD

To create a DID we need at least one
verification method. We'll generate one
for you, derived from this wallet's seed.

METHOD ID
#key-0    (default; renameable later)

CURVE
( ) Ed25519   recommended
( ) Jubjub    (for Midnight on-chain crypto)
(•) P-256     widely interoperable

AUTO-ASSIGN RELATIONS
☑ authentication
☑ assertionMethod
☐ keyAgreement
☐ capabilityInvocation
☐ capabilityDelegation

[                Next                  ]
```

**Steps.**
1. Initial verification method (curve + relations).
2. Optional: initial service. Skip if not needed.
3. Confirm + deploy. Shows the constructed initial state, asks
   "Hold to deploy" (1.0 s) → routes through prover-core +
   subxt-submit + indexer-watch. Result screen on confirmation
   shows the new DID id and a "Open DID" button.

**Empty/error states.**
- *Wallet not synced*: step 3 disabled with "Connect first".
- *Insufficient DUST*: step 3 disabled, "You need 0.012 DUST to
  deploy. Register NIGHT UTXOs from Settings → Networks."
- *Prove failed*: result screen shows error + diagnostic export.

**Transitions.** Each step → next step. Cancel at any time → back
to `Identity/My DIDs`.

---

### Identity · Resolve DID

**Purpose.** Read-only lookup of any DID — yours or someone else's.

**Layout sketch.**
```
‹ Resolve DID                          [⧉]
──────────────────────────────────────────
DID
┌──────────────────────────────────────┐
│ did:midnight:preprod:abcd…           │
└──────────────────────────────────────┘

[             Resolve                  ]

(after lookup)

● ACTIVE · v3 · updated 2 hours ago

VERIFICATION METHODS (3)
…

SERVICES (0)
(empty)

▾ Document JSON
   { "id": "did:midnight:preprod:…", … }

[ Copy document ]
```

**Components.** Same input + result block as the current Advanced
disclosure prototype, promoted to its own screen. Read-only views
of VMs / services share styling with `Identity/DID detail` but
without `[+ Add]` buttons.

**Errors.** Network error / unknown DID → red strip under input.

---

### Activity · list

**Purpose.** Per-wallet timeline of every wallet-driven event:
sends, receives, DID create/update, dust register, etc.

**Layout sketch.**
```
Activity                               [≡]
──────────────────────────────────────────
TODAY
┌─────────────────────────────────────┐
│ ↑ Send NIGHT     14:32  −12.345     │
│ ✚ DID created    11:08              │
└─────────────────────────────────────┘

YESTERDAY
┌─────────────────────────────────────┐
│ ↓ Receive NIGHT  20:11  +50.000     │
│ ✚ VM added       18:42              │
└─────────────────────────────────────┘

EARLIER
…
```

Tapping a row → `Wallet/Transaction detail` for tx events, or
`Identity/DID detail` for DID events.

**Empty state.** "No activity yet. Once you send or create a DID,
it'll show up here."

---

### Settings · root

**Purpose.** Everything not part of the Wallet / Identity / Activity
flows.

```
Settings                                [≡]
──────────────────────────────────────────
NETWORKS                              ›
WALLETS                               ›
DIAGNOSTICS                           ›
ABOUT                                 ›

THIS APP
Version 0.1.0+gitabcd123
Build 2026-04-30 17:24
[ Send feedback ]
```

Subscreens follow the same row pattern as DID detail.

---

### Settings · Networks

**Purpose.** Pick a network, override endpoints per env.

```
‹ Networks                              [⧉]
──────────────────────────────────────────
ACTIVE                Preprod          ›

ALL NETWORKS
( ) Mainnet
(•) PreProd
( ) Preview
( ) QANet
( ) DevNet
( ) Undeployed (localhost)

ENDPOINTS · PreProd
INDEXER HTTP   https://indexer.preprod… ✎
INDEXER WS     wss://indexer.preprod…   ✎
NODE WS        wss://rpc.preprod…       ✎
PROOF SERVER   http://localhost:6300    ✎

[ Reset to defaults ]
```

Editing a URL pops a single-field action sheet with Save / Cancel.

---

### Settings · Diagnostics

**Purpose.** Live sync state + last-N events ring buffer + NDJSON
export. The "what's actually happening" debugging surface.

```
‹ Diagnostics                          [⧉]
──────────────────────────────────────────
SYNC STATUS
Shielded   100% · synced
Unshielded 100% · synced
Dust        87% · syncing 5,420 evt/s

CONNECTIVITY
✓ indexer http   642 ms
✓ indexer ws     891 ms
✓ node ws       1024 ms

EVENTS  (last 2 000)
[All ⌄] [Errors ⌄] [Sync ⌄]
17:24:42  INF  ledger    block 557902 received
17:24:31  WRN  sync      gap detected: 12 events
…

[ Download NDJSON ]
[ Clear log ]
```

---

## Cross-cutting interaction patterns

### Status pill (header sub-line)

`● COLOR · NETWORK · STATE [· DETAIL]` — single source of truth for
"is the wallet usable right now". Already specified in
`MOBILE_WALLET.md`. States carry through every screen.

### Confirmation gestures

Any operation that **moves on-chain state** (Send, Create DID, Add
VM, Deactivate DID) requires the user to **hold a button for 1.0 s**
to confirm. Visualised as a fill ring around the button text. Tap
fires the same action only after a `:hover` of ≥ 200 ms — prevents
accidental taps.

### Long-running operations

Anything that takes > 1.0 s shows:
- Spinner-equivalent (`···` rotating dots) inside the originating
  CTA, label changes to present-tense ("Proving…", "Submitting…").
- A status pill update if it's a multi-stage op
  (`Proving → Submitting → Awaiting confirmation → Confirmed`).
- Cancel allowed up to "Submitting"; after that, the op is
  on-chain and Cancel becomes "Hide".

### Long-press = inspect

Any list row that has more actions than fit beside it (tap-to-open)
exposes them on long-press as an action sheet. Discoverable via
the consistency: rows with `›` always do tap-to-open, rows without
have long-press menus.

### Empty states

Three sentences max, no illustrations: title (5 words),
explanation (12), action (button). Example for empty My DIDs:
- "You don't have any DIDs yet."
- "DIDs let you sign verifiable credentials and connect to dApps."
- `[ Create DID ]`

### Loading states

- Below 200 ms: nothing.
- 200 ms – 1 s: spinner inside the originating CTA.
- > 1 s: skeleton card replacing the contents being loaded; status
  pill says what's happening.

### Error states

- Inline (form field): red 1 px strip under the field, error in
  `--error` tone, single-line.
- Section-level (chain query failed): red card replaces the
  contents, with a "Retry" pill in `--error-soft`.
- Tx fatal: routes to a dedicated Result screen with full diagnostic
  + downloadable JSON bundle.

## Component library (extending `MOBILE_WALLET.md`)

In addition to the components already specified in that doc:

| Component | Where used | Spec |
|---|---|---|
| **Tab bar** | App-wide bottom nav | 56 px tall, 4 tabs, icon + 11 px UPPERCASE label, active tab uses `--accent`. Sits inside safe-area inset on iOS / Android. |
| **List row** | All `›` rows | 56 px min-height, 16 px gutter, label left, secondary right, `›` 12 px from right edge in `--text-faint`. |
| **Section header** | All grouped lists | 13 px UPPERCASE, `--text-muted`, 24 px top margin, 8 px bottom; `[+ Add]` button right-aligned same row. |
| **Action sheet** | Confirm send / deactivate / row long-press menus | Full-width, slides up from bottom, 16 px gutter, dismissable via swipe-down or `Cancel`; max 60 % viewport height; `--surface-2` bg. |
| **Hold-to-confirm CTA** | All on-chain writes | Same 48 px pill as Primary CTA; on `pointerdown`, fill ring grows over 1 s; on `pointerup` before 1 s, ring resets and operation cancels. |
| **QR code** | Receive | 240×240 inline SVG, white modules on `--surface-2` bg, 16 px quiet zone. |
| **Stepper header** | Multi-step flows (Create DID, Send) | "Step N/M" right-aligned in `--text-muted`, current step in `--text`. Provides a dotted progress strip below the title. |
| **Document panel** | DID detail + Resolve DID | Mono `--mono-tint` text, scrollable inline, copy button top-right. |

## Accessibility

- All interactive elements have an accessible name (Dioxus
  `aria-label` when no visible text).
- Focus ring uses `--accent` outline, 2 px offset, on every focusable.
- Min contrast WCAG AA (already verified for the palette in
  `MOBILE_WALLET.md`).
- Status pill updates announced via `aria-live="polite"` on the
  status-line container.
- Long-press has a tap-and-hold-to-trigger keyboard equivalent
  (Shift+Enter on a row).

## Out of scope (explicit non-goals)

- **Push notifications** for incoming txs / DID resolution events
  — out of scope for desktop, deferred for mobile.
- **Multi-device sync** of DIDs / wallets via cloud — explicitly
  not supported (privacy decision).
- **DApp connector** (`window.midnight`) — see `WALLET_PLAN.md`'s
  Non-goals; the native app doesn't host web pages.
- **Light mode** — single dark theme only.

## Roadmap (UX-first)

Aligned to feature plans in `WALLET_PLAN.md` and `DID_PLAN.md`:

| Iteration | UX surface lit up |
|---|---|
| **iter-1 step-1** ✅ | Wallet/Home (Phase A skeleton) |
| **iter-1 step-2** ✅ | Wallet/Home — Connect, status pill, chain card |
| **iter-1 step-3** ✅ | Wallet/Home — full address pill + clipboard |
| **iter-1 Phase B** | Wallet/Home — real balances; Wallet/Receive |
| **iter-1 Phase C** | Wallet/Send → Confirm → Result |
| **DID Phase 2b/c** ⏳ | Identity/Resolve DID (full document); Identity/My DIDs (read-only) |
| **DID Phase 3**   | Identity/Create DID stepper; My DIDs writes; DID detail |
| **DID Phase 4**   | All circuit screens (VM add/remove, services, also-known-as, deactivate) |
| **iter-2** | Tab bar; Settings/Networks; Activity tab; multi-wallet |
| **iter-3** | Settings/Diagnostics with NDJSON export, Hold-to-confirm pattern, Action sheets |
| **iter-4** | Bundled mainnet snapshot UX, Onboarding screens, Android polish |

## Open questions

1. **Hierarchy under Identity tab when DIDs are zero**: should the
   landing default-show "Create DID" + "Resolve DID" prominently
   instead of the empty list? Currently the empty state surfaces
   both CTAs but they're below a "(empty)" message. Worth user
   testing once we have anyone to test with.
2. **DID address shortening**: gsd-wallet uses `mn_addr…suryn3a`
   (truncated middle). For DID strings (much longer) we use the
   same pattern with longer head/tail. Confirm legibility on a
   real phone before committing.
3. **QR code for DIDs**: should `Identity/DID detail` offer a QR of
   the DID URI for verifier-side sharing? Most DID-aware verifier
   apps support QR ingest. Tentatively yes, in `[≡]` overflow.
4. **Activity timeline retention**: how many events do we surface
   per wallet by default? gsd-wallet does 2 000 — applies the same
   to per-wallet activity. Reassess once Activity ships.
5. **DID create with multiple initial VMs**: midnight-did-api's
   `createDID` accepts a single VM at deploy. Adding more becomes
   `Add VM` after deploy. UX-wise this is two-stage; consider
   collapsing into a single "first run" wizard later.
