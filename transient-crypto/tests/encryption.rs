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

#[cfg(test)]
mod tests {
    use midnight_transient_crypto::curve::Fr;
    use midnight_transient_crypto::encryption::SecretKey;
    use rand::rngs::OsRng;

    #[test]
    fn encryption_test() {
        let sk = SecretKey::new(&mut OsRng);
        let pk = sk.public_key();
        let c = pk.encrypt(&mut OsRng, &Fr::from(42));
        let p: Option<Fr> = sk.decrypt(&c);
        assert_eq!(p, Some(Fr::from(42)));
    }
}
