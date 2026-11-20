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

#![deny(unreachable_pub)]
#![deny(warnings)]
use clap::Parser;

use base_crypto::data_provider::{FetchMode, MidnightDataProvider, OutputMode};
use futures::future::join;
use ledger::dust::DustResolver;
use ledger::prove::Resolver;
use midnight_proof_server::endpoints::PUBLIC_PARAMS;
use midnight_proof_server::key_derivation::KeyDerivationConfig;
use midnight_proof_server::{server_with_key_derivation, worker_pool::WorkerPool};
use tracing::{Level, info};
use tracing_subscriber::filter::Targets;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{Layer, Registry};
use transient_crypto::proofs::{KeyLocation, Resolver as ResolverT};

#[derive(Parser, Debug)]
struct Args {
    #[arg(
        short,
        long,
        default_value_t = 6300,
        env = "MIDNIGHT_PROOF_SERVER_PORT"
    )]
    port: u16,
    #[arg(short, long, env = "MIDNIGHT_PROOF_SERVER_VERBOSE")]
    verbose: bool,
    #[arg(long, default_value_t = 10, env = "MIDNIGHT_PROOF_SERVER_JOB_CAPACITY")]
    job_capacity: usize,
    #[arg(long, default_value_t = 2, env = "MIDNIGHT_PROOF_SERVER_NUM_WORKERS")]
    num_workers: usize,
    #[arg(
        long,
        default_value_t = 600.0,
        env = "MIDNIGHT_PROOF_SERVER_JOB_TIMEOUT"
    )]
    job_timeout: f64,
    #[arg(
        long,
        default_value_t = false,
        env = "MIDNIGHT_PROOF_SERVER_NO_FETCH_PARAMS"
    )]
    no_fetch_params: bool,
    /// Derive the prover key from the IR when a `/prove` request's key
    /// material has an empty prover key. Off by default: such a request is
    /// then refused with HTTP 422 and `x-key-derivation-refused: disabled`.
    /// Enable it only on a server that can take the extra CPU and memory a
    /// derivation costs; the limits below bound it.
    #[arg(
        long,
        default_value_t = false,
        env = "MIDNIGHT_PROOF_SERVER_KEY_DERIVATION"
    )]
    key_derivation: bool,
    /// Largest circuit `k` whose prover key is derived on demand, when
    /// derivation is enabled.
    #[arg(
        long,
        default_value_t = KeyDerivationConfig::default().max_k,
        env = "MIDNIGHT_PROOF_SERVER_KEY_DERIVATION_MAX_K",
        value_parser = clap::value_parser!(u8).range(1..=25)
    )]
    key_derivation_max_k: u8,
    /// Number of prover keys that may be derived at the same time.
    #[arg(
        long,
        default_value_t = KeyDerivationConfig::default().max_concurrent,
        env = "MIDNIGHT_PROOF_SERVER_KEY_DERIVATION_CONCURRENCY",
        value_parser = clap::builder::RangedU64ValueParser::<usize>::new().range(1..)
    )]
    key_derivation_concurrency: usize,
    /// Memory bound, in MiB, for derived prover keys kept for reuse.
    #[arg(
        long,
        default_value_t = KeyDerivationConfig::default().cache_bytes >> 20,
        env = "MIDNIGHT_PROOF_SERVER_KEY_CACHE_MIB"
    )]
    key_cache_mib: usize,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let args = Args::parse();
    init_logging(args.verbose);
    if !args.no_fetch_params {
        info!("Ensuring zswap key material is available...");
        let resolver = Resolver::new(
            PUBLIC_PARAMS.clone(),
            DustResolver(
                MidnightDataProvider::new(
                    FetchMode::OnDemand,
                    OutputMode::Log,
                    ledger::dust::DUST_EXPECTED_FILES.to_owned(),
                )
                .expect("data provider initialization failed"),
            ),
            Box::new(move |_: KeyLocation| Box::pin(std::future::ready(Ok(None)))),
        );
        let ks = futures::future::join_all((10..=15).map(|k| PUBLIC_PARAMS.0.fetch_k(k)));
        let keys = futures::future::join_all(
            [
                "midnight/zswap/spend",
                "midnight/zswap/output",
                "midnight/zswap/sign",
                "midnight/dust/spend",
            ]
            .into_iter()
            .map(|k| resolver.resolve_key(KeyLocation(k.into()))),
        );
        let (ks, keys) = join(ks, keys).await;
        ks.into_iter().collect::<Result<Vec<_>, _>>()?;
        keys.into_iter().collect::<Result<Vec<_>, _>>()?;
    }
    let key_derivation = KeyDerivationConfig {
        enabled: args.key_derivation,
        max_k: args.key_derivation_max_k,
        max_concurrent: args.key_derivation_concurrency,
        cache_bytes: args.key_cache_mib << 20,
    };
    if key_derivation.enabled {
        info!(
            "Deriving prover keys on demand up to k={}",
            key_derivation.max_k
        );
    }
    let pool = WorkerPool::new(args.num_workers, args.job_capacity, args.job_timeout);
    server_with_key_derivation(args.port, !args.no_fetch_params, pool, key_derivation)
        .unwrap()
        .0
        .await
}

fn init_logging(verbose: bool) {
    let level = if verbose { Level::DEBUG } else { Level::INFO };
    Registry::default()
        .with(
            tracing_subscriber::fmt::layer().with_filter(
                Targets::new()
                    .with_default(level)
                    .with_target("zkir", tracing_subscriber::filter::LevelFilter::OFF),
            ),
        )
        .try_init()
        .ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_derivation_is_opt_in() {
        let args = Args::try_parse_from(["midnight-proof-server"]).unwrap();
        assert!(!args.key_derivation);
        let args = Args::try_parse_from(["midnight-proof-server", "--key-derivation"]).unwrap();
        assert!(args.key_derivation);
    }
}
