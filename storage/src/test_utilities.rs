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

//! Helpers for tests in this and downstream crates.

use crate::arena::Sp;
use crate::db::DB;
use crate::storable::Storable;
use serialize::{Deserializable, Serializable};
use std::sync::mpsc;
use std::time::Duration;

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
