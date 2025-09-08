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

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;

/// A type that implements `Tagged` can be described by a fixed type string.
///
/// This string should be uniquely determined by the type, and uniquely determine the type, and the
/// type given by a given tag should future-proof: A tag should never change its meaning.
///
/// Because associated constants cannot reference generics, this uses a static function instead.
///
/// ## Conventions
///
/// Tags should be limited to alphanumeric characters, dashes, square brackets, and (for generic
/// arguments only) parentheses and commas. Square brackets are used to denote the version of a
/// given type, allowing the same type name to be used with new data structures. By convention,
/// tags are in kebab case.
///
/// ## Examples of `<type>` and `<corresponding tag>`
///
/// - `u64` <-> `"u64"`
/// - `String` <-> `"string"`
/// - `FrequentlyChangingType` <-> `"frequently-changing-type[v1]"`
/// - `Option<Foo>` <-> `"option(foo)"`
/// - `GenericType<Foo, Bar>` <-> `"generic-type[v1](foo,bar)"`
/// - `(Foo, Bar)` <-> `"(foo,bar)"`
///
/// ## Usage
///
/// Tags are used by the [`tagged_serialize`](crate::tagged_serialize) and
/// [`tagged_deserialize`](crate::tagged_deserialize) functions as prefixes of serialized data.
/// These can be used to identify what the data is, ensuring that data deserialized was what was
/// intended to deserialize. In the future, it may be used to identify if data is in an out-of-date
/// format, and determine how to translate it.
pub trait Tagged {
    /// Retrieves the tag of `Self`. Returns a `[Cow]` as type arguments require allocation, but no
    /// allocation is preferred.
    fn tag() -> Cow<'static, str>;

    /// A decomposition of this tag into primitive types (any type whose representation is not
    /// defined through other types), tuples (via `(a,b)`), and sum types (via `[a,b]`).
    ///
    /// This is primarily used in automated testing to ensure that a change in the normal form
    /// representation (which is derived in most cases) also changes the associated tag.
    fn tag_unique_factor() -> String;
}

impl<'a, T: Tagged + 'a> Tagged for &'a T {
    fn tag() -> Cow<'static, str> {
        T::tag()
    }
    fn tag_unique_factor() -> String {
        T::tag_unique_factor()
    }
}

impl<T: Tagged> Tagged for Arc<T> {
    fn tag() -> Cow<'static, str> {
        T::tag()
    }
    fn tag_unique_factor() -> String {
        T::tag_unique_factor()
    }
}

impl<T: Tagged> Tagged for Box<T> {
    fn tag() -> Cow<'static, str> {
        T::tag()
    }
    fn tag_unique_factor() -> String {
        T::tag_unique_factor()
    }
}

impl<T: Tagged> Tagged for Option<T> {
    fn tag() -> Cow<'static, str> {
        Cow::Owned(format!("option({})", T::tag()))
    }
    fn tag_unique_factor() -> String {
        format!("[(),{}]", T::tag())
    }
}

impl<T: Tagged> Tagged for Vec<T> {
    fn tag() -> Cow<'static, str> {
        Cow::Owned(format!("vec({})", T::tag()))
    }
    fn tag_unique_factor() -> String {
        format!("vec({})", T::tag())
    }
}

impl<K: Tagged, V: Tagged> Tagged for HashMap<K, V> {
    fn tag() -> Cow<'static, str> {
        Cow::Owned(format!("map({},{})", K::tag(), V::tag()))
    }
    fn tag_unique_factor() -> String {
        format!("map({},{})", K::tag(), V::tag())
    }
}

impl<const N: usize, T: Tagged> Tagged for [T; N] {
    fn tag() -> Cow<'static, str> {
        Cow::Owned(format!("array({},{})", T::tag(), N))
    }
    fn tag_unique_factor() -> String {
        format!("array({},{})", T::tag(), N)
    }
}
