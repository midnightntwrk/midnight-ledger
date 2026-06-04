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
use std::sync::Arc;

use crate::endpoints::{
    check, fetch_k, get_k, health, proof_versions, prove, prove_transaction, ready, version,
};
use crate::worker_pool::WorkerPool;

pub mod endpoints;
pub mod versioned_ir;
pub mod worker_pool;

mod payload_size_limit {
    use actix_web::body::EitherBody;
    use actix_web::dev::{self, Service, ServiceRequest, ServiceResponse, Transform};
    use actix_web::http::header;
    use actix_web::HttpResponse;
    use std::future::{ready, Future, Ready};
    use std::pin::Pin;

    pub(super) struct PayloadSizeLimit {
        pub max_payload: usize,
        pub max_json: usize,
    }

    impl<S, B> Transform<S, ServiceRequest> for PayloadSizeLimit
    where
        S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
        S::Future: 'static,
        B: 'static,
    {
        type Response = ServiceResponse<EitherBody<B>>;
        type Error = actix_web::Error;
        type InitError = ();
        type Transform = PayloadSizeLimitService<S>;
        type Future = Ready<Result<Self::Transform, Self::InitError>>;

        fn new_transform(&self, service: S) -> Self::Future {
            ready(Ok(PayloadSizeLimitService {
                service,
                max_payload: self.max_payload,
                max_json: self.max_json,
            }))
        }
    }

    pub(super) struct PayloadSizeLimitService<S> {
        service: S,
        max_payload: usize,
        max_json: usize,
    }

    impl<S, B> Service<ServiceRequest> for PayloadSizeLimitService<S>
    where
        S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = actix_web::Error>,
        S::Future: 'static,
        B: 'static,
    {
        type Response = ServiceResponse<EitherBody<B>>;
        type Error = actix_web::Error;
        type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

        dev::forward_ready!(service);

        fn call(&self, req: ServiceRequest) -> Self::Future {
            let is_json = req
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|ct| ct.starts_with("application/json"));
            let limit = if is_json {
                self.max_json
            } else {
                self.max_payload
            };

            let content_length = req
                .headers()
                .get(header::CONTENT_LENGTH)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<usize>().ok());

            if content_length.is_some_and(|len| len > limit) {
                let response = HttpResponse::PayloadTooLarge().finish();
                return Box::pin(async move {
                    Ok(req.into_response(response).map_into_right_body())
                });
            }

            let fut = self.service.call(req);
            Box::pin(async move {
                let res = fut.await?;
                Ok(res.map_into_left_body())
            })
        }
    }
}

pub fn server(
    port: u16,
    fetch_params: bool,
    pool: WorkerPool,
    max_payload_size: usize,
    max_json_size: usize,
) -> std::io::Result<(Server, u16)> {
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
            .wrap(Cors::permissive())
            .wrap(payload_size_limit::PayloadSizeLimit {
                max_payload: max_payload_size,
                max_json: max_json_size,
            });
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
