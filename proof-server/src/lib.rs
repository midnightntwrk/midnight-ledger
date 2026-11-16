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
use actix_cors::Cors;
use actix_web::dev::Server;
use actix_web::middleware::Logger;
use actix_web::web::{self, Data};
use actix_web::{App, HttpServer};
use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use tracing::warn;

use crate::endpoints::{
    BodyLimit, MaxK, check, fetch_k, get_k, health, proof_versions, prove, prove_transaction,
    ready, version,
};
use crate::worker_pool::WorkerPool;

pub mod endpoints;
pub mod versioned_ir;
pub mod worker_pool;

/// Where the server listens and which limits it enforces.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// The address to listen on. Loopback by default: the server takes private witness data over
    /// plain HTTP without authentication, so exposing it is an explicit choice.
    pub bind: IpAddr,
    /// The port to listen on; `0` picks a free one.
    pub port: u16,
    /// Whether to serve `/fetch-params/{k}`.
    pub fetch_params: bool,
    pub max_k: MaxK,
    pub max_body_size: BodyLimit,
    /// Origins allowed to call the server from a browser. `None` allows any origin.
    pub cors_origins: Option<Vec<String>>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            bind: IpAddr::V4(Ipv4Addr::LOCALHOST),
            port: 6300,
            fetch_params: true,
            max_k: MaxK::default(),
            max_body_size: BodyLimit::default(),
            cors_origins: None,
        }
    }
}

fn cors(origins: &Option<Vec<String>>) -> Cors {
    match origins {
        None => Cors::permissive(),
        Some(origins) => origins
            .iter()
            .fold(Cors::default(), |cors, origin| cors.allowed_origin(origin))
            .allow_any_method()
            .allow_any_header()
            .max_age(3600),
    }
}

pub fn server(config: ServerConfig, pool: WorkerPool) -> std::io::Result<(Server, u16)> {
    if config.cors_origins.is_none() {
        warn!(
            "CORS allows any origin: any web page open in a browser on this machine can submit \
             requests. Set --cors-origins to restrict it."
        );
    }
    let pool = Arc::new(pool);
    let ServerConfig {
        bind,
        port,
        fetch_params,
        max_k,
        max_body_size,
        cors_origins,
    } = config;
    let http_server = HttpServer::new(move || {
        let app = App::new()
            .app_data(Data::new(pool.clone()))
            .app_data(Data::new(max_k))
            .app_data(Data::new(max_body_size))
            .service(prove_transaction)
            .service(prove)
            .service(check)
            .service(get_k)
            .service(version)
            .service(proof_versions)
            .service(ready)
            .route("/", web::get().to(health))
            .route("/health", web::get().to(health))
            .wrap(Logger::new("%a %r; took %Ts"))
            .wrap(cors(&cors_origins));
        if fetch_params {
            app.service(fetch_k)
        } else {
            app
        }
    })
    .bind((bind, port))?;
    let port = http_server.addrs()[0].port();
    let srv = http_server.run();
    Ok((srv, port))
}
