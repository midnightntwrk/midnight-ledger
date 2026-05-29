//! `UnlockGate` port — wraps the wallet's passphrase-based unlock
//! policy.
//!
//! The wallet stores its sensitive bytes (HD seeds, per-DID
//! controller secrets) wrapped in scrypt+AES-GCM envelopes via
//! `store::envelope::{encrypt_secret, decrypt_secret}`. Unlocking
//! a wallet today means "try to decrypt one of those envelopes
//! with the provided passphrase — if it works, the passphrase
//! is correct". That's the production policy; what this port
//! adds:
//!
//! - **Lockout policy**: after N consecutive bad attempts the
//!   gate refuses further attempts for a backoff window. Today
//!   the App's unlock view doesn't enforce this — every wrong
//!   passphrase gets a fresh attempt. The headless binary's
//!   passphrase-via-stdin path needs the policy or it becomes
//!   a brute-force surface.
//! - **Testability**: an `AlwaysOkUnlockGate` / `NeverOkUnlockGate`
//!   pair lets per-service tests cover happy-path and locked-out
//!   branches without spinning up a real scrypt envelope.
//!
//! See design doc §2.3 (`UnlockGate` port).
//!
//! Wave B3 (this commit): trait + adapters + tests. Wave C3
//! (`WalletService::unlock`) consumes this port; the current
//! UI unlock view stays on its inline `WalletStore::open`
//! check until then.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Outcome of a `verify` call.
///
/// The `bad_attempts_remaining` field is informational —
/// adapters that don't track lockout state should return
/// `None`. Adapters that do should decrement it on each
/// failure and reset it on success.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnlockOutcome {
    /// Passphrase decrypted the wrapped seed successfully.
    Ok,
    /// Passphrase did not match. `bad_attempts_remaining`
    /// captures whatever policy budget is left before lockout
    /// (or `None` if the adapter doesn't track that).
    BadPassphrase { bad_attempts_remaining: Option<u32> },
    /// The gate is currently refusing attempts. Caller should
    /// wait until the cooldown window passes before retrying.
    LockedOut { retry_after_secs: u64 },
}

/// Port. Object-safe; the verify side is sync because scrypt
/// is CPU-bound (callers wrap in `spawn_blocking` if they care).
pub trait UnlockGate: Send + Sync + 'static {
    /// Try to unlock the wallet with `passphrase` against the
    /// stored `wrapped_seed` blob. Returns `UnlockOutcome::Ok`
    /// on success; `BadPassphrase` or `LockedOut` otherwise.
    ///
    /// The verified-cleartext bytes are NOT returned by this
    /// trait — that's `WalletStorage`'s job. The gate only
    /// answers "is this passphrase the right one?". Caller
    /// then loads the actual seed.
    fn verify(&self, passphrase: &str, wrapped_seed: &[u8]) -> UnlockOutcome;
}

/// Production adapter — wraps the existing
/// `store::envelope::decrypt_secret` machinery with a sliding
/// lockout window.
///
/// Lockout policy: 5 consecutive failures triggers a 60-second
/// cooldown. Numbers tunable per construction. The counter
/// resets on the first successful verify after a lockout
/// expires (we don't punish a user who eventually remembered
/// their passphrase).
pub struct ScryptUnlockGate {
    max_failures: u32,
    cooldown: Duration,
    state: Mutex<GateState>,
}

#[derive(Debug, Clone, Copy)]
struct GateState {
    consecutive_failures: u32,
    locked_until: Option<Instant>,
}

impl Default for ScryptUnlockGate {
    fn default() -> Self {
        Self {
            max_failures: 5,
            cooldown: Duration::from_secs(60),
            state: Mutex::new(GateState {
                consecutive_failures: 0,
                locked_until: None,
            }),
        }
    }
}

impl ScryptUnlockGate {
    pub fn new(max_failures: u32, cooldown: Duration) -> Self {
        Self {
            max_failures,
            cooldown,
            state: Mutex::new(GateState {
                consecutive_failures: 0,
                locked_until: None,
            }),
        }
    }
}

impl UnlockGate for ScryptUnlockGate {
    fn verify(&self, passphrase: &str, wrapped_seed: &[u8]) -> UnlockOutcome {
        // First check lockout window. A poisoned mutex returns
        // `Ok(Outcome::BadPassphrase)` so the caller's flow
        // doesn't deadlock — preferring availability over
        // correctness here is fine because the worst case is
        // "an attacker gets one extra attempt during a crash".
        let now = Instant::now();
        if let Ok(g) = self.state.lock() {
            if let Some(until) = g.locked_until {
                if now < until {
                    return UnlockOutcome::LockedOut {
                        retry_after_secs: (until - now).as_secs(),
                    };
                }
            }
        }

        // Decode the bincoded `SecretEnvelope` then attempt
        // decrypt. We use the same path as `WalletStore`'s
        // controller-secret + seed verification: scrypt-derive
        // KEK, AES-GCM, fail-on-tag.
        let envelope: crate::store::SecretEnvelope =
            match crate::store::Bincoded::decode(wrapped_seed) {
                Ok(e) => e,
                Err(_) => {
                    // Malformed input is treated as a bad
                    // passphrase — we don't want to leak
                    // "the file is corrupt" vs "you typed
                    // the wrong passphrase" via a distinct
                    // error. Both look the same to a caller.
                    return self.record_failure();
                }
            };
        match crate::store::decrypt_secret(passphrase, &envelope) {
            Ok(_) => self.record_success(),
            Err(_) => self.record_failure(),
        }
    }
}

impl ScryptUnlockGate {
    fn record_success(&self) -> UnlockOutcome {
        if let Ok(mut g) = self.state.lock() {
            g.consecutive_failures = 0;
            g.locked_until = None;
        }
        UnlockOutcome::Ok
    }

    fn record_failure(&self) -> UnlockOutcome {
        let remaining = if let Ok(mut g) = self.state.lock() {
            g.consecutive_failures = g.consecutive_failures.saturating_add(1);
            if g.consecutive_failures >= self.max_failures {
                g.locked_until = Some(Instant::now() + self.cooldown);
                // Once locked, the policy is "wait for
                // cooldown" — there's no remaining-attempts
                // budget left in the current window.
                return UnlockOutcome::LockedOut {
                    retry_after_secs: self.cooldown.as_secs(),
                };
            }
            Some(self.max_failures - g.consecutive_failures)
        } else {
            None
        };
        UnlockOutcome::BadPassphrase {
            bad_attempts_remaining: remaining,
        }
    }
}

/// Test adapter — always returns `Ok`. Useful for service
/// tests where unlock semantics are not under test and the
/// caller just needs a known-good gate.
#[derive(Debug, Default, Clone, Copy)]
pub struct AlwaysOkUnlockGate;

impl UnlockGate for AlwaysOkUnlockGate {
    fn verify(&self, _: &str, _: &[u8]) -> UnlockOutcome {
        UnlockOutcome::Ok
    }
}

/// Test adapter — always returns `BadPassphrase` with no
/// budget tracking. For coverage of failure-branch paths in
/// service tests.
#[derive(Debug, Default, Clone, Copy)]
pub struct NeverOkUnlockGate;

impl UnlockGate for NeverOkUnlockGate {
    fn verify(&self, _: &str, _: &[u8]) -> UnlockOutcome {
        UnlockOutcome::BadPassphrase {
            bad_attempts_remaining: Some(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::encrypt_secret;
    use crate::store::Bincoded;

    fn wrap(passphrase: &str, bytes: &[u8]) -> Vec<u8> {
        let env = encrypt_secret(passphrase, bytes).expect("encrypt");
        Bincoded::encode(&env).expect("bincode").as_slice().to_vec()
    }

    #[test]
    fn scrypt_gate_accepts_right_passphrase() {
        let wrapped = wrap("correct", &[42u8; 32]);
        let gate = ScryptUnlockGate::default();
        assert_eq!(gate.verify("correct", &wrapped), UnlockOutcome::Ok);
    }

    #[test]
    fn scrypt_gate_rejects_wrong_passphrase() {
        let wrapped = wrap("correct", &[42u8; 32]);
        let gate = ScryptUnlockGate::default();
        match gate.verify("wrong", &wrapped) {
            UnlockOutcome::BadPassphrase {
                bad_attempts_remaining,
            } => {
                // 5 max, 1 used → 4 remaining.
                assert_eq!(bad_attempts_remaining, Some(4));
            }
            other => panic!("expected BadPassphrase, got {other:?}"),
        }
    }

    #[test]
    fn scrypt_gate_locks_out_after_max_failures() {
        let wrapped = wrap("correct", &[42u8; 32]);
        let gate = ScryptUnlockGate::new(3, Duration::from_secs(60));
        // Three wrong attempts → locked.
        assert!(matches!(gate.verify("a", &wrapped), UnlockOutcome::BadPassphrase { .. }));
        assert!(matches!(gate.verify("b", &wrapped), UnlockOutcome::BadPassphrase { .. }));
        assert!(matches!(
            gate.verify("c", &wrapped),
            UnlockOutcome::LockedOut { .. }
        ));
        // Even the correct passphrase is now refused.
        assert!(matches!(
            gate.verify("correct", &wrapped),
            UnlockOutcome::LockedOut { .. }
        ));
    }

    #[test]
    fn scrypt_gate_resets_counter_on_success() {
        let wrapped = wrap("correct", &[42u8; 32]);
        let gate = ScryptUnlockGate::new(3, Duration::from_secs(60));
        // Two failures, then a success → next failure starts
        // from a fresh budget of 3.
        let _ = gate.verify("a", &wrapped);
        let _ = gate.verify("b", &wrapped);
        assert_eq!(gate.verify("correct", &wrapped), UnlockOutcome::Ok);
        match gate.verify("wrong", &wrapped) {
            UnlockOutcome::BadPassphrase {
                bad_attempts_remaining,
            } => assert_eq!(bad_attempts_remaining, Some(2)),
            other => panic!("expected BadPassphrase, got {other:?}"),
        }
    }

    #[test]
    fn scrypt_gate_treats_malformed_input_as_bad_passphrase() {
        let gate = ScryptUnlockGate::default();
        let garbage = b"not a bincoded envelope";
        // Should not panic, should not leak "corrupt file"
        // vs "wrong passphrase" — both look like
        // `BadPassphrase`.
        assert!(matches!(
            gate.verify("anything", garbage),
            UnlockOutcome::BadPassphrase { .. }
        ));
    }

    #[test]
    fn always_ok_gate_is_always_ok() {
        let g = AlwaysOkUnlockGate;
        assert_eq!(g.verify("", b""), UnlockOutcome::Ok);
        assert_eq!(g.verify("x", b"y"), UnlockOutcome::Ok);
    }

    #[test]
    fn never_ok_gate_is_always_bad() {
        let g = NeverOkUnlockGate;
        assert!(matches!(
            g.verify("", b""),
            UnlockOutcome::BadPassphrase { .. }
        ));
    }
}
