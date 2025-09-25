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
// limitations under the License

use transient_crypto::encryption::SecretKey;
use ledger::dust::DustSecretKey;

#[derive(Clone)]
pub struct CoinSecretKey(pub SecretKey);

impl CoinSecretKey {
    pub fn new(secret_key: SecretKey) -> Self {
        CoinSecretKey(secret_key)
    }

    pub fn inner(&self) -> &SecretKey {
        &self.0
    }
}

#[derive(Clone)]
pub struct DustSecretKeyWrapper(pub DustSecretKey);

impl DustSecretKeyWrapper {
    pub fn new(secret_key: DustSecretKey) -> Self {
        DustSecretKeyWrapper(secret_key)
    }

    pub fn inner(&self) -> &DustSecretKey {
        &self.0
    }
}
