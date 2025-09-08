// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serializable, Hash, Default)]
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
