// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Robustness + canonicity fuzz of the fab (field-aligned binary) `AlignedValue`
//! decoder, which is deserialized from untrusted bytes (onchain-VM contract
//! values / contract state). Over random / bit-flipped / truncated / extended
//! inputs, two independent properties:
//!
//!   * PANIC-SAFETY (`fab_deserialize_is_panic_safe`, ACTIVE): the decoder must
//!     never panic (the custom flagged-int machinery and an `unreachable!` in
//!     `int_size` raise a DoS concern) -- it must return Ok or Err. This holds
//!     today and guards against a future panic regression.
//!
//!   * CANONICITY / accept => canonical (`fab_accept_implies_canonical`): for any
//!     input the decoder ACCEPTS, the bytes it consumed must equal the canonical
//!     re-serialization of the decoded value. The decoder rejects the redundant
//!     forms (a count-1 multi-atom `Value`; a single byte `< 32` encoded in
//!     byte form instead of the short form), so accept implies a unique wire
//!     form.

use midnight_base_crypto::fab::AlignedValue;
use serialize::{Deserializable, Serializable};
use std::io::Cursor;
use std::panic::{AssertUnwindSafe, catch_unwind};

const FUZZ_ITERS: u64 = 30_000;
const MAX_RANDOM_LEN: u64 = 48;
const MAX_BIT_FLIPS: u64 = 5;
const MAX_TRAILING_GARBAGE: u64 = 8;
const SAMPLE_CAP: usize = 6;
const SEED: u64 = 0xFAB1_2345;

/// Tiny deterministic SplitMix64 PRNG (no external rng dependency; not shrinking).
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_add(0x9E37_79B9_7F4A_7C15))
    }
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

fn ser(av: &AlignedValue) -> Vec<u8> {
    let mut b = Vec::new();
    av.serialize(&mut b).unwrap();
    b
}

/// Restores the previous panic hook on drop, so an early `assert!` unwind cannot
/// leave the silencing hook installed for other tests.
struct HookGuard(Option<Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Sync + Send + 'static>>);
impl Drop for HookGuard {
    fn drop(&mut self) {
        if let Some(prev) = self.0.take() {
            std::panic::set_hook(prev);
        }
    }
}

/// Result of one fuzz sweep: sampled panic reports and sampled non-canonical
/// accepts (each capped at `SAMPLE_CAP`), plus accepted/rejected counts.
struct FuzzOutcome {
    panics: Vec<String>,
    noncanon: Vec<String>,
    accepted: u64,
    rejected: u64,
}

fn fuzz_fab() -> FuzzOutcome {
    // Valid encodings to mutate around.
    let bases: Vec<Vec<u8>> = vec![
        ser(&AlignedValue::from(0u8)),
        ser(&AlignedValue::from(42u64)),
        ser(&AlignedValue::from(u128::MAX)),
        ser(&AlignedValue::from(true)),
        ser(&AlignedValue::from([1u8, 2, 3, 4, 5, 6, 7, 8])),
        ser(&AlignedValue::from([0xABu8; 32])),
    ];

    // Silence the panic hook for the duration of the sweep; restored on drop.
    let _guard = HookGuard(Some(std::panic::take_hook()));
    std::panic::set_hook(Box::new(|_| {}));

    let mut rng = Rng::new(SEED);
    let mut panics: Vec<String> = Vec::new();
    let mut noncanon: Vec<String> = Vec::new();
    let (mut accepted, mut rejected) = (0u64, 0u64);

    for i in 0..FUZZ_ITERS {
        let pick = |rng: &mut Rng| bases[rng.below(bases.len() as u64) as usize].clone();
        let input: Vec<u8> = match i % 4 {
            0 => {
                // pure random bytes
                let n = rng.below(MAX_RANDOM_LEN) as usize;
                (0..n).map(|_| rng.below(256) as u8).collect()
            }
            1 => {
                // byte-flip a valid encoding
                let mut b = pick(&mut rng);
                let flips = 1 + rng.below(MAX_BIT_FLIPS);
                for _ in 0..flips {
                    if !b.is_empty() {
                        let p = rng.below(b.len() as u64) as usize;
                        b[p] = rng.below(256) as u8;
                    }
                }
                b
            }
            2 => {
                // truncate a valid encoding
                let b = pick(&mut rng);
                let cut = rng.below(b.len().max(1) as u64) as usize;
                b[..cut].to_vec()
            }
            _ => {
                // extend a valid encoding with trailing garbage
                let mut b = pick(&mut rng);
                for _ in 0..rng.below(MAX_TRAILING_GARBAGE) {
                    b.push(rng.below(256) as u8);
                }
                b
            }
        };

        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let mut cur = Cursor::new(&input[..]);
            match AlignedValue::deserialize(&mut cur, 0) {
                Err(_) => None,
                Ok(av) => {
                    let consumed = cur.position() as usize;
                    let mut re = Vec::new();
                    av.serialize(&mut re).unwrap();
                    Some((consumed, re))
                }
            }
        }));

        match outcome {
            Err(_) => {
                if panics.len() < SAMPLE_CAP {
                    panics.push(format!("input #{i} ({} bytes): {:02x?}", input.len(), input));
                }
            }
            Ok(None) => rejected += 1,
            Ok(Some((consumed, re))) => {
                accepted += 1;
                // accept => canonical: consumed bytes must equal canonical re-serialization.
                if consumed > input.len() || input[..consumed] != re[..] {
                    if noncanon.len() < SAMPLE_CAP {
                        noncanon.push(format!(
                            "input #{i}: consumed {consumed} bytes {:02x?} but canonical re-serialize is {:02x?}",
                            &input[..consumed.min(input.len())],
                            re
                        ));
                    }
                }
            }
        }
    }

    FuzzOutcome { panics, noncanon, accepted, rejected }
}

#[test]
fn fab_deserialize_is_panic_safe() {
    let out = fuzz_fab();
    eprintln!("[fab fuzz] accepted={} rejected={} panics={}", out.accepted, out.rejected, out.panics.len());
    assert!(
        out.panics.is_empty(),
        "fab AlignedValue::deserialize PANICKED (DoS) on:\n{}",
        out.panics.join("\n")
    );
}

#[test]
fn fab_accept_implies_canonical() {
    let out = fuzz_fab();
    eprintln!("[fab fuzz] accepted={} rejected={} noncanon={}", out.accepted, out.rejected, out.noncanon.len());
    assert!(
        out.noncanon.is_empty(),
        "fab AlignedValue::deserialize accepted NON-CANONICAL encoding(s):\n{}",
        out.noncanon.join("\n")
    );
}
