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

use std::collections::HashMap;
use std::hash::Hash;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct MultiSet<T: Eq + Hash> {
    elements: HashMap<T, usize>,
}

impl<T: Eq + Hash> MultiSet<T> {
    pub(crate) fn new() -> Self {
        MultiSet {
            elements: std::collections::HashMap::new(),
        }
    }

    pub(crate) fn insert(&mut self, element: T) {
        *self.elements.entry(element).or_insert(0) += 1;
    }

    pub(crate) fn count(&self, element: &T) -> usize {
        *self.elements.get(element).unwrap_or(&0)
    }

    pub(crate) fn has_subset(&self, other: &MultiSet<T>) -> bool {
        for (element, other_count) in &other.elements {
            let self_count = self.count(element);
            if self_count < *other_count {
                return false;
            }
        }
        true
    }
}

impl<T: Eq + Hash> IntoIterator for MultiSet<T> {
    type Item = (T, usize);
    type IntoIter = std::collections::hash_map::IntoIter<T, usize>;

    fn into_iter(self) -> Self::IntoIter {
        self.elements.into_iter()
    }
}

impl<T: Eq + Hash + Clone> FromIterator<T> for MultiSet<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        let mut multiset = MultiSet::new();
        for item in iter {
            multiset.insert(item);
        }
        multiset
    }
}
