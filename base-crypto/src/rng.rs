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

//! Extension traits for specific [`Rng`]s.

use rand::rngs::{OsRng, StdRng};
use rand::{Rng, RngCore, SeedableRng};

/// A [`Rng`] that can be split. This is *not* the same as [Clone], as the
/// resulting instance is guaranteed to produce independent random values from
/// `self`.
///
/// Due to trait limitations, this does not have a blanket implementation for
/// and [`SeedableRng`], but may be implemented for any as needed.
pub trait SplittableRng: Rng {
    /// Generates a separate instance of `Self` from a random number generator,
    /// which is guaranteed to produce data that is independent of the data
    /// `self` generates in the future.
    fn split(&mut self) -> Self;
}

impl SplittableRng for OsRng {
    fn split(&mut self) -> Self {
        OsRng
    }
}

trait SplittableMarker {}

impl SplittableMarker for StdRng {}

impl<R: SeedableRng + RngCore + SplittableMarker> SplittableRng for R {
    fn split(&mut self) -> Self {
        Self::from_rng(self).expect("Rng must survive splitting!")
    }
}

/// Adapts a `rand` 0.8 RNG to the `rand_core` 0.10 traits expected by `k256`.
pub(crate) struct RngCompat<'a, R: ?Sized>(pub(crate) &'a mut R);

impl<R: RngCore + ?Sized> k256::elliptic_curve::rand_core::TryRng for RngCompat<'_, R> {
    type Error = std::convert::Infallible;

    fn try_next_u32(&mut self) -> Result<u32, Self::Error> {
        Ok(self.0.next_u32())
    }

    fn try_next_u64(&mut self) -> Result<u64, Self::Error> {
        Ok(self.0.next_u64())
    }

    fn try_fill_bytes(&mut self, dst: &mut [u8]) -> Result<(), Self::Error> {
        self.0.fill_bytes(dst);
        Ok(())
    }
}

impl<R: RngCore + rand::CryptoRng + ?Sized> k256::elliptic_curve::rand_core::TryCryptoRng
    for RngCompat<'_, R>
{
}
