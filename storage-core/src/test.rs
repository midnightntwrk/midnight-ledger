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

//! Testing helpers

use crate::Storable;
use crate::arena::Sp;
use crate::db::DB;
use serialize::{Deserializable, Serializable};
use std::cell::Cell;
use std::sync::mpsc;
use std::time::{Duration, Instant};

thread_local! {
    static DEREFS: Cell<u64> = const { Cell::new(0) };
}

/// The number of `Sp` dereferences and arena node loads made on this thread
/// so far. Every walk of a storage DAG does one of these per node it visits,
/// so this measures walking work deterministically.
pub fn derefs() -> u64 {
    DEREFS.with(Cell::get)
}

pub(crate) fn count_deref() {
    DEREFS.with(|n| n.set(n.get() + 1));
}

/// Helper for printing time elapsed between chunks of code.
pub struct Timer {
    prefix: String,
    start: Instant,
    last: Instant,
}
impl Timer {
    /// Create a new Timer, initializing both start and last to the current
    /// instant
    ///
    /// The prefix is added to `Self::delta` messages.
    pub fn new<S: AsRef<str>>(prefix: S) -> Self {
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
    pub fn delta<S: AsRef<str>>(&mut self, msg: S) -> f32 {
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

/// `value` encoded as an `Sp` and decoded again, running every decode check.
pub fn round_trip<T: Storable<D> + Serializable, D: DB>(value: &T) -> std::io::Result<Sp<T, D>> {
    let mut bytes = Vec::new();
    Sp::<T, D>::new(value.clone()).serialize(&mut bytes)?;
    Sp::<T, D>::deserialize(&mut &bytes[..], 0)
}

/// Runs `f` on a separate thread, panicking if it does not finish within
/// `limit`, so a regression to super-linear work fails instead of hanging.
/// A timed-out `f` keeps running until the test process exits.
pub fn assert_completes_within<T: Send + 'static>(
    limit: Duration,
    f: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (tx, rx) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    match rx.recv_timeout(limit) {
        Ok(res) => res,
        Err(mpsc::RecvTimeoutError::Timeout) => panic!("did not complete within {limit:?}"),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            std::panic::resume_unwind(handle.join().expect_err("sender dropped without sending"))
        }
    }
}
