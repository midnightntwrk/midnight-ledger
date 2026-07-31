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

//! Representation of time and duration
use crate::fab::Aligned;
use crate::fab::Alignment;
use crate::fab::Value;
use crate::fab::ValueAtom;
use serialize::{Deserializable, Serializable, Tagged, tag_enforcement_test};
use std::ops::Add;
use std::ops::AddAssign;
use std::ops::Sub;

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serializable,
    serde::Serialize,
    serde::Deserialize,
)]
#[tag = "timestamp"]
/// Time since Unix Epoch
pub struct Timestamp(u64);
tag_enforcement_test!(Timestamp);

impl Timestamp {
    /// The maximum representable time.
    pub const MAX: Timestamp = Timestamp(u64::MAX);

    /// Creates a timestamp `s` seconds after the start of Unix Epoch
    pub const fn from_secs(s: u64) -> Self {
        Timestamp(s)
    }

    /// Gets the number of seconds since the start of Unix Epoch from the `Timestamp`
    pub fn to_secs(self) -> u64 {
        self.0
    }
}

impl rand::distributions::Distribution<Timestamp> for rand::distributions::Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> Timestamp {
        Timestamp(rng.r#gen())
    }
}

impl Aligned for Timestamp {
    fn alignment() -> Alignment {
        u64::alignment()
    }
}

impl From<Timestamp> for ValueAtom {
    fn from(timestamp: Timestamp) -> ValueAtom {
        ValueAtom::from(timestamp.0).normalize()
    }
}

impl From<Timestamp> for Value {
    fn from(val: Timestamp) -> Value {
        Value(vec![val.into()])
    }
}

impl Sub<Self> for Timestamp {
    type Output = Duration;

    fn sub(self, rhs: Self) -> Self::Output {
        Duration(self.0 as i128 - rhs.0 as i128)
    }
}

impl AddAssign<Duration> for Timestamp {
    fn add_assign(&mut self, rhs: Duration) {
        *self = *self + rhs;
    }
}

impl Add<Duration> for Timestamp {
    type Output = Timestamp;

    fn add(self, rhs: Duration) -> Self::Output {
        if rhs.0 >= 0 {
            let result = self.0.saturating_add(rhs.0 as u64);
            Timestamp(result)
        } else {
            let abs_duration: u64 = rhs
                .0
                .checked_abs()
                .and_then(|val| u64::try_from(val).ok())
                .unwrap_or(u64::MAX);
            Timestamp(self.0.saturating_sub(abs_duration))
        }
    }
}

impl Sub<Duration> for Timestamp {
    type Output = Timestamp;

    fn sub(self, rhs: Duration) -> Self::Output {
        if rhs.0 >= 0 {
            let result = self.0.saturating_sub(rhs.0 as u64);
            Timestamp(result)
        } else {
            let abs_duration: u64 = rhs
                .0
                .checked_abs()
                .and_then(|val| u64::try_from(val).ok())
                .unwrap_or(u64::MAX);
            Timestamp(self.0.saturating_add(abs_duration))
        }
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serializable,
    Hash,
    Default,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(transparent)]
#[tag = "duration"]
/// Some duration of time in seconds
pub struct Duration(i128);
tag_enforcement_test!(Duration);

impl Duration {
    /// Gets the `Duration` from a number of seconds
    pub const fn from_secs(s: i128) -> Self {
        Duration(s)
    }

    /// Gets the `Duration` from a number of hours
    pub const fn from_hours(h: i128) -> Self {
        Duration::from_secs(h * 60 * 60)
    }

    /// Returns the duration's raw value in seconds.
    pub fn as_seconds(self) -> i128 {
        self.0
    }
}

impl Add<Duration> for Duration {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Duration(self.0.saturating_add(rhs.0))
    }
}

impl Sub<Self> for Duration {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Duration(self.0.saturating_sub(rhs.0))
    }
}

// ---------------------------------------------------------------------------
// Area F: totality of `Duration::from_hours`.
//
// `Duration::from_hours(h)` (time.rs:159) computes `h * 60 * 60` with the
// plain `*` operator and no overflow guard. Every other arithmetic operation
// in this module is explicitly *saturating* (`Timestamp::add`/`sub` at
// time.rs:101/119/127, `Duration::add`/`sub` at time.rs:173/181 all use
// `saturating_add`/`saturating_sub`). The unguarded multiplication therefore
// diverges from the module's own arithmetic contract: for `|h| > i128::MAX /
// 3600` the product overflows `i128`, which is a debug-mode panic ("attempt to
// multiply with overflow") and a silent wrap in release. Construction from a
// decoded/attacker-chosen hour count must be *total* (never panic); to stay
// consistent with the rest of the module it should saturate.
// ---------------------------------------------------------------------------
#[cfg(all(test, feature = "proptest"))]
mod from_hours_totality_props {
    use super::Duration;
    use proptest::prelude::*;

    /// The largest hour count for which `h * 3600` does not overflow `i128`.
    const SAFE_MAX_HOURS: i128 = i128::MAX / 3600;

    proptest! {
        // (F-holds) Within the representable range the product is exact and
        // never panics. This half of the property HOLDS today.
        #[test]
        fn from_hours_exact_in_range(h in -SAFE_MAX_HOURS..=SAFE_MAX_HOURS) {
            let d = Duration::from_hours(h);
            prop_assert_eq!(d.as_seconds(), h * 3600);
        }
    }

    // `from_hours` arguments come from decode (`Duration` deserializes over the
    // whole `i128` range), so it must be total for any `i128`.
    proptest! {
        #[test]
        fn from_hours_is_total(h in any::<i128>()) {
            let _ = Duration::from_hours(h);
        }
    }

    // Just past the positive safe range the product saturates to i128::MAX.
    #[test]
    fn from_hours_saturates_at_boundary() {
        assert_eq!(Duration::from_hours(SAFE_MAX_HOURS + 1).as_seconds(), i128::MAX);
    }

    // The negative extreme saturates symmetrically to i128::MIN.
    #[test]
    fn from_hours_saturates_at_negative_boundary() {
        assert_eq!(Duration::from_hours(i128::MIN).as_seconds(), i128::MIN);
    }
}
