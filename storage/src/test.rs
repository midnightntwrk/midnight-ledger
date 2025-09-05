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

//! Testing helpers

use std::time::Instant;

/// Helper for printing time elapsed between chunks of code.
pub(crate) struct Timer {
    prefix: String,
    start: Instant,
    last: Instant,
}
impl Timer {
    /// Create a new Timer, initializing both start and last to the current
    /// instant
    ///
    /// The prefix is added to `Self::delta` messages.
    pub(crate) fn new<S: AsRef<str>>(prefix: S) -> Self {
        let now = Instant::now();
        Timer {
            prefix: prefix.as_ref().to_string(),
            start: now,
            last: now,
        }
    }

    /// Print `"&lt;self.prefix&gt;: &lt;time since last delta&gt;/&lt;time since start&gt;: &lt;msg&gt;"`.
    ///
    /// Returns the time since the last call to `delta`, or since
    /// construction if this is the first call to `delta`.
    pub(crate) fn delta<S: AsRef<str>>(&mut self, msg: S) -> f32 {
        let now = Instant::now();
        let duration_since_start = now.duration_since(self.start).as_secs_f32();
        let duration_since_last = now.duration_since(self.last).as_secs_f32();
        self.last = now;
        println!(
            "{}: {:.2?}/{:.2?}: {}",
            self.prefix,
            duration_since_last,
            duration_since_start,
            msg.as_ref(),
        );
        duration_since_last
    }
}
