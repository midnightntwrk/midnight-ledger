# Midnight SSI Wallet — UI/UX redesign backlog

**Audience:** the engineer who will implement the redesign in Dioxus rsx + `assets/styles.css`.
**Status:** design spec. No Rust touched.
**Reference:** `/tmp/vc-bundle/vc_inventory_modern.html` + `vc_inventory_modern.css` (the digital-passport mock that landed the visual language we now propagate). Worked example in-tree: `mobile-bench/dioxus-wallet/src/vc_views/digital_passport.rs` + the `.vc-card-passport__*` block in `assets/styles.css`.

## Executive summary

The wallet today is **functionally complete, visually utilitarian.** It is an M3-flavoured dark-theme phone-shell with eleven tabs, a single 12 px-radius card primitive, a tonal-accent button system, and dense rows of `seed-blob` monospace text dumps for every outcome — chain hashes, JSON, errors. The card grammar reads engineering-grade: small uppercase labels, flat surfaces, accent purple `#7C5CFF` on `#080B14`, no depth, no ornament. Two surfaces have already moved up: the **digital-passport VC card** (commit `4e4a6501`) brings tier chips, lockable claims and verify badges; the **DID picker modal** (`did_picker.rs`) lands a clean modal pattern with status chips. Everything else still wears the v1 utilitarian skin.

The redesign should make this feel like a **production consumer wallet** — Trust Wallet / Apple Wallet / Rainbow polish, minus the speculation iconography. Lift the card primitive to a **22 px radius layered surface** with ambient blobs and a faint inner highlight. Replace `seed-blob` data dumps with **monospace meta-grids** like the bundle's `.meta-item`. Standardise on **pill chips** (status / tier / category) — the wallet already has `status-pill`, `did-picker-status--active`, `log-level-pill`, `vc-card-passport__tier-chip`; they should all collapse into ONE `<Chip variant="…">` atom. Introduce a **gradient primary CTA** (green→cyan) for the one "do the thing" button per screen, demote `cta` and `btn-primary` to the right contexts. Establish a **tab-shell template** that gives every tab a hero strip, a section grid, and a sticky bottom-tab-nav — currently every tab does its own vertical stack of bare `.card`s with no rhythm.

Target: ship the visual lift in two waves. **Wave 1 (P0)** is tokens + atoms + the top-3 tabs (Wallet / DIDs / Identity Centre) — that's the demo path. **Wave 2 (P1)** is the rest of the tabs and the modal polish. **P2** is motion + skeleton-loading + a11y pass.

---

## A. Per-screen audit

### A.1 Wallet tab

**`src/app.rs:1695–1790` (`Tab::Wallet` arm) + `AddressCard` (8247) + `BalancesCard` (8276) + `WalletSyncPane`.**

**What it looks like now.** Header wordmark + `≡` menu, "WALLET" uppercase subtitle, status line ("◉ synced · tip 1,234,567"). Then a stack of three flat `.card`s: Address (with mono-text block + copy button), Balances (two rows NIGHT + DUST with `value-stack`), and a big tonal "Connect" CTA. Underneath: a network `<select>`, two demo-reload buttons in a `.row`, and a Wallet sync pane with progress bars + foot meta. No hero. No accent. The most important thing on the screen — your money — is the same visual weight as the row of dev buttons below it.

**What works.** Balance row layout (exact value primary, `≈ 5K` compact tag secondary) — *don't change the data shape, only the typography*. Copy-button hover→checkmark microinteraction. The status line pattern (dot + uppercase line) is the right vocabulary; it just needs to live inside a hero strip.

**What doesn't.**
1. No visual hierarchy — Address / Balance / network selector / "Reload demo" all read at the same level. Balance should be the page hero.
2. The CTA "Connect" is tonal purple-on-purple-soft — same colour family as every other button on the screen, so the eye doesn't land on it.
3. `address-block` mono text fills nearly a full width but truncating at half-width with a copy chip would read calmer.
4. `WalletSyncPane` is a debug panel masquerading as a feature — the four-line meta strip ("epoch · viewing-key · …") belongs behind a "Details" disclosure.
5. No QR for the receive address.

**Top-3 lifts.**
- **Hero balance** — large `clamp(2.5rem, 8vw, 4rem)` weight-800 number with the currency unit at 0.4× and a "≈ $0 USD" secondary line. Borrow the bundle's `h1` ramp.
- **Gradient connect CTA** — green→cyan gradient `.btn-primary` only when `phase == Idle`; tonal once synced.
- **Address card collapse** — truncated `did:midnight…7fd5e` with a QR icon-button to expand to full + show QR.

### A.2 DIDs tab (inventory + detail view)

**`src/app.rs:1791–2115` (`Tab::Dids` arm) + `DidInventoryPanel` (8037) + `DidDetailView`.**

**What it looks like now.** When `open_did` is None: a Create-DID wizard stub (currently unmounted, see comment at 1792–98), then a list of inventory rows. Each row is a `.card.interactive` with a 4-column grid: status chip · DID (truncated mono) · counter / VM / service / block height meta · "Open →" affordance. When `open_did` is Some: an 8-tab carousel of detail panes (Resolve, Operations, etc.). 

**What works.** The inventory row's structure is already close to right — status-first, mono-DID, secondary meta. The detail carousel uses `.carousel-nav` (chip rail) which is a solid pattern. Status chips `did-picker-status--{active,pending,deactivated}` already exist.

**What doesn't.**
1. Inventory row chrome reads cramped on phones — five columns crammed into 360 px.
2. No empty state when inventory is empty — the user lands on a bare wizard stub.
3. The detail carousel is functionally rich but visually muted — every sub-tab is "card + table"; no card differentiation.
4. The mono-DID truncation cuts in the middle (`did:midnight…f9a5`); users want a *visible "tail"* (last 6 chars) since that's how they identify "the one I just made".

**Top-3 lifts.**
- **Card-as-row** for inventory: 22 px radius, ambient purple blob behind the status chip, big DID type-mark on the left (e.g. a small gradient token like the bundle's `.credential-token`).
- **Filter chips** above the list (All / Active / Pending / Deactivated).
- **Empty state** with illustration + "Bootstrap your first DID" CTA → routes to the Bootstrap tab.

### A.3 Identity Centre tab

**`src/identity_centre.rs:273` `IdentityCentrePanel` → `ScanQrSection` + `Oid4vciSection` + `VcInventorySection`.**

**What it looks like now.** Three stacked `.card`s. "Identity Centre" header card with a one-paragraph blurb. "📷 Scan QR" card with a paragraph + CTA. "Request VC (OID4VCI)" card with a textarea + CTA + outcome blob. "VC inventory" card listing every stored VC — for `digital-passport:v1` it renders the new card from `vc_views/digital_passport.rs`; for everything else, a fallback row.

**What works.** The digital-passport card is the gold-standard surface in the wallet right now — tier chips, claim toggles, verify badge. The Scan QR auto-dispatch (OID4VP vs OID4VCI by URL prefix) is the right UX.

**What doesn't.**
1. "Scan QR" should be a hero — it's the primary action of the tab, currently it's row 2.
2. The OID4VCI card has a paste-URL `<textarea>` exposed — should be hidden behind a "Paste URL" disclosure since the primary path is camera scan.
3. The VC inventory header is the same text-weight as the section above; needs a count badge and a filter row.
4. Outcomes (`wizard-outcome ok` / `err`) are walls of mono — should be styled like the bundle's `.success` `.meta-item` (label + truncated value + copy chip).

**Top-3 lifts.**
- **Big Scan-QR hero card** — full-bleed gradient (cyan→purple), QR-frame illustration, single CTA "Scan QR code". Auto-routes by scheme.
- **Outcome banner** atom — replaces `wizard-outcome` blobs. Three variants: `ok` (green strip + "View" button), `err` (red strip + "Copy error"), `info` (cyan).
- **VC inventory header** — title + "3 credentials" count chip + filter chip rail (All / Passport / KYC / Other), then the cards.

### A.4 Bootstrap tab

**`src/identity_centre.rs:510` `BootstrapPanel` → `BootstrapSection` (Identity Centre DID minting) + `Oid4vpSection` (paste-URL).**

**What it looks like now.** Header blurb card. "Identity Centre DID" card: shows the truncated active DID or "No Identity Centre DID yet…" + the "Bootstrap DID with VC keys" CTA. While `busy`, an inline activity-feed `<ul>` of three monospace lines. Outcomes blob below. "Authenticate with QR (OID4VP)" card: textarea + CTA + outcome blob.

**What works.** The activity-feed-while-busy pattern is a great touch — the bootstrap is a 2-3 minute operation and the live op-log keeps the user from thinking it's frozen. Don't lose it.

**What doesn't.**
1. The DID-status state is just text — should be an `OutcomeBanner` with the "Active DID" chip pattern from `ic-did-row` rendered properly.
2. The activity feed is inline-styled monospace 11 px — extract it into a `LogStream` molecule with monospace, fade-out top edge, and a "Show more" disclosure.
3. The OID4VP textarea is exposed by default — same disclosure pattern as OID4VCI.
4. "Operator/dev setup" framing is missing — the tab description on `app.rs:116-122` says this tab is for operators; the UI should signal that with an "Advanced" or "Setup" eyebrow.

**Top-3 lifts.**
- **Eyebrow + sectioned layout** — "OPERATOR · BOOTSTRAP", h1 "Seed a wallet", description, then collapsible Sections 1 & 2.
- **Live `ActivityFeed`** molecule with smooth pop-in for each new line, mono, max-height with fade.
- **Disclosure pattern** for the paste-URL flows — closed by default with a "Paste URL instead" link.

### A.5 Keys tab

**`src/app.rs:4464` `KeysTab`.**

**What it looks like now.** A segmented control selecting curve type (Ed25519 / Jubjub / P-256), a table of the wallet's keys with kid + curve + public-key blob, refresh icon-button. Functional but pure debug surface.

**What works.** Segmented control is a clean primitive — keep it as the `Segmented` molecule. The `.icon-btn` next to it is the right pattern for "fourth action that would otherwise overflow".

**What doesn't.**
1. Public-key blobs occupy 4-5 lines each — break-anywhere on phones. Needs the `CopyableMonoBlock` molecule with default collapsed (first 8 + last 8 chars) and tap-to-expand.
2. No "kid" semantic grouping — a key authority-chain (controller · authentication · assertion) is a flat list; should be grouped by `kid` fragment.
3. No empty state when curve has no keys.

**Top-3 lifts.**
- **Group by purpose** — collapse the list into 3 sections: "Authentication", "Assertion / VC signing", "Other". Each section's keys live inside.
- **`KeyCard` molecule** — kid + curve chip + copyable mono pubkey + (future) "Use to sign" affordance.
- **Empty state** per curve filter.

### A.6 Diagnostics tab

**`src/app.rs:2120–2256` (`Tab::Diagnostics` arm) — a 5-page `.carousel` containing Probes / Metrics / Benchmark / Test / Logs.**

**What it looks like now.** A horizontal scroll-snap carousel with five pages, navigated by a chip rail at top (`.carousel-nav-item`). Page 0 is the network-probe + node-identity panel. Pages 1-4 are the previously-top-level Metrics / Benchmark / Test / Logs tabs, unchanged.

**What works.** Collapsing four dev-only tabs behind one user-facing "Diagnostics" was the right call. The carousel pattern is good — chip rail at top, scroll-snap below, swipe on touch.

**What doesn't.**
1. Each carousel page is a different visual style — Probes is rows of `.probe`, Metrics is `.metrics-table`s, Logs is a virtualised list. Needs a `DiagnosticsShell` template so each page shares an h2 + description.
2. The chip rail at top has no badge for "page has new data" — would help operators know which carousel page just got a fresh probe result.
3. Logs row layout (`.log-row` with stamp · level · target · message) is solid — DON'T touch it.

**Top-3 lifts.**
- **Per-page header** — eyebrow ("DIAGNOSTICS · PROBES"), h2, description, optional "last updated" timestamp.
- **Badge dot** on chip rail items when an out-of-view page has new data.
- **`StatRow` molecule** for Metrics counters — label · value · sparkline placeholder.

### A.7 Settings tab

**`src/app.rs:5151` `SettingsTab` + `ControllerSecretCard` + `WalletBackupCard` + `JsBridgePanel`.**

**What it looks like now.** Long vertical stack of `.card`s: Wallet store badge, Controller secrets, Wallet backup (export/import seed), JS bridge state, possibly more. Each card a different layout (one is a kv-grid, one is a CTA-row, one is a textarea).

**What works.** The structure (one concern per card) is right.

**What doesn't.**
1. No sectioning — Settings looks like one long scroll of unrelated controls. Needs section headers: "Wallet", "Security", "Developer", "Network".
2. The seed-blob copy/export UX for backup is the most safety-critical control in the wallet and gets the same visual treatment as a debug-only JS bridge probe. **Backup needs its own emphasis: warning eyebrow + danger-tinted card border + confirm-modal-gated reveal.**
3. No "About" / version / build-hash footer.

**Top-3 lifts.**
- **Sectioned settings** — group cards under "Wallet · Security · Developer · About".
- **`DangerCard` variant** for backup — red-tinted border, lock-icon, "Reveal seed (passphrase required)" CTA.
- **About row** at the bottom: version, build hash, "Open log directory" link, "Reset wallet" (gated).

### A.8 Modals

#### DID Picker modal — `src/did_picker.rs`

**What works.** This is the cleanest modal in the wallet — backdrop, dialog, title + subtitle, scrollable list of rows, footer cancel. Each row has truncated-mono DID + meta line ("Active · ctr=12 · 3 VMs · …") + status chip. Don't disturb it; lift its language into the global `PickerModal` template.

**What doesn't.** The "Cancel" button at the footer is the only action — picker rows themselves are the implicit "confirm". Add a clearer "tap a DID to continue" hint near the top.

#### Create-DID wizard modal — `src/app.rs` `CreateDidWizard`

Currently unmounted (see comment at `app.rs:1792–98`), but the component still exists with `.wizard-steps` + `.wizard-step` + `.wizard-outcome`. When re-mounted: needs the same modal language as DID-picker (backdrop, dialog, title, content, footer actions).

#### Verify-VC outcome

Today a `wizard-outcome ok|err` strip under the OID4VCI section. Lift to a global `OutcomeBanner` atom + a `VerifyVcResultModal` for the longer signature-trace explanation when the user taps "Why?".

---

## B. Atomic-design component inventory

Naming follows Brad Frost. Props are TypeScript-ish for legibility — the actual Dioxus types live in the implementer's component crate.

### B.1 Atoms

| Atom | Props | Variants | Replaces |
|---|---|---|---|
| **`Button`** | `variant`, `size`, `disabled`, `loading`, `onClick`, `iconLeft?`, `iconRight?`, `children` | `primary` (gradient green→cyan), `tonal` (purple-soft), `ghost` (transparent), `danger` (red-outline), `text` | `.cta`, `.btn-primary`, `.btn-text`, `.btn-danger`, all bare `<button>` rules |
| **`IconButton`** | `icon`, `size: 32\|40\|44`, `tone: muted\|accent\|danger`, `disabled`, `onClick`, `aria-label` | round (`.icon-btn`), squircle (`.copy-btn`) | `.icon-btn`, `.copy-btn`, `.ghost-button` (bundle), header `.menu-btn` |
| **`Chip`** | `variant`, `tone`, `size`, `iconLeft?`, `children` | `status` (with dot), `tier`, `category`, `version`, `count` | `.status-pill`, `.did-picker-status--*`, `.log-level-pill`, `.vc-card-passport__tier-chip`, `.store-badge`, `.op-stat`, `version-chip`, `binding-chip` |
| **`Input`** | `type`, `value`, `onChange`, `placeholder`, `error?`, `monospace?`, `prefix?`, `suffix?` | `text`, `password`, `numeric`, `search` | bare `input[type=text]`, `.search-field`, `.threshold-control input` |
| **`Textarea`** | `value`, `onChange`, `placeholder`, `rows`, `monospace?` | default, mono | bare `textarea` |
| **`Label`** | `as: span\|div`, `tone: default\|muted\|faint`, `caps?`, `children` | eyebrow (uppercase tracked), label (form), helper | `.label`, `.eyebrow`, `.card-header` |
| **`Heading`** | `level: 1\|2\|3\|4`, `children` | hero (clamp 2.35–5rem), section (1.4rem), card (1.05rem), row | `h1`–`h4` ad-hoc |
| **`Spinner`** | `size`, `tone` | inline (12 px), button (16 px), block (24 px) | currently absent; `"Working…"` text + disabled button |
| **`Avatar`** | `seed`, `size`, `kind: did\|address\|user` | mono-token (gradient like bundle's `.credential-token`), initials, jazzicon-ish | currently absent |
| **`Badge`** | `count`, `tone`, `dot?` | numeric pill, indicator dot | currently absent; ad-hoc `(3)` strings |
| **`Tooltip`** | `text`, `placement`, `children` | dark, light | currently absent (uses `title=` attribute) |
| **`Divider`** | `inset?`, `strong?` | full, inset, strong | `.divider`, `<hr>` |
| **`Dot`** | `tone: success\|warn\|error\|muted` | always 6 px | `.dot` |
| **`Switch`** | `checked`, `onChange`, `disabled`, `label?` | iOS-style | currently absent |
| **`Skeleton`** | `width`, `height`, `radius` | line, block, circle | currently absent |

### B.2 Molecules

| Molecule | Props | Replaces |
|---|---|---|
| **`MetaItem`** | `label`, `value`, `mono?`, `tone?`, `copyable?` | `.detail-kv`, `.ic-did-row`, `.balance-row` (label/value half), bundle `.meta-item` |
| **`MetaGrid`** | `items: MetaItem[]`, `columns: 1\|2\|3\|4` | `.did-inventory-row` meta column, `.detail-kv` clusters, bundle `.meta-grid` |
| **`CopyableMonoBlock`** | `value`, `truncated?`, `expandable?`, `multiline?` | `.seed-blob`, `.address-block`, `.did-picker-row-did.mono` |
| **`StatusRow`** | `tone`, `label`, `secondary?` | `.status-line`, bundle `.trust-row` |
| **`OutcomeBanner`** | `tone: ok\|err\|warn\|info`, `title`, `body`, `actions?` | `.wizard-outcome.ok`, `.wizard-outcome.err`, ad-hoc error rows |
| **`SectionHeader`** | `eyebrow?`, `title`, `subtitle?`, `actions?` | repeated `.card-header` + paragraph + button patterns |
| **`FormField`** | `label`, `helper?`, `error?`, `children` (the input) | `.text-field` |
| **`SegmentedControl`** | `options`, `value`, `onChange` | `.segmented` |
| **`ChipRail`** | `chips`, `selected`, `onSelect`, `scrollable?` | `.carousel-nav`, filter rails (new) |
| **`ActionRow`** | `actions: Button[]`, `align: start\|center\|space-between` | `.row` containing buttons, `.dialog-actions`, bundle `.footer-actions` |
| **`KvpGrid`** | `entries: [label, value][]`, `columns?` | repeated `.row.label` + value pairs |
| **`CTACard`** | `icon?`, `eyebrow?`, `title`, `body`, `cta: Button` | the "📷 Scan QR" card pattern, the empty-state-with-action pattern |
| **`ActivityFeed`** | `lines: string[]`, `mono?`, `maxHeight?`, `fadeTop?` | the inline `<ul>` at `identity_centre.rs:791–803` |
| **`CredentialTokenMark`** | `tone: cyan\|purple\|green`, `size` | bundle `.credential-token`; used as the visual on credential cards + DID rows |
| **`AmbientBlobBg`** | `tones: [colour, ...]`, `intensity?` | bundle `.ambient`; reused as section backdrops |

### B.3 Organisms

- **`TopBar`** — logo, current-tab eyebrow, network selector, overflow `IconButton`. Replaces `.header` + `.header-subtitle`.
- **`BottomTabNav`** — five primary tabs (Wallet · DIDs · Identity · Diagnostics · Settings) with icons + labels, the floating "More" menu for Keys / Bootstrap / dev tabs. Replaces `.tab-nav` + `.menu-dropdown`.
- **`CredentialCard`** — the digital-passport pattern lifted to the base organism: ambient blobs · topbar (eyebrow + `IconButton`) · hero (trust-row + h1 + subtitle + `CredentialTokenMark`) · `ChipRail` · `MetaGrid` · claims list · footer actions. Schemas plug in claim renderers via a variant slot.
- **`DIDListItem`** — `Avatar` (mono-token) · DID truncated mono · `MetaGrid` (counter / VMs / services / height) · status `Chip` · trailing `IconButton`.
- **`PickerModal`** — backdrop · dialog · `SectionHeader` · scrollable list of rows · `ActionRow`. Generic over row content. Replaces `.dialog-scrim` + `.did-picker-*`.
- **`FlowStatusBanner`** — sticky banner at top of a screen during a long-running flow, with `Spinner` + label + cancel `IconButton`. Bootstrap busy state, OID4VP in-flight, etc.
- **`ActivityLogPanel`** — virtualised list of `LogRow`s, level filter chips, search input. Lifts `LogsTab` body without disturbing the row layout.
- **`StatPanel`** — counter pills · latency tables · sparklines. Lifts `MetricsTab`.

### B.4 Templates

- **`TabShell`** — vertical scroll container, max-width 560 px, `TopBar` · optional `SectionHeader` hero · content slot · `BottomTabNav` (sticky).
- **`ListShell`** — `TabShell` with header containing `Heading` + `ChipRail` filter + content list slot + optional FAB.
- **`DetailShell`** — `TabShell` with header containing back `IconButton` + `Heading` + actions slot + content slot.
- **`ModalShell`** — backdrop · centred dialog · `SectionHeader` · content slot · `ActionRow`. Generic for all modals.

### B.5 Pages

| Page | Template | Composition |
|---|---|---|
| Wallet | `TabShell` | Hero (`StatusRow` + balance `Heading` + currency `Chip`) → `MetaItem` row (address) → `CTACard` (Connect / Send / Receive) → `KvpGrid` (network · sync stats) |
| DIDs | `ListShell` | Filter `ChipRail` (All/Active/Pending/Deactivated) → list of `DIDListItem` → FAB (Create DID) |
| DID detail | `DetailShell` | Back + DID heading → `ChipRail` (Resolve/Ops/...) → carousel of sub-page organisms |
| Identity Centre | `TabShell` | Hero scan `CTACard` → `OutcomeBanner` (if any) → VC inventory section (`SectionHeader` + count + filter + `CredentialCard`s) |
| Bootstrap | `TabShell` | Eyebrow OPERATOR · BOOTSTRAP → Hero (`Heading` + description) → `CTACard` (bootstrap DID) + live `ActivityFeed` → disclosure (paste OID4VP) |
| Keys | `ListShell` | `SegmentedControl` (curve) → grouped `KeyCard`s by purpose |
| Diagnostics | `TabShell` | `ChipRail` (Probes/Metrics/Bench/Test/Logs) with badge dots → carousel of sub-organisms |
| Settings | `TabShell` | Sectioned: Wallet · Security (`DangerCard` for backup) · Developer · About |

---

## C. Design tokens

The bundle's tokens are the target. Below is the **wallet-flavoured set** with concrete values; install as CSS custom properties on `:root`.

### C.1 Colour

**Surfaces** (darkest → lightest):
```
--surface-0:        #070b14   /* page background, behind everything */
--surface-1:        #0b1020   /* tab content background */
--surface-2:        rgba(16, 24, 43, 0.82)   /* card */
--surface-3:        rgba(22, 33, 57, 0.94)   /* elevated card, modal */
--surface-4:        rgba(255, 255, 255, 0.045) /* hover tint, claim background */
```

**Text**:
```
--text-strong:      #f8fafc   /* headings, balances, primary value */
--text:             #e2e8f0   /* default body */
--text-soft:        #cbd5e1   /* secondary body, subtitle */
--text-muted:       #94a3b8   /* labels, eyebrows, meta */
--text-faint:       #64748b   /* hints, footer, placeholder */
```

**Accent / Status**:
```
--accent-cyan:      #35d3ff
--accent-purple:    #b99cff
--accent-green:     #43f2b2
--success:          #43f2b2   /* alias of green */
--warn:             #ffb020
--error:            #ff557f
--info:             #35d3ff   /* alias of cyan */
```

**Gradients** (use as fills, not borders):
```
--gradient-primary: linear-gradient(135deg, var(--accent-green), var(--accent-cyan))
--gradient-purple:  linear-gradient(135deg, var(--accent-purple), var(--accent-cyan))
--gradient-token:   linear-gradient(145deg, rgba(53,211,255,.24), rgba(185,156,255,.11))
```

**Borders**:
```
--line-faint:       rgba(148, 163, 184, 0.10)
--line:             rgba(148, 163, 184, 0.16)
--line-strong:      rgba(148, 163, 184, 0.28)
--line-danger:      rgba(255, 85, 127, 0.45)
--line-success:     rgba(67, 242, 178, 0.30)
```

### C.2 Spacing scale (4 px base)

| Token | Value | Use |
|---|---|---|
| `--sp-1` | 4 px | chip internal, tight inline gap |
| `--sp-2` | 8 px | row gap, button internal gutter |
| `--sp-3` | 12 px | meta-grid gap, claim-card gap |
| `--sp-4` | 16 px | card padding, between rows |
| `--sp-5` | 20 px | section internal padding |
| `--sp-6` | 24 px | card padding (large), between sections |
| `--sp-8` | 32 px | between large sections, hero top margin |
| `--sp-12` | 48 px | page top, after hero |

### C.3 Radius scale

| Token | Value | Use |
|---|---|---|
| `--r-1` | 8 px | input |
| `--r-2` | 12 px | small card, badge container |
| `--r-3` | 16 px | button, icon-button squircle |
| `--r-4` | 22 px | meta-item, claim-card, list-row |
| `--r-5` | 28 px | modal, large card |
| `--r-6` | 36 px | hero card, credential card |
| `--r-pill` | 999 px | chip, segmented button |

### C.4 Typography

**Family.** Keep **Outfit** (the Midnight brand font already loaded via Google Fonts). The bundle uses Inter — they're close cousins, and Outfit's geometric warmth distinguishes Midnight from every other dark-theme wallet. **Recommendation: keep Outfit for headings + body; use `ui-monospace` stack for mono.**

**Heading ramp** (clamp-based, line-height shrinks with size):
```
--fs-hero:    clamp(2.35rem, 7vw, 5rem)    /* h1 — page hero, balance */ line-height: 0.92, tracking: -0.075em
--fs-h1:      clamp(1.75rem, 4vw, 2.5rem)  /* h1 — screen heading */ line-height: 1.05, tracking: -0.04em
--fs-h2:      1.4rem                        /* h2 — section heading */ line-height: 1.2, tracking: -0.03em
--fs-h3:      1.15rem                       /* h3 — card heading */ line-height: 1.3, tracking: -0.02em
--fs-h4:      1.0rem                        /* h4 — list-item heading */
```

**Body**:
```
--fs-body:    0.96rem    /* default */
--fs-meta:    0.85rem    /* meta line, secondary */
--fs-small:   0.78rem    /* hint, footer */
--fs-eyebrow: 0.72rem    /* uppercase tracked */
--fs-mono:    0.85rem    /* monospace, meta-grid values */
```

**Weights**: 400 (body), 500 (label / chip), 700 (h2-h3, eyebrow), 800 (h1, primary-button), 900 (hero number).

**Tracking**: eyebrow `0.14em`, label `0.10em`, default `normal`, heading `-0.02em` to `-0.075em`.

### C.5 Shadow + blur

```
--shadow-card:    0 26px 90px rgba(0,0,0,0.45)               /* card depth */
--shadow-inset:   inset 0 1px 0 rgba(255,255,255,0.08)       /* faint top highlight on dark cards */
--shadow-button:  0 16px 40px rgba(53,211,255,0.16)          /* primary button gradient halo */
--shadow-modal:   0 24px 48px rgba(0,0,0,0.55)
--blur-card:      blur(22px)                                  /* backdrop-filter */
--blur-ambient:   blur(54px)                                  /* ambient blob */
```

**Ambient blobs.** Two per hero card, top-right cyan + bottom-left purple, `260–320 px` radius, opacity `0.7`, `border-radius: 999px`. Match the bundle's `.ambient-one` / `.ambient-two`.

### C.6 Motion

```
--ease-out:     cubic-bezier(0.16, 1, 0.3, 1)        /* enter, modal, button release */
--ease-spring:  cubic-bezier(0.34, 1.56, 0.64, 1)    /* press feedback (existing) */
--dur-fast:     120ms                                 /* hover */
--dur-norm:     180ms                                 /* card transform, chip select */
--dur-slow:     320ms                                 /* modal enter, page transition */
```

**Specific motions**:
- Hover on card: `transform: translateY(-2px)` + border-color shift, `180ms ease-out`.
- Button press: `transform: scale(0.97)`, `220ms spring`.
- Modal enter: backdrop fade-in `200ms ease-out`, dialog `scale(0.96) → 1` + `translateY(8px) → 0` over `320ms ease-out`.
- Outcome banner: slide-in-from-top `220ms ease-out`.

---

## D. Prioritised backlog

40 items, **P0 / P1 / P2** markers. Effort: **S** (≤ 4 h), **M** (½ – 2 days), **L** (3+ days).

### P0 — ships in next 1–2 sessions (foundation + demo path)

**1. Install design-token layer** — *S*
Why: every subsequent item depends on the new tokens. Today's `:root` block at `styles.css:12-42` defines a different palette; we keep the old vars as aliases for the transition, but introduce the new ones in parallel.
Files: `assets/styles.css` `:root`.
Done when: `--surface-{0..4}`, `--text-{strong,…,faint}`, `--accent-{cyan,purple,green}`, `--gradient-primary`, `--r-{1..6,pill}`, `--sp-{1..12}`, `--fs-{hero,…,eyebrow}` all declared; old tokens still resolve (`--bg: var(--surface-0)`, etc.).

**2. Adopt ambient + radial-gradient page background** — *S*
Why: the single biggest visual lift for zero structural cost. Page reads like a designed surface, not a flat dev shell.
Files: `assets/styles.css` `body`.
Done when: `body { background: radial-gradient(circle at 18% 12%, …cyan…) , radial-gradient(78% 24%, …purple…), linear-gradient(180deg, …) }` matches the bundle.

**3. `Button` atom — gradient primary variant** — *M*
Why: the wallet has 4 button visual-classes (`.cta`, `.btn-primary`, `.btn-text`, `.btn-danger`) all dialled to "tonal purple". A single gradient `.btn-primary` for the *one* hero action per screen lifts every tab.
Files: `assets/styles.css` `.btn-primary` block (`435–455`); leave `.cta` for now as an alias.
Done when: `.btn-primary` is gradient green→cyan, dark text, `--shadow-button`, `radius var(--r-3)`. Wallet Connect, IC Scan-QR, Bootstrap-DID all use it.

**4. `Chip` atom — unify status/tier/category/version pills** — *M*
Why: today there are at least six near-duplicate chip-like classes. Collapsing them to one atom + variants is the highest leverage cleanup.
Files: `assets/styles.css` — `.status-pill`, `.did-picker-status--*`, `.log-level-pill`, `.vc-card-passport__tier-chip*`, `.store-badge`, `.op-stat*` → all delegate to a base `.chip` + modifier classes.
Done when: a single `.chip` class with `.chip--{status,tier,category,version}` and `.chip--tone-{success,warn,error,info,purple,muted}` variants is defined; old class names alias to compound classes; every existing call site still renders.

**5. `MetaItem` molecule replacing the seed-blob dump** — *M*
Why: kills the wall-of-monospace UX. Every "label : long string" pair in the wallet becomes a 22 px-radius card with uppercase label, mono value, and break-anywhere wrapping — matches bundle `.meta-item`.
Files: new `.meta-item` rule in CSS; replace ad-hoc `<div class="row label">` + `<div class="seed-blob">` patterns.
Done when: rule defined; demo replacement done in the Bootstrap outcome (`identity_centre.rs:805–820`).

**6. `OutcomeBanner` atom** — *M*
Why: `wizard-outcome ok|err` is the same shape repeated in 6 places; lift to one component with three tones + optional copy/action.
Files: new `.outcome-banner` + `.outcome-banner--{ok,err,warn,info}` rules; deprecate `.wizard-outcome.{ok,err}`.
Done when: defined; in-place replacement at `identity_centre.rs` Bootstrap / Scan-QR / OID4VCI / OID4VP outcome blocks (4 call sites).

**7. Card primitive lift — 22 px radius, ambient inset, depth** — *M*
Why: `.card` today is a 12 px flat surface. Lift to the bundle's card grammar so every existing `.card` site rides the upgrade.
Files: `assets/styles.css:208-214, 719-735`.
Done when: `.card { border-radius: var(--r-4); background: linear-gradient(180deg, rgba(255,255,255,.045), transparent 28%), var(--surface-2); box-shadow: var(--shadow-card), var(--shadow-inset); border-color: var(--line-strong); backdrop-filter: var(--blur-card); }`.

**8. Wallet hero — balance as the page subject** — *M*
Why: the wallet's most-viewed surface deserves the bundle's h1 typography.
Files: `app.rs:8276` `BalancesCard` + `assets/styles.css` `.balance-row`.
Done when: NIGHT balance renders at `--fs-hero` weight-900 with currency unit at 0.4× and a subtitle line; DUST is the secondary balance at `--fs-h2`.

**9. Wallet `AddressCard` collapse + QR icon-button** — *S*
Why: full-width mono address eats half the screen real estate.
Files: `app.rs:8247–8270`.
Done when: address truncated to `did:midnight…7fd5e` (first 12 + last 6) by default; trailing `IconButton` reveals full + shows inline QR placeholder.

**10. Identity Centre — hero Scan-QR card** — *M*
Why: the single most important user action in the wallet is currently row 2.
Files: `identity_centre.rs:459` (`ScanQrSection`).
Done when: `.card` becomes `.cta-card.cta-card--hero` with gradient backdrop, large QR-frame illustration (inline SVG ok), single primary-gradient CTA.

**11. DID inventory row as 22 px card with mono-token mark** — *M*
Why: aligns the DIDs tab with the new card grammar; the mono-token visual makes "did identity" a recognisable shape.
Files: `assets/styles.css` `.did-inventory-row` + related (search `did-inventory` in styles.css); `app.rs:8037` `DidInventoryPanel`.
Done when: each inventory row is a 22 px-radius card with a small `CredentialTokenMark` on the left + truncated DID + meta-grid + status chip.

**12. `CredentialTokenMark` atom** — *S*
Why: foundational visual primitive reused on DID rows + VC cards + Wallet hero.
Files: new `.token-mark` rule.
Done when: 48 / 64 / 80 / 148 px sizes, gradient inset, optional `--purple` / `--cyan` / `--green` tone variant.

**13. `TabShell` template — common top + bottom + content frame** — *M*
Why: every tab today does its own `style { "{STYLES}" }` + ad-hoc layout. A template gives consistent padding, scroll behaviour, and a place to mount the future `BottomTabNav`.
Files: new wrapper component (Rust-side), but CSS-only change is sufficient for mockups: `.tab-shell { display: flex; flex-direction: column; min-height: 100dvh; padding: 0 var(--sp-4) calc(var(--sp-12) + env(safe-area-inset-bottom)); gap: var(--sp-4); }`.
Done when: rule defined; layout pattern documented.

**14. Bottom tab nav with icons** — *L*
Why: today the nav is a top dropdown + a horizontal `.tab-nav` strip — neither reads like a phone wallet. Bottom nav with 5 icons is the production pattern.
Files: `assets/styles.css` new `.bottom-nav` block; `app.rs:1684–1692`.
Done when: 5 primary tabs (Wallet · DIDs · Identity · Diagnostics · Settings) live in a fixed-bottom 64 px-tall pill bar with icon + label + active-indicator pill behind the icon; overflow goes into "More" → opens dropdown for Keys / Bootstrap / dev tabs.

**15. `SectionHeader` molecule (eyebrow + h2 + actions)** — *S*
Why: replaces 20+ ad-hoc `.card-header` blocks; lets us put a CTA next to a heading uniformly.
Files: new `.section-header` rule.
Done when: rule defined; pattern documented; one in-place replacement in Identity Centre `VcInventorySection` header.

### P1 — lifts the demo materially

**16. `CopyableMonoBlock` molecule with collapse / copy / tap-to-expand** — *M*
Why: shrinks the wall-of-mono in keys / address / VC IDs.
Files: new `.mono-block` rule.
Done when: default shows `first8…last8` + copy `IconButton`; tap expands to full with reflow.

**17. `Avatar` (mono-token small) atom for DID rows** — *S*
Files: rule + Rust component.
Done when: variants for "did" (gradient token), "address" (jazzicon), "user" (initials).

**18. `ChipRail` molecule + filter chip pattern** — *M*
Files: replace `.carousel-nav` block; introduce `.chip-rail`.
Done when: scrollable, scroll-snap-x, active state, optional badge dot per chip.

**19. DID detail header — back button + DID heading + status chip** — *S*
Files: `app.rs` `DidDetailView` header.
Done when: `DetailShell` pattern with back `IconButton`, truncated DID `Heading`, status chip in actions slot.

**20. Bootstrap activity feed → `ActivityFeed` molecule** — *S*
Files: `identity_centre.rs:789–803`, `assets/styles.css`.
Done when: extracted to `.activity-feed` rule; max-height with `mask-image: linear-gradient(to top, black 70%, transparent)` fade-top; new lines pop-in with `--dur-norm`.

**21. Settings sectioning (Wallet / Security / Developer / About)** — *S*
Files: `app.rs:5151` `SettingsTab`.
Done when: section headers separate cards into the four groups; About row at the bottom.

**22. `DangerCard` variant for wallet backup** — *S*
Files: `assets/styles.css` new `.card.card--danger`; `WalletBackupCard` site.
Done when: red-tinted border, lock icon, gated reveal CTA.

**23. Diagnostics per-page `SectionHeader`** — *S*
Files: `app.rs:2120–2256`.
Done when: each carousel page leads with eyebrow + h2 + description; "Last updated" timestamp where relevant.

**24. Logs level filter chips → `Chip --status` variants** — *S*
Files: `assets/styles.css` `.log-level-pill` blocks (1855–1900).
Done when: rules retired; `.log-level-pill` aliases to `.chip.chip--tone-{error,warn,info,debug,trace}` with `.chip--active` state.

**25. Network selector → custom dropdown** — *M*
Why: the native `<select>` element looks like a 90s control next to the new chrome.
Files: `assets/styles.css` `select` block (751–766); rsx for the network selector.
Done when: tap opens a `ChipRail`-like overlay with the four networks; selected one shows in the top bar as a chip.

**26. `PickerModal` template extracted from `did_picker`** — *M*
Why: lifts the wallet's best modal into the reusable shell so future pickers (Currency, Schema, VC for Presentation, …) inherit it.
Files: `assets/styles.css` `.did-picker-*` (2270–2410); refactor classes to `.picker-*`.
Done when: `.picker-modal-backdrop`, `.picker-modal-dialog`, `.picker-modal-list`, etc.; old `.did-picker-*` selectors alias.

**27. Wallet Connect flow → `FlowStatusBanner`** — *S*
Files: `app.rs:1706–1716`.
Done when: while `phase == Connecting`, a sticky banner with spinner + "Connecting to {network}…" sits at top; auto-hides at `Synced`.

**28. Empty states with illustration + CTA** — *M*
Why: every empty state today is grey text ("No DIDs yet. …"). Bump to centred illustration + heading + CTA.
Files: `.empty-state` rule + per-site usages.
Done when: DIDs / Identity Centre VC list / Keys / Logs all have a designed empty-state.

**29. Form fields (`text-field`) → `FormField` molecule with helper + error** — *S*
Files: `.text-field` (815–834).
Done when: floating label, helper text below, error state with red border + `--text-faint`.

**30. Diagnostics chip rail — page-has-new-data badge dot** — *S*
Files: `app.rs:2120–2148`.
Done when: tracked via signal; each chip gets `.chip--badge` when `unread`.

**31. Carousel scroll-progress indicator** — *S*
Files: `.carousel` block (555–575).
Done when: thin 2 px progress bar at the bottom of the carousel-nav reflecting `scrollLeft / scrollMax`.

**32. Wallet send / receive split-button → `ActionRow`** — *M*
Why: today the wallet has Connect + Reload demo + Random wallet in arbitrary stacking. A 3-up "Receive / Send / Swap (soon)" action row is the wallet convention.
Files: `app.rs:1706–1782`.
Done when: 3 icon-labelled `IconButton`s in a row above the balance card.

**33. VC inventory header — count badge + filter rail** — *S*
Files: `identity_centre.rs:1149–1188`.
Done when: header shows "3 credentials" `Chip --count` + chip rail (All / Passport / KYC / Other).

### P2 — future polish

**34. Skeleton loading states** — *M*
Why: today loading shows "Loading…" or "Working…" text; skeleton blocks read calmer.
Done when: every list / card with async data has a 3-row skeleton.

**35. Motion pass — page enter / button press / modal enter** — *M*
Files: `assets/styles.css` global transitions.
Done when: all motion tokens applied per C.6.

**36. A11y pass — focus rings, aria-label, role attrs, contrast check** — *M*
Done when: every IconButton has `aria-label`, every chip has `role="status"` if status-y, lighthouse axe scan returns ≤ 3 minor.

**37. Reduced-motion + reduced-transparency support** — *S*
Done when: `@media (prefers-reduced-motion)` drops all transforms; `@media (prefers-reduced-transparency)` swaps backdrop-filter for solid surface.

**38. Light-theme draft** — *L*
Why: the brand bundle's `:root.color-scheme dark` lets a `light` variant slot in. Not P0, but worth a token-level draft.
Done when: `@media (prefers-color-scheme: light)` block flips token values; visually tested.

**39. `Tooltip` atom replacing `title=` attribute** — *S*
Files: new `.tooltip` JS-less CSS pattern (`::after` + `:focus-visible` + `:hover`).
Done when: pattern documented; first 5 `title=` sites converted.

**40. Splash → page transition** — *S*
Files: `.splash` (148–166).
Done when: splash dissolves into the wallet hero (cross-fade + slight scale-down).

---

## Wins to build on (do NOT regress)

- **Digital passport VC card** (`vc_views/digital_passport.rs` + `.vc-card-passport__*`): the tier chips + per-claim reveal + verify badge are correct; the **CredentialCard organism is essentially this generalised**.
- **DID picker modal** (`did_picker.rs`): the cleanest interaction in the app; lift its language into `PickerModal`.
- **Log row layout** (`.log-row` + `.log-level-pill`): the stamp · level · target · message grammar is solid; only the level-pills migrate to the unified `Chip` atom.
- **Balance value-stack** (`.balance-row .value-stack`): the exact-primary + compact-tag pattern is the right information design; only the typography weights change.
- **Diagnostics carousel** as a tab-collapser pattern (`.carousel-nav` + scroll-snap): keep the *interaction*; lift the chrome.
- **Activity-feed-while-busy** in Bootstrap: keep the *idea* (live ops feedback during long-running flow); lift the visual to the new `ActivityFeed` molecule.

---

## Editorial pick — three highest-impact items to ship first

1. **Item 1 (tokens) + Item 2 (page background) + Item 7 (card primitive lift)** — three small CSS-only items that together visually transform every existing tab without touching a single rsx! line. Ship them as one PR; the demo immediately reads "modern wallet" instead of "engineering dashboard".
2. **Item 10 (Identity Centre Scan-QR hero)** — the demo lives or dies on this surface. A hero scan-QR card with gradient backdrop is what the audience will remember.
3. **Item 14 (bottom tab nav)** — current top-dropdown navigation is the single biggest "phone wallet?" failure cue. Bottom nav makes the wallet look at-home on iOS / Android without any new functionality.
