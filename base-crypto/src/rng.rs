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
