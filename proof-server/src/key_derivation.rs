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

//! On-demand prover key derivation.
//!
//! A `/prove` request may carry [`ProvingKeyMaterial`] whose `prover_key` is
//! empty. If the operator enabled derivation, the server then derives the
//! prover key from the supplied IR with the in-tree key generation, accepts it
//! only if the verifier key generated with it is byte-for-byte equal to the
//! supplied verifier key, caches it, and proves exactly as if the client had
//! uploaded it. Derivation is off by default, and such a request is then
//! refused with [`Refusal::Disabled`].

use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use base_crypto::hash::{HashOutput, persistent_hash};
use tokio::sync::Semaphore;
use tracing::info;
use transient_crypto::proofs::ProvingKeyMaterial;

use crate::versioned_ir::{self, DeriveError};
use crate::worker_pool::WorkError;

/// Response header carrying the machine-readable reason a derivation was
/// refused. A client that sees it can resend the request with the full prover
/// key.
pub const REFUSAL_HEADER: &str = "x-key-derivation-refused";

/// Why the server refused to derive a prover key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Derivation is disabled on this server.
    Disabled,
    /// The circuit's `k` is above the server's derivation limit.
    KTooLarge,
    /// The derived verifier key differs from the supplied verifier key.
    VkMismatch,
    /// The IR version has no supported key generation.
    UnsupportedIr,
}

impl Refusal {
    /// The value of [`REFUSAL_HEADER`] for this refusal.
    pub fn code(&self) -> &'static str {
        match self {
            Refusal::Disabled => "disabled",
            Refusal::KTooLarge => "k-too-large",
            Refusal::VkMismatch => "vk-mismatch",
            Refusal::UnsupportedIr => "unsupported-ir",
        }
    }
}

/// Operator settings for on-demand derivation.
#[derive(Clone, Debug)]
pub struct KeyDerivationConfig {
    /// Whether an empty prover key is derived (otherwise it is refused).
    /// Off by default: an operator opts in.
    pub enabled: bool,
    /// Largest circuit `k` that is derived on demand.
    pub max_k: u8,
    /// Number of derivations that may run at the same time.
    pub max_concurrent: usize,
    /// Bound on the bytes of derived prover keys kept in memory.
    pub cache_bytes: usize,
}

impl Default for KeyDerivationConfig {
    fn default() -> Self {
        KeyDerivationConfig {
            enabled: false,
            max_k: 17,
            max_concurrent: 1,
            cache_bytes: 1 << 30,
        }
    }
}

/// Derived keys are identified by the IR and the verifier key they were
/// derived from and checked against. Both are needed: the IR is what was
/// derived from, and the verifier key is what the result was checked against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct CacheId {
    ir: HashOutput,
    vk: HashOutput,
}

impl CacheId {
    fn new(material: &ProvingKeyMaterial) -> Self {
        CacheId {
            ir: persistent_hash(&material.ir_source),
            vk: persistent_hash(&material.verifier_key),
        }
    }
}

#[derive(Default)]
struct Cache {
    bytes: usize,
    clock: u64,
    entries: HashMap<CacheId, (u64, Arc<Vec<u8>>)>,
}

/// On-demand prover key derivation, shared by all workers.
pub struct KeyDerivation {
    config: KeyDerivationConfig,
    permits: Semaphore,
    cache: Mutex<Cache>,
    derivations: AtomicU64,
    cache_hits: AtomicU64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct KeyDerivationStatus {
    enabled: bool,
    max_k: u8,
    max_concurrent: usize,
    cache_max_bytes: usize,
    cache_bytes: usize,
    cache_entries: usize,
    derivations: u64,
    cache_hits: u64,
}

impl KeyDerivation {
    pub fn new(config: KeyDerivationConfig) -> Self {
        KeyDerivation {
            permits: Semaphore::new(config.max_concurrent.max(1)),
            config,
            cache: Mutex::new(Cache::default()),
            derivations: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
        }
    }

    pub(crate) fn status(&self) -> KeyDerivationStatus {
        let cache = self.cache.lock().expect("mutex is not poisoned");
        KeyDerivationStatus {
            enabled: self.config.enabled,
            max_k: self.config.max_k,
            max_concurrent: self.config.max_concurrent.max(1),
            cache_max_bytes: self.config.cache_bytes,
            cache_bytes: cache.bytes,
            cache_entries: cache.entries.len(),
            derivations: self.derivations.load(Ordering::Relaxed),
            cache_hits: self.cache_hits.load(Ordering::Relaxed),
        }
    }

    /// Fills in the empty prover key of `material`, from the cache or by
    /// deriving it from `material.ir_source`.
    pub(crate) async fn complete(
        &self,
        mut material: ProvingKeyMaterial,
    ) -> Result<ProvingKeyMaterial, WorkError> {
        if !self.config.enabled {
            return Err(refuse(
                Refusal::Disabled,
                "prover key derivation is disabled on this server".into(),
            ));
        }
        let id = CacheId::new(&material);
        if let Some(prover_key) = self.cached(&id) {
            material.prover_key = prover_key;
            return Ok(material);
        }
        let ir = versioned_ir::DerivableIr::load(&material.ir_source).map_err(derive_error)?;
        let k = ir.k();
        if k > self.config.max_k {
            return Err(refuse(
                Refusal::KTooLarge,
                format!(
                    "k={k} exceeds this server's derivation limit of {}",
                    self.config.max_k
                ),
            ));
        }
        let supplied_vk = &material.verifier_key;
        let prover_key = self
            .single_flight(&id, || async move {
                let start = Instant::now();
                let (prover_key, verifier_key) = ir.keygen().await.map_err(derive_error)?;
                if &verifier_key != supplied_vk {
                    return Err(refuse(
                        Refusal::VkMismatch,
                        "the verifier key derived from the IR differs from the supplied verifier key"
                            .into(),
                    ));
                }
                info!(
                    k,
                    prover_key_bytes = prover_key.len(),
                    seconds = start.elapsed().as_secs_f64(),
                    "derived prover key"
                );
                Ok(prover_key)
            })
            .await?;
        material.prover_key = prover_key;
        Ok(material)
    }

    /// Runs `derive` under a derivation permit, unless the key was cached
    /// while waiting for the permit, and caches its result.
    async fn single_flight<F, Fut>(&self, id: &CacheId, derive: F) -> Result<Vec<u8>, WorkError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Vec<u8>, WorkError>>,
    {
        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|e| WorkError::InternalError(e.to_string()))?;
        if let Some(prover_key) = self.cached(id) {
            return Ok(prover_key);
        }
        let prover_key = derive().await?;
        self.derivations.fetch_add(1, Ordering::Relaxed);
        self.insert(*id, &prover_key);
        Ok(prover_key)
    }

    fn cached(&self, id: &CacheId) -> Option<Vec<u8>> {
        let hit = {
            let mut cache = self.cache.lock().expect("mutex is not poisoned");
            cache.clock += 1;
            let clock = cache.clock;
            cache.entries.get_mut(id).map(|(used, key)| {
                *used = clock;
                key.clone()
            })
        };
        hit.map(|key| {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
            (*key).clone()
        })
    }

    fn insert(&self, id: CacheId, prover_key: &[u8]) {
        let size = prover_key.len();
        if size > self.config.cache_bytes {
            return;
        }
        let mut cache = self.cache.lock().expect("mutex is not poisoned");
        while cache.bytes + size > self.config.cache_bytes {
            let oldest = cache
                .entries
                .iter()
                .min_by_key(|(_, (used, _))| *used)
                .map(|(id, _)| *id);
            match oldest.and_then(|id| cache.entries.remove(&id)) {
                Some((_, evicted)) => cache.bytes -= evicted.len(),
                None => break,
            }
        }
        cache.clock += 1;
        let clock = cache.clock;
        if let Some((_, replaced)) = cache
            .entries
            .insert(id, (clock, Arc::new(prover_key.to_vec())))
        {
            cache.bytes -= replaced.len();
        }
        cache.bytes += size;
    }
}

fn refuse(refusal: Refusal, message: String) -> WorkError {
    WorkError::KeyDerivationRefused(refusal, message)
}

fn derive_error(e: DeriveError) -> WorkError {
    match e {
        DeriveError::Unsupported(msg) => refuse(Refusal::UnsupportedIr, msg),
        DeriveError::BadInput(msg) => WorkError::BadInput(msg),
        DeriveError::Failed(msg) => WorkError::InternalError(msg),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    fn id(n: u8) -> CacheId {
        CacheId {
            ir: HashOutput([n; 32]),
            vk: HashOutput([n; 32]),
        }
    }

    fn derivation(max_concurrent: usize, cache_bytes: usize) -> Arc<KeyDerivation> {
        Arc::new(KeyDerivation::new(KeyDerivationConfig {
            max_concurrent,
            cache_bytes,
            ..Default::default()
        }))
    }

    async fn run_concurrently(derivation: Arc<KeyDerivation>, n: u8) -> usize {
        let running = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));
        let tasks = (0..n).map(|i| {
            let (derivation, running, peak) = (derivation.clone(), running.clone(), peak.clone());
            tokio::spawn(async move {
                derivation
                    .single_flight(&id(i), || async move {
                        let now = running.fetch_add(1, Ordering::SeqCst) + 1;
                        peak.fetch_max(now, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        running.fetch_sub(1, Ordering::SeqCst);
                        Ok(vec![i])
                    })
                    .await
            })
        });
        for result in futures::future::join_all(tasks).await {
            result.expect("task panicked").expect("derivation failed");
        }
        peak.load(Ordering::SeqCst)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn derivations_run_one_at_a_time_by_default() {
        assert_eq!(KeyDerivationConfig::default().max_concurrent, 1);
        assert_eq!(run_concurrently(derivation(1, 1 << 20), 4).await, 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrency_bound_is_configurable() {
        assert_eq!(run_concurrently(derivation(2, 1 << 20), 4).await, 2);
    }

    #[tokio::test]
    async fn waiting_request_uses_the_key_derived_meanwhile() {
        let derivation = derivation(1, 1 << 20);
        let calls = Arc::new(AtomicUsize::new(0));
        let tasks = (0..3).map(|_| {
            let (derivation, calls) = (derivation.clone(), calls.clone());
            tokio::spawn(async move {
                derivation
                    .single_flight(&id(7), || async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        Ok(vec![7; 8])
                    })
                    .await
            })
        });
        for result in futures::future::join_all(tasks).await {
            assert_eq!(result.unwrap().unwrap(), vec![7; 8]);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(derivation.status().derivations, 1);
        assert_eq!(derivation.status().cache_hits, 2);
    }

    #[tokio::test]
    async fn failed_derivation_is_not_cached() {
        let derivation = derivation(1, 1 << 20);
        let refused = derivation
            .single_flight(&id(1), || async {
                Err(refuse(Refusal::VkMismatch, "mismatch".into()))
            })
            .await;
        assert!(matches!(
            refused,
            Err(WorkError::KeyDerivationRefused(Refusal::VkMismatch, _))
        ));
        assert!(derivation.cached(&id(1)).is_none());
        assert_eq!(derivation.status().cache_entries, 0);
    }

    #[test]
    fn cache_evicts_least_recently_used_within_its_bound() {
        let derivation = KeyDerivation::new(KeyDerivationConfig {
            cache_bytes: 30,
            ..Default::default()
        });
        derivation.insert(id(1), &[1; 10]);
        derivation.insert(id(2), &[2; 10]);
        derivation.insert(id(3), &[3; 10]);
        // Touch 1, so that 2 is the least recently used.
        assert!(derivation.cached(&id(1)).is_some());
        derivation.insert(id(4), &[4; 10]);
        assert!(derivation.cached(&id(2)).is_none());
        assert!(derivation.cached(&id(1)).is_some());
        assert!(derivation.cached(&id(3)).is_some());
        assert!(derivation.cached(&id(4)).is_some());
        assert_eq!(derivation.status().cache_bytes, 30);
        // A key larger than the whole cache is used but never cached.
        derivation.insert(id(5), &[5; 31]);
        assert!(derivation.cached(&id(5)).is_none());
        assert_eq!(derivation.status().cache_entries, 3);
    }

    #[tokio::test]
    async fn derivation_is_refused_by_default() {
        assert!(!KeyDerivationConfig::default().enabled);
        let derivation = KeyDerivation::new(KeyDerivationConfig::default());
        let material = ProvingKeyMaterial {
            prover_key: vec![],
            verifier_key: vec![1],
            ir_source: vec![2],
        };
        assert!(matches!(
            derivation.complete(material).await,
            Err(WorkError::KeyDerivationRefused(Refusal::Disabled, _))
        ));
    }
}
