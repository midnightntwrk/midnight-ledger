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

#![deny(unreachable_pub)]
#![deny(warnings)]
use actix_cors::Cors;
use actix_web::dev::Server;
use actix_web::middleware::Logger;
use actix_web::web::{self, Data};
use actix_web::{App, HttpServer};
use std::sync::Arc;

use crate::endpoints::{
    check, fetch_k, get_k, health, proof_versions, prove, prove_transaction, ready, version,
};
use crate::worker_pool::WorkerPool;

pub mod endpoints;
pub mod versioned_ir;
pub mod worker_pool;

pub fn server(port: u16, fetch_params: bool, pool: WorkerPool) -> std::io::Result<(Server, u16)> {
    let pool = Arc::new(pool);
    let http_server = HttpServer::new(move || {
        let app = App::new()
            .app_data(Data::new(pool.clone()))
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
            .wrap(Cors::permissive());
        if fetch_params {
            app.service(fetch_k)
        } else {
            app
        }
    })
    .bind(("0.0.0.0", port))?;
    let port = http_server.addrs()[0].port();
    let srv = http_server.run();
    Ok((srv, port))
}
