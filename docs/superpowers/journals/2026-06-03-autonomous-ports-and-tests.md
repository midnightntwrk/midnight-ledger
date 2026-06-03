# 2026-06-03 — autonomous ports + wallet test coverage

Session opened with the user explicitly delegating: "port `Login with DID`
from the mocked issuer to the didit issuer," "port the REST API we have in
didit issuer to the mocked issuer," "wallet hex architecture hardening,"
"add better composability, tests coverage," and "iterate a couple of hours."

This journal records what landed, what didn't, and why.

## TL;DR

| | Repo | Branch | Pushed | What |
|---|---|---|---|---|
| 1 | `yshyn-iohk/midnight-ledger` | `dioxus-vc-demo` | ✅ `dec229c5` | wallet OID4VCI passport-issuer response parser + tests |
| 2 | `midnightntwrk/midnight-identity-solution-examples` | `feat/issuer-demo-polish` | ✅ `1ecda31..3fd6ca4` | digital-passport endpoints in the IssuerDIDIT-mock (behind `ENABLE_DIGITAL_PASSPORT`) |
| 3 | `midnightntwrk/midnight-identity-workspace` | `midnight-ssi-demo-patenv` | ✅ `c0c03d8` | DIDIT_* env-var fallback in `run-demo.sh` for switching to real Didit |
| 4 | `yshyn-iohk/midnight-passport-kyc` | `feat/oid4vp-login` | ⏳ in flight | OID4VP "Login with DID" port to passport-issuer (subagent) |

All pushed commits are GPG-signed + DCO. Subagent #1 (item 4) was still
running when this journal was written; its result is captured below if
it completed.

## What got shipped — commit-by-commit

### Wallet (`yshyn-iohk/midnight-ledger`)

**`a3ccd9b6` — fix(oid4vci): parse passport-issuer credentialPrivateParts shape**

The merged `DigitalPassportResponse` parser expected `openings:
Vec<{ fieldName, plaintextB64, openingB64 }>` at the top level.
The passport-issuer actually wraps the openings under
`credentialPrivateParts.{ claimValues, openings }` with field-named
keys (`firstNameValuePadded`, `firstNameOpening`, etc.). With the
old shape, `serde_json::from_str` silently set `openings = vec![]`
(via `#[serde(default)]`), the wallet stored the VC with zero
openings, and the UI surfaced "No opening stored — cannot reveal"
for every claim. Fixed; the wallet now re-keys the issuer's named
fields into the wallet's `/credentialSubject/{name}` JSON-Pointer
convention and re-packs the integer `dateOfBirthDays` into 4 u32-LE
bytes for the wallet's existing `decode_days_since_epoch` reader.

**`dec229c5` — test(wallet-core): unblock lib-test target + extend OID4VCI digital-passport coverage**

The `b753e399` merge brought a `run_issuance` signature change that
left the lib-test target uncompilable (5 sites passed `&clock` where
`clock` was `Arc<dyn Clock>`; one site called with 5 args where 9
were needed; three test functions moved values into stub-port
chains before re-borrowing them). Fixed every site, then added four
new test functions over the post-`a3ccd9b6` parser:

- `request_credential_passport_round_trip` — happy path; verifies all
  5 `VcOpening` rows land under the expected paths, opening bytes are
  32 each, plaintext encodings match the wire shape.
- `request_credential_passport_no_private_parts_drops_only_openings`
  — boundary: missing `credentialPrivateParts` doesn't block issuance.
- `date_of_birth_days_roundtrips_through_u32_le_bytes` — closes the
  integer → bytes → integer loop the reveal UI depends on.
- `request_credential_passport_http_call_shape` — Bearer-token threading
  from `/token` into `/credentials`.

All 344 lib tests now pass (up from 0-compilable previously).

### IssuerDIDIT-mock (`feat/issuer-demo-polish`)

Subagent dispatched with full reference brief; landed 4 signed commits:

- `1ecda31` — compact-vc encoders + mock minter (`padTextToBytes32/64`,
  `daysSinceEpoch`, `u32LeBytes`, `mintDigitalPassportCredentialResponse()`)
- `755dd02` — OID4VCI digital-passport endpoints behind feature flag
  (`/.well-known/openid-credential-issuer`, `/api/issuer/credential-offer/:id`,
  `/api/issuer/token`, `/api/issuer/credentials`)
- `7fb8321` — 21 unit + integration tests (all passing)
- `3fd6ca4` — README + caveat about the placeholder `credentialProof`

Both flows coexist: the legacy `/credential` route is untouched (live
demo runs against it), and the new digital-passport flow only mounts
when `ENABLE_DIGITAL_PASSPORT=true`. Zero regression to the running demo.

**Known limitation** (documented in the issuer's README + code comments):
the mock's `credentialProof` is a 64-zero-byte placeholder, not a
real Compact-runtime issuance proof. The wallet's `vc_self_verify`
routes `midnight_compact_vc` through the JS bridge's
`verifyDigitalPassportIssuanceProof` and surfaces
`Invalid(InvalidIssuanceProof(...))`. The VC still lands in the
wallet's `vc_store` and renders correctly in the UI; only the
cryptographic verify step is mocked.

### Workspace (`midnight-ssi-demo-patenv`)

**`08a2907` (rebased to `c0c03d8`)** — `feat(run-demo): DIDIT_*
env-var fallback for real Didit credentials`. The `cmd_start_passport_issuer`
heredoc that generates `scripts/.env.passport-issuer` previously
hardcoded the smocker mock URL + integration-test API key. Switched
those four lines to `${VAR:-<mock-default>}` so an operator can flip
to real Didit by exporting the env vars before running the script.

Tested live tonight on tailnet — wallet completed an OID4VCI
issuance against real `verification.didit.me` end-to-end.

## What I deliberately did NOT do

- **Service-skeleton cleanup (audit §9 item 3).** `wallet-core/src/service/`
  has 945 lines across 11 files; at least one (`controller_secret_service.rs`)
  has live passing tests, so a bulk delete would lose coverage. Needs
  per-file review I couldn't do safely in autonomous mode. Punted.

- **dioxus-wallet prelude sweep (audit §9 item 4).** Mechanical but
  no test value without sustained focus on each migrated `use` line.
  Punted.

- **Port-error-type refactor (audit §9 item E).** Large structural
  change. Out of scope for this block.

- **passport-issuer fork content (subagent #1).** The OID4VP port is
  running. If it completed, see the subagent's final report; if not,
  the branch `feat/oid4vp-login` on `yshyn-iohk/midnight-passport-kyc`
  has partial work — diff against `9183794` (the parent commit) to see
  what landed.

## Self-review

**Things that went well:**

- Both subagents (item 2 = digital-passport port to mock, item 4 =
  OID4VP port to passport-issuer) were briefed with full reference
  pointers, target file paths, push instructions, commit discipline,
  and acceptance criteria. Subagent #2 returned with 21/21 tests
  passing and pushed cleanly — no review iteration needed.
- Wallet test coverage now covers the today-fixed parser path
  end-to-end with both happy-path and boundary cases.
- Pre-existing test compile errors that blocked the entire lib-test
  target got resolved as a side effect (1 site per file, all in
  test-only code, surgical).

**Risks I'm aware of:**

- Subagent #1 may have introduced regressions in passport-issuer's
  existing routes if its diff isn't surgically additive. Read the
  diff before merging to passport-issuer's parent (Pat's `kyc-demo`).
- The IssuerDIDIT-mock digital-passport endpoint emits a placeholder
  proof. Anyone using it for end-to-end demos will see
  `Invalid(InvalidIssuanceProof(...))` in the wallet's
  `Last verified` badge. Documented but easy to forget.
- The wallet's new `paired_wallets` test helper creates two bootstrap
  invocations per test (deterministic via the same seed). This adds
  ~200 ms per test run on a cold cache. Acceptable for now; if it
  becomes a problem, switch to `Arc<Wallet>` + clone.

**Push hygiene:**

- Wallet → `yshyn-iohk/midnight-ledger` (my fork): fast-forward push,
  no force needed.
- Workspace → `midnightntwrk/midnight-identity-workspace`
  `midnight-ssi-demo-patenv`: rebased onto upstream then pushed. Tip
  moved from `3826c38` to `c0c03d8`.
- IssuerDIDIT-mock → subagent's push to
  `midnightntwrk/midnight-identity-solution-examples`
  `feat/issuer-demo-polish`. Verified via `git log` after the
  subagent's report.

All commits GPG-signed (verifiable via `git log --format='%h %G?'`).
All carry the `Signed-off-by:` DCO line via `--signoff`. Each new
commit also carries a `Co-Authored-By:` trailer for the
Claude pair-coding attribution.

## What you'll want to do tomorrow

1. **Review subagent #1's branch** (`yshyn-iohk/midnight-passport-kyc`
   `feat/oid4vp-login`) before merging anywhere. If you want it
   upstreamed to Pat's repo, open a PR `yshyn-iohk:feat/oid4vp-login →
   patextreme:kyc-demo`. The subagent operated with the
   `ENABLE_OID4VP_LOGIN=true` env gate so the existing flow isn't
   touched by default.

2. **Update memory** if my consolidated entries don't match your
   mental model. Three new entries landed tonight in
   `~/.claude/projects/-Users-ysh-iohk-midnight-ledger/memory/`:
   `feedback_smocker_mock_localhost_trap.md`,
   `feedback_dioxus_asset_embedded_in_so.md`,
   `project_passport_issuer_response_shape.md`.

3. **Decide on the audit §9 leftovers** (service-skeleton cleanup,
   prelude sweep, port-error consistency). Those are next-steps
   candidates if you want a focused half-day refactor block.

4. **Consider whether the IssuerDIDIT-mock's placeholder
   credentialProof is acceptable indefinitely** or whether it should
   be replaced with a real Compact-runtime mint. Today's mock is
   useful for response-shape symmetry but doesn't exercise the
   wallet's verify path.
