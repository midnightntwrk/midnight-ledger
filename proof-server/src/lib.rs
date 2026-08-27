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

#[cfg(feature = "gcp_cs")]
use {
    crate::endpoints::attestation,
    crypto_box::aead::Aead,
    crypto_box::{Nonce, PublicKey, SalsaBox, SecretKey},
    hex::{FromHex, ToHex},
    rand::RngCore,
};

use crate::endpoints::{
    check, fetch_k, get_k, health, proof_versions, prove, prove_transaction, ready, version,
};

#[cfg(feature = "gcp_cs")]
use crate::endpoints::proof_status;

use crate::worker_pool::WorkerPool;

pub mod endpoints;
pub mod versioned_ir;
pub mod worker_pool;

#[cfg(feature = "gcp_cs")]
#[derive(Clone)]
pub struct ServerEncryptionKey {
    _secret_key: SecretKey,
    public_key_hex: String,
}

#[cfg(feature = "gcp_cs")]
impl ServerEncryptionKey {
    pub fn generate() -> Self {
        let mut secret_key_bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut secret_key_bytes);
        let secret_key = SecretKey::from(secret_key_bytes);
        let public_key_hex = secret_key.public_key().encode_hex::<String>();

        Self {
            _secret_key: secret_key,
            public_key_hex,
        }
    }

    pub fn public_key_hex(&self) -> &str {
        &self.public_key_hex
    }

    pub fn decrypt_request(
        &self,
        client_public_key_hex: &str,
        nonce_hex: &str,
        ciphertext: &[u8],
    ) -> Option<Vec<u8>> {
        let client_public_key = Self::decode_public_key(client_public_key_hex)?;
        let nonce = Self::decode_nonce(nonce_hex)?;
        SalsaBox::new(&client_public_key, &self._secret_key)
            .decrypt(&nonce, ciphertext)
            .ok()
    }

    pub fn encrypt_response(
        &self,
        client_public_key_hex: &str,
        plaintext: &[u8],
    ) -> Option<(String, Vec<u8>)> {
        let mut nonce_bytes = [0u8; 24];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let response_nonce_hex = nonce_bytes.encode_hex::<String>();
        let ciphertext = self.encrypt_response_with_nonce(
            client_public_key_hex,
            &response_nonce_hex,
            plaintext,
        )?;
        Some((response_nonce_hex, ciphertext))
    }

    pub fn encrypt_response_with_nonce(
        &self,
        client_public_key_hex: &str,
        response_nonce_hex: &str,
        plaintext: &[u8],
    ) -> Option<Vec<u8>> {
        let client_public_key = Self::decode_public_key(client_public_key_hex)?;
        let nonce = Self::decode_nonce(response_nonce_hex)?;
        SalsaBox::new(&client_public_key, &self._secret_key)
            .encrypt(&nonce, plaintext)
            .ok()
    }

    fn decode_public_key(public_key_hex: &str) -> Option<PublicKey> {
        let public_key_bytes = <[u8; 32]>::from_hex(public_key_hex).ok()?;
        Some(PublicKey::from(public_key_bytes))
    }

    fn decode_nonce(nonce_hex: &str) -> Option<Nonce> {
        let nonce_bytes = <[u8; 24]>::from_hex(nonce_hex).ok()?;
        Some(Nonce::from(nonce_bytes))
    }
}

pub fn server(port: u16, fetch_params: bool, pool: WorkerPool) -> std::io::Result<(Server, u16)> {
    let pool = Arc::new(pool);
    #[cfg(feature = "gcp_cs")]
    let server_encryption_key = ServerEncryptionKey::generate();
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

        #[cfg(feature = "gcp_cs")]
        let app = app
            .app_data(Data::new(server_encryption_key.clone()))
            .service(attestation)
            .service(proof_status);

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
