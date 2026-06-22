//! Passport-vault contract support.
//!
//! The passport-vault contract (`depositFunds` / `claimFunds` /
//! `adminWithdraw` / `passportPolicyRequest`) is a non-DID Compact
//! contract. This module vendors its compiled circuit artifacts the
//! same way [`crate::did::artifacts`] does for the DID contract, so the
//! shared prove pipeline ([`crate::tx::prove`]) can resolve the
//! per-circuit prover keys by bare circuit name.
//!
//! The compose half (building the `UnprovenTransaction`) runs in the
//! WebView / Node JS layer via `prepareVaultCallTx` (see
//! `dioxus-wallet/web/src/entry.ts` and the test harness), and the
//! native balance/prove/submit half is driven by
//! [`crate::Wallet::call_contract_circuit`].

pub(crate) mod artifacts;
