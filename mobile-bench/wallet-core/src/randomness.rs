//! `Randomness` port — wraps the OS entropy source so tests can
//! pin the byte stream to a deterministic ChaCha-seeded RNG.
//!
//! Why a port: per-DID controller secrets, OID4VCI nonces, and
//! anywhere else the wallet pulls fresh entropy currently goes
//! through `rand::thread_rng()` or `rand::rngs::OsRng` inline.
//! Headless integration tests that need reproducible runs
//! (e.g. "the same input always produces the same VC URI")
//! can't fix the entropy without a port.
//!
//! See design doc §2.3 (`Randomness` port).

use std::sync::Mutex;

use rand::RngCore;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

/// Object-safe entropy source.  Bounded to two operations the
/// wallet actually needs: fill a buffer, draw one u64.  No
/// `gen_range`, no distributions — those are caller concerns.
pub trait Randomness: Send + Sync + 'static {
    fn fill_bytes(&self, buf: &mut [u8]);
    fn next_u64(&self) -> u64;
}

/// Production adapter — pulls from `rand::rngs::OsRng`.  Each
/// method opens a fresh `OsRng` (it's a zero-size handle on
/// every platform we ship on) so there's no shared mutable
/// state and the impl is trivially `Send + Sync`.
#[derive(Debug, Default, Clone, Copy)]
pub struct OsRandomness;

impl Randomness for OsRandomness {
    fn fill_bytes(&self, buf: &mut [u8]) {
        rand::rngs::OsRng.fill_bytes(buf);
    }
    fn next_u64(&self) -> u64 {
        rand::rngs::OsRng.next_u64()
    }
}

/// Test adapter — ChaCha20-seeded; bytes are reproducible given
/// the same seed.  Used by per-service tests + the headless
/// `--mock-chain` mode to keep state stable across runs.
///
/// `Mutex<ChaCha20Rng>` because `RngCore::fill_bytes` mutates
/// the internal stream cursor.  The mutex is uncontended in
/// practice (one test at a time per `DeterministicRng`); the
/// lock cost is negligible compared to the entropy draw.
pub struct DeterministicRng {
    inner: Mutex<ChaCha20Rng>,
}

impl DeterministicRng {
    /// Build with an explicit 32-byte seed.  Reproducibility
    /// across processes depends on this seed.
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            inner: Mutex::new(ChaCha20Rng::from_seed(seed)),
        }
    }

    /// Build with `seed = [0u8; 32]` — the "default test
    /// entropy" all-zero seed.  Sufficient for most fixture
    /// tests where the exact bytes don't matter, only that
    /// reruns produce the same bytes.
    pub fn zero_seed() -> Self {
        Self::from_seed([0u8; 32])
    }
}

impl Randomness for DeterministicRng {
    fn fill_bytes(&self, buf: &mut [u8]) {
        if let Ok(mut g) = self.inner.lock() {
            g.fill_bytes(buf);
        }
    }
    fn next_u64(&self) -> u64 {
        match self.inner.lock() {
            Ok(mut g) => g.next_u64(),
            Err(_) => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_randomness_yields_non_zero_bytes() {
        let r = OsRandomness;
        let mut buf = [0u8; 32];
        r.fill_bytes(&mut buf);
        // The probability of OsRng producing 32 zero bytes is
        // 2^-256 — treat any all-zero buffer as a broken impl.
        assert!(buf.iter().any(|&b| b != 0), "OsRng filled with all zeros");
    }

    #[test]
    fn deterministic_rng_is_deterministic() {
        let a = DeterministicRng::from_seed([7u8; 32]);
        let b = DeterministicRng::from_seed([7u8; 32]);
        let mut ba = [0u8; 64];
        let mut bb = [0u8; 64];
        a.fill_bytes(&mut ba);
        b.fill_bytes(&mut bb);
        assert_eq!(ba, bb, "same seed must yield same byte stream");
    }

    #[test]
    fn deterministic_rng_advances_internal_cursor() {
        let r = DeterministicRng::from_seed([1u8; 32]);
        let first = {
            let mut buf = [0u8; 32];
            r.fill_bytes(&mut buf);
            buf
        };
        let second = {
            let mut buf = [0u8; 32];
            r.fill_bytes(&mut buf);
            buf
        };
        assert_ne!(first, second, "second draw must advance the stream");
    }

    #[test]
    fn deterministic_rng_zero_seed_is_stable() {
        let r1 = DeterministicRng::zero_seed();
        let r2 = DeterministicRng::zero_seed();
        let n1 = r1.next_u64();
        let n2 = r2.next_u64();
        assert_eq!(n1, n2);
    }

    #[test]
    fn different_seeds_diverge() {
        let a = DeterministicRng::from_seed([0u8; 32]);
        let b = DeterministicRng::from_seed([1u8; 32]);
        assert_ne!(a.next_u64(), b.next_u64());
    }
}
