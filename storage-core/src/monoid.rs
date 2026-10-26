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

//! Associative combination, with saturating instances for integers.

use crate::storable::SizeAnn;

/// A `Semigroup` is a structure with an associative binary operator
///
/// Implementations are responsible for satisfying the law:
///
/// `∀ a b c. (a.append(b)).append(c) == a.append(b.append(c))`
pub trait Semigroup {
    /// The associative binary operator
    fn append(&self, other: &Self) -> Self;
}

/// A `Monoid` is a `Semigroup` with an identity element.
///
/// The identity element, `empty`, can be combined with any other value
/// using `Semigroup::append` without changing the other value.
///
/// For example, for numbers under addition, the identity element is `0`, since `n + 0 == n`.
///
/// Implementations are responsible for satisfying the laws:
///
/// Right identity: `∀ a. a.append(Self::empty()) == a`
/// Left identity: `∀ a. Self::empty().append(a) == a`
pub trait Monoid: Semigroup {
    /// Returns the identity element
    fn empty() -> Self;

    /// Appends `n` copies of `self`, in `O(log n)` appends.
    fn times(&self, mut n: u64) -> Self
    where
        Self: Sized,
    {
        let mut acc = Self::empty();
        let mut base = Self::empty().append(self);
        while n > 0 {
            if n & 1 == 1 {
                acc = acc.append(&base);
            }
            base = base.append(&base);
            n >>= 1;
        }
        acc
    }
}

impl Semigroup for () {
    fn append(&self, _: &Self) -> Self {}
}

impl Monoid for () {
    fn empty() -> Self {}
}

impl Semigroup for u64 {
    fn append(&self, other: &Self) -> Self {
        self.saturating_add(*other)
    }
}

impl Monoid for u64 {
    fn empty() -> Self {
        0
    }
}

impl Semigroup for u128 {
    fn append(&self, other: &Self) -> Self {
        self.saturating_add(*other)
    }
}

impl Monoid for u128 {
    fn empty() -> Self {
        0
    }
}

impl Semigroup for i64 {
    fn append(&self, other: &Self) -> Self {
        self.saturating_add(*other)
    }
}

impl Monoid for i64 {
    fn empty() -> Self {
        0
    }
}

impl Semigroup for i128 {
    fn append(&self, other: &Self) -> Self {
        self.saturating_add(*other)
    }
}

impl Monoid for i128 {
    fn empty() -> Self {
        0
    }
}

impl Semigroup for SizeAnn {
    fn append(&self, other: &Self) -> Self {
        SizeAnn(self.0.saturating_add(other.0))
    }
}

impl Monoid for SizeAnn {
    fn empty() -> Self {
        SizeAnn(0)
    }
}

impl Semigroup for usize {
    fn append(&self, other: &Self) -> Self {
        self.saturating_add(*other)
    }
}

impl Monoid for usize {
    fn empty() -> Self {
        0
    }
}

/// Keeps the leftmost error, so a fold over a fixed order fails deterministically.
impl<E: Clone> Semigroup for Result<(), E> {
    fn append(&self, other: &Self) -> Self {
        match self {
            Ok(()) => other.clone(),
            Err(e) => Err(e.clone()),
        }
    }
}

impl<E: Clone> Monoid for Result<(), E> {
    fn empty() -> Self {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[derive(Clone, Debug, PartialEq)]
    struct Concat(Vec<u8>);

    impl Semigroup for Concat {
        fn append(&self, other: &Self) -> Self {
            Concat([&self.0[..], &other.0[..]].concat())
        }
    }

    impl Monoid for Concat {
        fn empty() -> Self {
            Concat(Vec::new())
        }
    }

    proptest! {
        #[test]
        fn times_is_repeated_append(v in prop::collection::vec(any::<u8>(), 0..4), n in 0u64..40) {
            let x = Concat(v);
            let naive = (0..n).fold(Concat::empty(), |acc, _| acc.append(&x));
            prop_assert_eq!(x.times(n), naive);
        }

        #[test]
        fn times_saturates(x in any::<u64>(), n in any::<u64>()) {
            prop_assert_eq!(x.times(n), x.saturating_mul(n));
        }
    }
}
