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

//! This module contains primitives used in the cost model, including the
//! structures used for costing, the time abstraction used in the cost model,
//! and the arithmetic defined over them.

use ethnum::i256;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use rand::{distributions::Standard, prelude::Distribution};
use serde::{Deserialize, Serialize};
use serialize::{Deserializable, Serializable, Tagged};
use std::{
    fmt::Debug,
    iter::Sum,
    ops::{Add, AddAssign, Div, Mul, Neg, Sub},
};

#[derive(
    Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serializable, Serialize, Deserialize, Default,
)]
#[tag = "cost-duration[v1]"]
#[serde(transparent)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
/// A 'costed' time, measured in picoseconds.
pub struct CostDuration(u64);

impl Debug for CostDuration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            1..1_000 => write!(f, "{}ps", self.0),
            1_000..1_000_000 => write!(f, "{:.3}ns", self.0 as f64 / 1e3f64),
            1_000_000..1_000_000_000 => write!(f, "{:.3}μs", self.0 as f64 / 1e6f64),
            1_000_000_000..1_000_000_000_000 => write!(f, "{:.3}ms", self.0 as f64 / 1e9f64),
            _ => write!(f, "{:.3}s", self.0 as f64 / 1e12f64),
        }
    }
}

impl Distribution<CostDuration> for Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> CostDuration {
        CostDuration(self.sample(rng))
    }
}

impl Sum for CostDuration {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        CostDuration(
            iter.map(|i| i.0)
                .reduce(|a, b| a.saturating_add(b))
                .unwrap_or(0),
        )
    }
}

impl AddAssign for CostDuration {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Add for CostDuration {
    type Output = CostDuration;
    fn add(self, rhs: Self) -> Self::Output {
        CostDuration(self.0.saturating_add(rhs.0))
    }
}

impl Mul<CostDuration> for usize {
    type Output = CostDuration;
    fn mul(self, rhs: CostDuration) -> Self::Output {
        CostDuration((self as u64).saturating_mul(rhs.0))
    }
}

impl Mul<CostDuration> for u64 {
    type Output = CostDuration;
    fn mul(self, rhs: CostDuration) -> Self::Output {
        CostDuration(self.saturating_mul(rhs.0))
    }
}

impl Mul<usize> for CostDuration {
    type Output = CostDuration;
    fn mul(self, rhs: usize) -> Self::Output {
        CostDuration(self.0.saturating_mul(rhs as u64))
    }
}

impl Mul<u64> for CostDuration {
    type Output = CostDuration;
    fn mul(self, rhs: u64) -> Self::Output {
        CostDuration(self.0.saturating_mul(rhs))
    }
}

impl Mul<f64> for CostDuration {
    type Output = CostDuration;
    fn mul(self, rhs: f64) -> Self::Output {
        CostDuration((self.0 as f64 * rhs) as u64)
    }
}

impl Mul<CostDuration> for f64 {
    type Output = CostDuration;
    fn mul(self, rhs: CostDuration) -> Self::Output {
        CostDuration((self * rhs.0 as f64) as u64)
    }
}

impl Div for CostDuration {
    type Output = FixedPoint;
    fn div(self, rhs: Self) -> Self::Output {
        FixedPoint::from_u64_div(self.0, rhs.0)
    }
}

impl Div<u64> for CostDuration {
    type Output = CostDuration;
    fn div(self, rhs: u64) -> Self::Output {
        CostDuration(self.0.div_ceil(rhs))
    }
}

#[derive(
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Debug,
    Copy,
    Clone,
    Serializable,
    Serialize,
    Deserialize,
    Default,
)]
#[tag = "synthetic-cost[v1]"]
/// The synthetic (modeled) cost of execution, typically over a transaction or
/// block.
pub struct SyntheticCost {
    #[serde(rename = "readTime")]
    /// The time spent in IO reads
    pub read_time: CostDuration,
    #[serde(rename = "computeTime")]
    /// The time spent in single-threaded compute
    pub compute_time: CostDuration,
    #[serde(rename = "blockUsage")]
    /// The bytes used of block size capacity
    pub block_usage: u64,
    #[serde(rename = "bytesWritten")]
    /// The bytes written persistently to disk.
    /// Unlike in [`RunningCost`], this represents net bytes written, defined for `r: RunningCost`
    /// as `max(0, r.bytes_written - r.bytes_deleted)`
    pub bytes_written: u64,
    #[serde(rename = "bytesChurned")]
    /// The bytes written temporarily or overwritten
    pub bytes_churned: u64,
}

impl Mul<f64> for SyntheticCost {
    type Output = SyntheticCost;
    fn mul(self, rhs: f64) -> Self::Output {
        SyntheticCost {
            compute_time: self.compute_time * rhs,
            read_time: self.read_time * rhs,
            block_usage: (self.block_usage as f64 * rhs).ceil() as u64,
            bytes_written: (self.bytes_written as f64 * rhs).ceil() as u64,
            bytes_churned: (self.bytes_churned as f64 * rhs).ceil() as u64,
        }
    }
}

impl Add for SyntheticCost {
    type Output = SyntheticCost;
    fn add(self, rhs: Self) -> Self::Output {
        SyntheticCost {
            read_time: self.read_time + rhs.read_time,
            compute_time: self.compute_time + rhs.compute_time,
            block_usage: self.block_usage.saturating_add(rhs.block_usage),
            bytes_written: self.bytes_written.saturating_add(rhs.bytes_written),
            bytes_churned: self.bytes_churned.saturating_add(rhs.bytes_churned),
        }
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Copy, Clone)]
/// The costs normalized to a block's limit in each dimension
pub struct NormalizedCost {
    /// The fraction of a block's read time used
    pub read_time: FixedPoint,
    /// The fraction of a block's compute time used
    pub compute_time: FixedPoint,
    /// The fraction of a block's size used
    pub block_usage: FixedPoint,
    /// The fraction of a block's data write allowance used
    pub bytes_written: FixedPoint,
    /// The fraction of a block's data churn allowance used
    pub bytes_churned: FixedPoint,
}

impl SyntheticCost {
    /// The empty cost
    pub const ZERO: SyntheticCost = SyntheticCost {
        read_time: CostDuration::ZERO,
        compute_time: CostDuration::ZERO,
        block_usage: 0,
        bytes_written: 0,
        bytes_churned: 0,
    };

    /// The longest time spent in this cost
    pub fn max_time(&self) -> CostDuration {
        CostDuration::max(self.read_time, self.compute_time)
    }

    /// Normalizes the cost against block limits, returning `None` if they exceed them
    pub fn normalize(self, limits: SyntheticCost) -> Option<NormalizedCost> {
        let res = NormalizedCost {
            read_time: self.read_time / limits.read_time,
            compute_time: self.compute_time / limits.compute_time,
            block_usage: FixedPoint::from_u64_div(self.block_usage, limits.block_usage),
            bytes_written: FixedPoint::from_u64_div(self.bytes_written, limits.bytes_written),
            bytes_churned: FixedPoint::from_u64_div(self.bytes_churned, limits.bytes_churned),
        };
        let vals = [
            &res.read_time,
            &res.compute_time,
            &res.block_usage,
            &res.bytes_written,
            &res.bytes_churned,
        ];
        if vals.into_iter().any(|val| *val > FixedPoint::ONE) {
            None
        } else {
            Some(res)
        }
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone, Serializable)]
#[tag = "fee-prices[v1]"]
/// The pricing of the various block operations
///
/// All values are denominated in DUST (*not* atomic units, or SPECKs)
pub struct FeePrices {
    /// The price in DUST of a block's full read capacity
    pub read_price: FixedPoint,
    /// The price in DUST of a block's full compute capacity
    pub compute_price: FixedPoint,
    /// The price in DUST of a block's full size capacity
    pub block_usage_price: FixedPoint,
    /// The price in DUST of a block's full write allowance capacity
    pub write_price: FixedPoint,
}

impl FeePrices {
    /// Compute an updated cost from a given block fullness. This should be the
    /// sum of the normalized costs of all transactions in a block.
    ///
    /// `min_ratio` specifies a bound that the smallest price will not fall
    /// below, as a ratio of the highest price. It should be `0 < min_ratio < 1`.
    ///
    /// `a` is the `a` parameter from [`price_adjustment_function`].
    pub fn update_from_fullness(
        &self,
        block_fullness: NormalizedCost,
        min_ratio: FixedPoint,
        a: FixedPoint,
    ) -> Self {
        let multiplier = |frac| price_adjustment_function(frac, a) + FixedPoint::ONE;
        let mut updated = FeePrices {
            read_price: self.read_price * multiplier(block_fullness.read_time),
            compute_price: self.compute_price * multiplier(block_fullness.compute_time),
            block_usage_price: self.block_usage_price * multiplier(block_fullness.block_usage),
            write_price: self.write_price
                * multiplier(FixedPoint::max(
                    block_fullness.bytes_written,
                    block_fullness.bytes_churned,
                )),
        };
        let dimensions = [
            &mut updated.read_price,
            &mut updated.compute_price,
            &mut updated.block_usage_price,
            &mut updated.write_price,
        ];
        let most_expensive_dimension = **dimensions
            .iter()
            .max()
            .expect("max of 4 elements must exist");
        // The smallest fixed point cost is *not* MIN_POSITIVE, to ensure that we don't get 'stuck'
        // there and unable to adjust up. Because adjustments are small, single-digit percentages,
        // rounding would keep us at 1 if this was 1.
        const MIN_COST: FixedPoint = FixedPoint(100);
        for dim in dimensions.into_iter() {
            *dim = FixedPoint::max(
                FixedPoint::max(*dim, most_expensive_dimension * min_ratio),
                MIN_COST,
            );
        }
        updated
    }

    /// The overall (dust) cost of a synthetic resource cost, given this
    /// resource price object.
    ///
    /// The final cost is denominated in DUST.
    pub fn overall_cost(&self, tx_normalized: &NormalizedCost) -> FixedPoint {
        let read_cost = self.read_price * tx_normalized.read_time;
        let compute_cost = self.compute_price * tx_normalized.compute_time;
        let block_usage_cost = self.block_usage_price * tx_normalized.block_usage;
        let write_cost = self.write_price * tx_normalized.bytes_written;
        let churn_cost = self.write_price * tx_normalized.bytes_churned;
        let utilization_cost =
            FixedPoint::max(read_cost, FixedPoint::max(compute_cost, block_usage_cost));
        utilization_cost + write_cost + churn_cost
    }
}

#[derive(
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Debug,
    Copy,
    Clone,
    Serializable,
    Serialize,
    Deserialize,
    Default,
)]
#[tag = "running-cost[v1]"]
/// The cost during computation, tracking read time, compute time, and bytes
/// written and deleted.
pub struct RunningCost {
    #[serde(rename = "readTime")]
    /// The time spent reading according to the model
    pub read_time: CostDuration,
    #[serde(rename = "computeTime")]
    /// The time spent in single-threaded compute according to the model
    pub compute_time: CostDuration,
    #[serde(rename = "bytesWritten")]
    /// The number of bytes written according to the model.
    /// Unlike `bytes_written` in [`SyntheticCost`], this one represents the absolute bytes
    /// written, not net bytes written.
    pub bytes_written: u64,
    #[serde(rename = "bytesDeleted")]
    /// The number of bytes deleted according to the model
    pub bytes_deleted: u64,
}

impl RunningCost {
    /// The running cost of zero.
    pub const ZERO: RunningCost = RunningCost {
        read_time: CostDuration::ZERO,
        compute_time: CostDuration::ZERO,
        bytes_written: 0,
        bytes_deleted: 0,
    };

    /// Captures only some compute time
    pub const fn compute(time: CostDuration) -> RunningCost {
        RunningCost {
            read_time: CostDuration::ZERO,
            compute_time: time,
            bytes_written: 0,
            bytes_deleted: 0,
        }
    }

    /// The longest time spent in this cost
    pub fn max_time(&self) -> CostDuration {
        CostDuration::max(self.read_time, self.compute_time)
    }
}

impl Distribution<RunningCost> for Standard {
    fn sample<R: rand::Rng + ?Sized>(&self, rng: &mut R) -> RunningCost {
        RunningCost {
            read_time: self.sample(rng),
            compute_time: self.sample(rng),
            bytes_written: self.sample(rng),
            bytes_deleted: self.sample(rng),
        }
    }
}

impl From<RunningCost> for SyntheticCost {
    fn from(value: RunningCost) -> Self {
        // Convert from absolute bytes written to net bytes written
        let bytes_written = value.bytes_written.saturating_sub(value.bytes_deleted);
        SyntheticCost {
            read_time: value.read_time,
            compute_time: value.compute_time,
            block_usage: 0,
            bytes_written,
            bytes_churned: value.bytes_written - bytes_written,
        }
    }
}

impl Mul<f64> for RunningCost {
    type Output = RunningCost;
    fn mul(self, rhs: f64) -> Self::Output {
        RunningCost {
            compute_time: self.compute_time * rhs,
            read_time: self.read_time * rhs,
            bytes_written: (self.bytes_written as f64 * rhs).ceil() as u64,
            bytes_deleted: (self.bytes_deleted as f64 * rhs).ceil() as u64,
        }
    }
}

impl Mul<usize> for RunningCost {
    type Output = RunningCost;
    fn mul(self, rhs: usize) -> Self::Output {
        self * rhs as u64
    }
}

impl Mul<u64> for RunningCost {
    type Output = RunningCost;
    fn mul(self, rhs: u64) -> Self::Output {
        RunningCost {
            compute_time: self.compute_time * rhs,
            read_time: self.read_time * rhs,
            bytes_written: self.bytes_written.saturating_mul(rhs),
            bytes_deleted: self.bytes_deleted.saturating_mul(rhs),
        }
    }
}

impl Add for RunningCost {
    type Output = RunningCost;
    fn add(self, rhs: Self) -> Self::Output {
        RunningCost {
            read_time: self.read_time + rhs.read_time,
            compute_time: self.compute_time + rhs.compute_time,
            bytes_written: self.bytes_written.saturating_add(rhs.bytes_written),
            bytes_deleted: self.bytes_deleted.saturating_add(rhs.bytes_deleted),
        }
    }
}

impl AddAssign for RunningCost {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl CostDuration {
    /// No cost duration
    pub const ZERO: CostDuration = CostDuration(0);
    /// A second in [`CostDuration`] representation.
    pub const SECOND: CostDuration = CostDuration(1_000_000_000_000);
    /// Initializes this cost duration measurement from raw picoseconds
    pub const fn from_picoseconds(picoseconds: u64) -> CostDuration {
        CostDuration(picoseconds)
    }

    /// The raw picosecond count of this cost duration measurement
    pub const fn into_picoseconds(self) -> u64 {
        self.0
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Serializable)]
#[tag = "fixed-point[v1]"]
/// Represents a rational number deterministically. Internally, numbers are
/// represented by an integer `x: i128`, which represents the real `x / (2 ** 64)`.
///
/// Addition, multiplication, and division are defined; as this is used for cost
/// estimations, addition and multiplication are saturating (because the maximum
/// should always be rejected), and division rounds up.
pub struct FixedPoint(i128);

impl Debug for FixedPoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FixedPoint({})", f64::from(*self))
    }
}

impl From<f64> for FixedPoint {
    fn from(float: f64) -> FixedPoint {
        FixedPoint((float * 2f64.powi(64)) as i128)
    }
}

impl From<FixedPoint> for i128 {
    fn from(fp: FixedPoint) -> i128 {
        fp.0 >> 64
    }
}

impl From<FixedPoint> for f64 {
    fn from(fp: FixedPoint) -> f64 {
        fp.0 as f64 / 2f64.powi(64)
    }
}

// NOTE: For reasoning about arith, convention is that:
// - A/B is the 'real' number
// - a/b is the representation
// - Therefore, A = a / (2 ** 64)

impl FixedPoint {
    /// The value of 0.0
    pub const ZERO: FixedPoint = FixedPoint(0);
    /// The value of 1.0
    pub const ONE: FixedPoint = FixedPoint::from_u64_div(1, 1);
    /// The smallest positive fraction representable in this fixed point representation
    pub const MIN_POSITIVE: FixedPoint = FixedPoint(1);
    /// The maximum representable fixed point number
    pub const MAX: FixedPoint = FixedPoint(i128::MAX);

    /// Takes a [`FixedPoint`] denominated in a non-base token unit (for instance,
    /// 1.0 representing DUST) to it's base unit.
    ///
    /// Conceptually, acts as `self * base_unit` as `u128` with smarter overflow
    /// handling.
    ///
    /// Rounds up, and returns zero for negatives.
    pub fn into_atomic_units(self, base_unit: u128) -> u128 {
        let raw = i256::from(self.0) * i256::from(base_unit);
        let (res, rem) = raw.div_rem(i256::from(1u128 << 64));
        let res = if rem <= 0 { 0 } else { 1 } + res;
        if res < 0 {
            0
        } else if res > i256::from(u128::MAX) {
            u128::MAX
        } else {
            res.as_u128()
        }
    }

    /// Raises the number to an integer power.
    pub fn powi(self, mut exp: i32) -> Self {
        match exp {
            i32::MIN..=-1 => dbg!(FixedPoint::ONE / self).powi(dbg!(-exp)),
            0 => FixedPoint::ONE,
            1..=i32::MAX => {
                let mut acc = FixedPoint::ONE;
                let mut cur = self;
                while exp >= 1 {
                    if exp & 0b1 != 0 {
                        acc = acc * cur;
                    }
                    cur = cur * cur;
                    exp >>= 1;
                }
                acc
            }
        }
    }

    /// Instantiates a fixed point from a/b (rounded up to the nearest 2^-64)
    ///
    /// Unlike [`FixedPoint::from_u128_div`], this method is `const`, due to using only builtin
    /// rust arithmetic operations.
    pub const fn from_u64_div(a: u64, b: u64) -> FixedPoint {
        // C = a / b
        // c / (2 ** 64) = a / b
        // c = a * (2 ** 64) / b
        if b == 0 {
            return FixedPoint(i128::MAX);
        }
        let ashift = (a as u128) << 64;
        let c = ashift.div_ceil(b as u128) as i128;
        FixedPoint(c)
    }

    /// Instantiates a fixed point from a/b (rounded up to the nearest 2^-64)
    pub fn from_u128_div(a: u128, b: u128) -> FixedPoint {
        // C = a / b
        // c / (2 ** 64) = a / b
        // c = a * (2 ** 64) / b
        if b == 0 {
            return FixedPoint(i128::MAX);
        }
        let ashift = i256::from(a) * i256::from(1u128 << 64);
        let (c, rem) = ashift.div_rem(i256::from(b));
        let c = if rem == i256::ZERO {
            i256::from(0u64)
        } else {
            i256::from(1u64)
        } + c;
        if c > i256::from(u128::MAX) {
            FixedPoint(i128::MAX)
        } else {
            FixedPoint(c.as_i128())
        }
    }
}

impl Add for FixedPoint {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        // C = A + B
        // c / (2 ** 64) = (a / (2 ** 64)) + (b / (2 ** 64)) = (a + b) / (2 ** 64)
        // c = a + b
        FixedPoint(self.0.saturating_add(rhs.0))
    }
}

impl Sub for FixedPoint {
    type Output = FixedPoint;
    fn sub(self, rhs: Self) -> Self::Output {
        // C = A - B
        // c / (2 ** 64) = (a / (2 ** 64)) - (b / (2 ** 64)) = (a - b) / (2 ** 64)
        // c = a - b
        FixedPoint(self.0.saturating_sub(rhs.0))
    }
}

impl Neg for FixedPoint {
    type Output = FixedPoint;
    fn neg(self) -> Self::Output {
        FixedPoint(self.0.saturating_neg())
    }
}

impl Mul for FixedPoint {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self::Output {
        // C = A * B
        // (c / (2 ** 64)) = (a / (2 ** 64)) * (b / (2 ** 64)) = (a * b) / (2 ** 128)
        // c = (a * b) / (2 ** 64)
        let ab = i256::from(self.0) * i128::from(rhs.0);
        let c = i256::min(i256::from(i128::MAX), ab >> 64).as_i128();
        FixedPoint(c)
    }
}

impl Div for FixedPoint {
    type Output = Self;
    /// Division rounding up to the nearest 2^-64
    fn div(self, rhs: Self) -> Self::Output {
        // C = A / B
        // C = |A| / |B| * sign(A) * sign(B)
        if rhs.0 == 0 {
            // Rather max out pricing than panic
            return FixedPoint(i128::MAX);
        }
        let a_abs = self.0.unsigned_abs();
        let b_abs = rhs.0.unsigned_abs();
        let a_sign = self.0.signum();
        let b_sign = rhs.0.signum();
        FixedPoint(FixedPoint::from_u128_div(a_abs, b_abs).0 * a_sign * b_sign)
    }
}

/// The raw price adjustment function from fullness, as specified in tokenomics
/// documents
pub fn price_adjustment_function(usage: FixedPoint, a: FixedPoint) -> FixedPoint {
    // Points of the function to linearly interpolate between. 0 is point 0, the
    // final point is 1.
    //
    // As output and enforced by test_price_adjustment_static
    const POINTS: &[FixedPoint] = &[
        FixedPoint(-84764999863455367168), // 0.00 => -4.59512
        FixedPoint(-84764999863455367168), // 0.01 => -4.59512
        FixedPoint(-71791413020114739200), // 0.02 => -3.89182
        FixedPoint(-64122702906348363776), // 0.03 => -3.47610
        FixedPoint(-58624745660900909056), // 0.04 => -3.17805
        FixedPoint(-54315312289337933824), // 0.05 => -2.94444
        FixedPoint(-50756867729459126272), // 0.06 => -2.75154
        FixedPoint(-47715996328766365696), // 0.07 => -2.58669
        FixedPoint(-45053350700638961664), // 0.08 => -2.44235
        FixedPoint(-42679031418590216192), // 0.09 => -2.31363
        FixedPoint(-40531639450585882624), // 0.10 => -2.19722
        FixedPoint(-38567365939524018176), // 0.11 => -2.09074
        FixedPoint(-36753849332779208704), // 0.12 => -1.99243
        FixedPoint(-35066499762404089856), // 0.13 => -1.90096
        FixedPoint(-33486189434148528128), // 0.14 => -1.81529
        FixedPoint(-31997741738730885120), // 0.15 => -1.73460
        FixedPoint(-30588908944945000448), // 0.16 => -1.65823
        FixedPoint(-29249660330515177472), // 0.17 => -1.58563
        FixedPoint(-27971674063185141760), // 0.18 => -1.51635
        FixedPoint(-26747966611833823232), // 0.19 => -1.45001
        FixedPoint(-25572617290405310464), // 0.20 => -1.38629
        FixedPoint(-24440560042528645120), // 0.21 => -1.32493
        FixedPoint(-23347423671542177792), // 0.22 => -1.26567
        FixedPoint(-22289407577085239296), // 0.23 => -1.20831
        FixedPoint(-21263183918842343424), // 0.24 => -1.15268
        FixedPoint(-20265819725292941312), // 0.25 => -1.09861
        FixedPoint(-19294714246602784768), // 0.26 => -1.04597
        FixedPoint(-18347548093616457728), // 0.27 => -0.99462
        FixedPoint(-17422241585731162112), // 0.28 => -0.94446
        FixedPoint(-16516920363702972416), // 0.29 => -0.89538
        FixedPoint(-15629886784764432384), // 0.30 => -0.84730
        FixedPoint(-14759595957603760128), // 0.31 => -0.80012
        FixedPoint(-13904635528415858688), // 0.32 => -0.75377
        FixedPoint(-13063708520358164480), // 0.33 => -0.70819
        FixedPoint(-12235618674138603520), // 0.34 => -0.66329
        FixedPoint(-11419257849061355520), // 0.35 => -0.61904
        FixedPoint(-10613595130224742400), // 0.36 => -0.57536
        FixedPoint(-9817667354921738240),  // 0.37 => -0.53222
        FixedPoint(-9030570824192866304),  // 0.38 => -0.48955
        FixedPoint(-8251454007294845952),  // 0.39 => -0.44731
        FixedPoint(-7479511080090284032),  // 0.40 => -0.40547
        FixedPoint(-6713976164925600768),  // 0.41 => -0.36397
        FixedPoint(-5954118160879564800),  // 0.42 => -0.32277
        FixedPoint(-5199236070424976384),  // 0.43 => -0.28185
        FixedPoint(-4448654742390539264),  // 0.44 => -0.24116
        FixedPoint(-3701720962283612672),  // 0.45 => -0.20067
        FixedPoint(-2957799830037197824),  // 0.46 => -0.16034
        FixedPoint(-2216271372462491904),  // 0.47 => -0.12014
        FixedPoint(-1476527343420476672),  // 0.48 => -0.08004
        FixedPoint(-737968169202023424),   // 0.49 => -0.04001
        FixedPoint(0),                     // 0.50 => -0.00000
        FixedPoint(737968169202024192),    // 0.51 => 0.04001
        FixedPoint(1476527343420477440),   // 0.52 => 0.08004
        FixedPoint(2216271372462496512),   // 0.53 => 0.12014
        FixedPoint(2957799830037204480),   // 0.54 => 0.16034
        FixedPoint(3701720962283612672),   // 0.55 => 0.20067
        FixedPoint(4448654742390539264),   // 0.56 => 0.24116
        FixedPoint(5199236070424971264),   // 0.57 => 0.28185
        FixedPoint(5954118160879561728),   // 0.58 => 0.32277
        FixedPoint(6713976164925597696),   // 0.59 => 0.36397
        FixedPoint(7479511080090281984),   // 0.60 => 0.40547
        FixedPoint(8251454007294848000),   // 0.61 => 0.44731
        FixedPoint(9030570824192860160),   // 0.62 => 0.48955
        FixedPoint(9817667354921742336),   // 0.63 => 0.53222
        FixedPoint(10613595130224742400),  // 0.64 => 0.57536
        FixedPoint(11419257849061359616),  // 0.65 => 0.61904
        FixedPoint(12235618674138605568),  // 0.66 => 0.66329
        FixedPoint(13063708520358168576),  // 0.67 => 0.70819
        FixedPoint(13904635528415862784),  // 0.68 => 0.75377
        FixedPoint(14759595957603747840),  // 0.69 => 0.80012
        FixedPoint(15629886784764430336),  // 0.70 => 0.84730
        FixedPoint(16516920363702964224),  // 0.71 => 0.89538
        FixedPoint(17422241585731166208),  // 0.72 => 0.94446
        FixedPoint(18347548093616463872),  // 0.73 => 0.99462
        FixedPoint(19294714246602788864),  // 0.74 => 1.04597
        FixedPoint(20265819725292945408),  // 0.75 => 1.09861
        FixedPoint(21263183918842335232),  // 0.76 => 1.15268
        FixedPoint(22289407577085243392),  // 0.77 => 1.20831
        FixedPoint(23347423671542181888),  // 0.78 => 1.26567
        FixedPoint(24440560042528653312),  // 0.79 => 1.32493
        FixedPoint(25572617290405310464),  // 0.80 => 1.38629
        FixedPoint(26747966611833827328),  // 0.81 => 1.45001
        FixedPoint(27971674063185145856),  // 0.82 => 1.51635
        FixedPoint(29249660330515173376),  // 0.83 => 1.58563
        FixedPoint(30588908944945000448),  // 0.84 => 1.65823
        FixedPoint(31997741738730881024),  // 0.85 => 1.73460
        FixedPoint(33486189434148524032),  // 0.86 => 1.81529
        FixedPoint(35066499762404098048),  // 0.87 => 1.90096
        FixedPoint(36753849332779192320),  // 0.88 => 1.99243
        FixedPoint(38567365939524001792),  // 0.89 => 2.09074
        FixedPoint(40531639450585874432),  // 0.90 => 2.19722
        FixedPoint(42679031418590240768),  // 0.91 => 2.31363
        FixedPoint(45053350700638978048),  // 0.92 => 2.44235
        FixedPoint(47715996328766390272),  // 0.93 => 2.58669
        FixedPoint(50756867729459134464),  // 0.94 => 2.75154
        FixedPoint(54315312289337958400),  // 0.95 => 2.94444
        FixedPoint(58624745660900876288),  // 0.96 => 3.17805
        FixedPoint(64122702906348298240),  // 0.97 => 3.47610
        FixedPoint(71791413020114722816),  // 0.98 => 3.89182
        FixedPoint(84764999863455252480),  // 0.99 => 4.59512
        FixedPoint(84764999863455252480),  // 1.00 => 4.59512
    ];

    // Distance between points, in unwrapped FixedPoint
    const POINT_LEN_FP: i128 = FixedPoint::ONE.0 / (POINTS.len() as i128 - 1);
    // Which line segment we're on, defined as the segment between POINTS[bucket] and POINTS[bucket + 1]
    let bucket = (usage.0 / POINT_LEN_FP).clamp(0, POINTS.len() as i128 - 2) as usize;
    let b = POINTS[bucket];
    let c = POINTS[bucket + 1];
    let frac = FixedPoint::from_u128_div(
        i128::max(0, usage.0 - bucket as i128 * POINT_LEN_FP) as u128,
        POINT_LEN_FP as u128,
    );
    (b * (FixedPoint::ONE - frac) + c * frac) / a
}

#[cfg(test)]
// The price adjustment function, (almost) as specified in tokenomics document
fn price_adjustment_target(usage: f64, a: f64) -> f64 {
    // original
    // -(1f64 / (f64::max(usage, 0.01)) - 0.99).ln() / a
    // instead of this, we clamp usage at 0.01 and 0.99, in order for the result to be symmetric.
    -(1f64 / usage.clamp(0.01, 0.99) - 1f64).ln() / a
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn within_permissible_error(a: f64, b: f64, epsilon: f64) {
        if a - epsilon >= b || a + epsilon <= b {
            panic!("{a} != {b} (with error {epsilon})");
        }
    }

    #[test]
    // This test ensures parity between the spec implementation of the price adjustment function
    // `price_adjustment_target` and the real implementation `price_adjustment_function`.
    //
    // While the test `test_price_adjustment` tests this as a proptest, this function tests a fixed
    // number of statically defined points, which, if this test fails, can be extracted from stdout
    // and directly used to instantiate the lookup table in `price_adjustment_function` to correct
    // it.
    fn test_price_adjustment_static() {
        const N: usize = 100;
        let xs = (0..=N).map(|x| x as f64 / N as f64).collect::<Vec<_>>();
        let target = xs
            .iter()
            .map(|x| price_adjustment_target(*x, 1f64))
            .collect::<Vec<_>>();
        let approx = xs
            .iter()
            .map(|x| f64::from(price_adjustment_function((*x).into(), FixedPoint::ONE)))
            .collect::<Vec<_>>();
        assert_eq!(price_adjustment_target(0.5, 1f64), 0f64);
        // Print out the initialization for `POINTS` derived from `target`
        // Manually munged, as debug printing for fixedpoint is lossy
        for (x, point) in xs.iter().zip(target.iter()) {
            println!(
                "FixedPoint({}), // {x:0.2} => {point:0.5}",
                FixedPoint::from(*point).0
            );
        }
        for ((t, a), _x) in target.iter().zip(approx.iter()).zip(xs.iter()) {
            within_permissible_error(*t, *a, 1e-9f64);
        }
    }

    #[test]
    fn test_pricing_cant_get_stuck() {
        let mut cur = FeePrices {
            block_usage_price: FixedPoint::ONE,
            read_price: FixedPoint::ONE,
            write_price: FixedPoint::ONE,
            compute_price: FixedPoint::ONE,
        };
        for _ in 0..10_000 {
            cur = cur.update_from_fullness(
                NormalizedCost {
                    read_time: FixedPoint::ZERO,
                    compute_time: FixedPoint::ZERO,
                    block_usage: FixedPoint::ZERO,
                    bytes_written: FixedPoint::ZERO,
                    bytes_churned: FixedPoint::ZERO,
                },
                FixedPoint::from_u64_div(1, 4),
                FixedPoint::from_u64_div(100, 1),
            );
        }
        let dims = |cur: &FeePrices| {
            [
                cur.block_usage_price,
                cur.read_price,
                cur.write_price,
                cur.compute_price,
            ]
        };
        assert!(dims(&cur).into_iter().all(|price| price > FixedPoint::ZERO));
        let fraction = FixedPoint::from_u64_div(3, 4);
        let fullness = NormalizedCost {
            block_usage: fraction,
            compute_time: fraction,
            read_time: fraction,
            bytes_written: fraction,
            bytes_churned: fraction,
        };
        let next = cur.update_from_fullness(
            fullness,
            FixedPoint::from_u64_div(1, 4),
            FixedPoint::from_u64_div(100, 1),
        );
        assert!(
            dims(&cur)
                .into_iter()
                .zip(dims(&next).into_iter())
                .all(|(cur, next)| next > cur)
        );
    }

    proptest! {
        #[test]
        fn test_price_adjustment(usage in (0f64..1f64)) {
            let a = price_adjustment_target(usage, 1f64);
            let b = f64::from(price_adjustment_function(FixedPoint::from(usage), FixedPoint::ONE));
            let epsilon = (a / 50f64).abs();
            within_permissible_error(
                a, b, epsilon
            );
        }
        #[test]
        fn fixed_point_powi(a in (1e-1f64..1e1f64), b in (-15..15)) {
            if a != 0.0 {
                let pure_error_factor = a.powi(b) / f64::from(FixedPoint::from(a).powi(b));
                let non_compounded_error_factor = pure_error_factor.powi(-b.abs());
                within_permissible_error(1.0, non_compounded_error_factor, 0.001);
            }
        }
        #[test]
        fn fixed_point_addition(a in (-1e18f64..1e18f64), b in (-1e18f64..1e18f64)) {
            assert_eq!(a + b, f64::from(FixedPoint::from(a) + FixedPoint::from(b)));
        }
        #[test]
        fn fixed_point_subtraction(a in (-1e18f64..1e18f64), b in (-1e18f64..1e18f64)) {
            assert_eq!(a - b, f64::from(FixedPoint::from(a) - FixedPoint::from(b)));
        }
        #[test]
        fn fixed_point_negation(a in (-1e18f64..1e18f64)) {
            assert_eq!(-a, f64::from(-FixedPoint::from(a)));
        }
        #[test]
        fn fixed_point_mul(a in (-1e9f64..1e9f64), b in (-1e9f64..1e9f64)) {
            assert_eq!(a * b, f64::from(FixedPoint::from(a) * FixedPoint::from(b)));
        }
        #[test]
        fn fixed_point_div(a in (-1e9f64..1e9f64), b in (-1e9f64..1e9f64)) {
            within_permissible_error(a / b, f64::from(FixedPoint::from(a) / FixedPoint::from(b)), 1e-3f64);
        }
        #[test]
        fn u64_div(a: u64, b in 1u64..u64::MAX) {
            within_permissible_error(a as f64 / b as f64, f64::from(FixedPoint::from_u64_div(a, b)), 1e-9f64);
        }
        #[test]
        fn u128_div(a: u128, b in 1u128..u128::MAX) {
            within_permissible_error(a as f64 / b as f64, f64::from(FixedPoint::from_u128_div(a, b)), 1e-9f64);
        }
    }
}
