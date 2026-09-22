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

use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use transient_crypto::proofs::{KeyLocation, ProvingKeyMaterial};

struct Artifact {
    path: PathBuf,
    size: usize,
    hash: String,
}

impl Artifact {
    fn from_manifest(root: &Path, manifest: &Value, dir: &str, name: &str) -> io::Result<Self> {
        let entry = &manifest[dir][name];
        let size = entry["size"]
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| invalid("Missing artifact size"))?;
        let hash = entry["hash"]
            .as_str()
            .filter(|h| is_hash(h))
            .ok_or_else(|| invalid("Missing artifact SHA-256"))?
            .to_owned();
        if entry["type"] != "file" {
            return Err(invalid("Expected artifact file"));
        }
        let path = root.join(dir).join(name).canonicalize()?;
        if !path.starts_with(root) {
            return Err(invalid("Artifact escapes configured directory"));
        }
        let metadata = fs::metadata(&path)?;
        if !metadata.is_file() || metadata.len() != size as u64 {
            return Err(invalid(format!(
                "Artifact size differs from manifest: {}",
                path.display()
            )));
        }
        Ok(Self { path, size, hash })
    }

    fn read(&self) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        fs::File::open(&self.path)?
            .take(self.size as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() != self.size || hex::encode(Sha256::digest(&bytes)) != self.hash {
            return Err(invalid(format!(
                "Artifact integrity check failed: {}",
                self.path.display()
            )));
        }
        Ok(bytes)
    }
}

struct Bundle {
    prover: Artifact,
    verifier: Artifact,
    ir: Artifact,
}

impl Bundle {
    fn load(root: &Path, manifest: &Value, circuit: &str) -> io::Result<Self> {
        let verifier =
            Artifact::from_manifest(root, manifest, "keys", &format!("{circuit}.verifier"))?;
        let prover = Artifact::from_manifest(root, manifest, "keys", &format!("{circuit}.prover"))?;
        let ir = Artifact::from_manifest(root, manifest, "zkir", &format!("{circuit}.bzkir"))?;
        verifier.read()?;
        ir.read()?;
        Ok(Self {
            prover,
            verifier,
            ir,
        })
    }
}

#[derive(Hash, PartialEq, Eq)]
struct CircuitIdentity {
    circuit: String,
    verifier_hash: String,
}

pub struct ArtifactRegistry {
    bundles: HashMap<CircuitIdentity, Bundle>,
    location_pattern: Regex,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn is_hash(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

impl ArtifactRegistry {
    pub fn load(directories: &[PathBuf]) -> io::Result<Self> {
        let mut bundles: HashMap<CircuitIdentity, Bundle> = HashMap::new();
        for directory in directories {
            let root = directory.canonicalize()?;
            let manifest: Value =
                serde_json::from_slice(&fs::read(root.join("compiler/contract-manifest.json"))?)?;
            if manifest["manifest-version"] != "1" {
                return Err(invalid("Unsupported artifact manifest version"));
            }
            let keys = manifest["keys"]
                .as_object()
                .ok_or_else(|| invalid("Missing manifest keys"))?;
            let mut count = 0;
            for name in keys.keys() {
                let Some(circuit) = name.strip_suffix(".verifier") else {
                    continue;
                };
                if circuit.is_empty()
                    || !circuit
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'_')
                {
                    return Err(invalid("Invalid circuit name"));
                }
                let bundle = Bundle::load(&root, &manifest, circuit)?;
                let identity = CircuitIdentity {
                    circuit: circuit.to_owned(),
                    verifier_hash: bundle.verifier.hash.clone(),
                };
                if let Some(previous) = bundles.get(&identity) {
                    if previous.prover.hash != bundle.prover.hash
                        || previous.ir.hash != bundle.ir.hash
                    {
                        return Err(invalid(format!(
                            "Conflicting artifacts for circuit {circuit} and verifier {}",
                            identity.verifier_hash,
                        )));
                    }
                } else {
                    bundles.insert(identity, bundle);
                }
                count += 1;
            }
            if count == 0 {
                return Err(invalid("No circuits in configured artifact directory"));
            }
        }
        Ok(Self {
            bundles,
            location_pattern: Regex::new(
                r"\Acontract:[0-9a-fA-F]{64}/(?P<circuit>[A-Za-z0-9_]+)\?vk=(?P<verifier_hash>[0-9a-f]{64})\z",
            )
            .expect("constant pattern"),
        })
    }

    fn bundle(&self, location: &KeyLocation) -> io::Result<Option<&Bundle>> {
        if !location.0.starts_with("contract:") {
            return Ok(None);
        }
        let parts = self
            .location_pattern
            .captures(&location.0)
            .ok_or_else(|| invalid("Malformed contract key location"))?;
        let identity = CircuitIdentity {
            circuit: parts["circuit"].to_owned(),
            verifier_hash: parts["verifier_hash"].to_owned(),
        };
        self.bundles
            .get(&identity)
            .map(Some)
            .ok_or_else(|| invalid(format!("No registered artifacts for {}", location.0)))
    }

    pub(crate) fn resolve_ir(&self, location: &KeyLocation) -> io::Result<Option<Vec<u8>>> {
        let Some(bundle) = self.bundle(location)? else {
            return Ok(None);
        };
        bundle.ir.read().map(Some)
    }

    pub(crate) fn resolve(&self, location: &KeyLocation) -> io::Result<Option<ProvingKeyMaterial>> {
        let Some(bundle) = self.bundle(location)? else {
            return Ok(None);
        };
        Ok(Some(ProvingKeyMaterial {
            prover_key: bundle.prover.read()?,
            verifier_key: bundle.verifier.read()?,
            ir_source: bundle.ir.read()?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("proof-artifacts-{}", uuid::Uuid::new_v4()));
            for dir in ["compiler", "keys", "zkir"] {
                fs::create_dir_all(root.join(dir)).unwrap();
            }
            let mut manifest = json!({
                "manifest-version": "1",
                "keys": {},
                "zkir": {},
            });
            for (dir, name, data) in [
                ("keys", "test.prover", b"prover".as_slice()),
                ("keys", "test.verifier", b"verifier".as_slice()),
                ("zkir", "test.bzkir", b"ir".as_slice()),
            ] {
                fs::write(root.join(dir).join(name), data).unwrap();
                manifest[dir][name] = json!({
                    "type": "file",
                    "size": data.len(),
                    "hash": hex::encode(Sha256::digest(data)),
                });
            }
            fs::write(
                root.join("compiler/contract-manifest.json"),
                manifest.to_string(),
            )
            .unwrap();
            Self(root)
        }

        fn registry(&self) -> ArtifactRegistry {
            ArtifactRegistry::load(std::slice::from_ref(&self.0)).unwrap()
        }

        fn location(&self) -> KeyLocation {
            KeyLocation(
                format!(
                    "contract:{}/test?vk={}",
                    "ab".repeat(32),
                    hex::encode(Sha256::digest(b"verifier"))
                )
                .into(),
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn resolves_registered_material_and_leaves_native_resolution_alone() {
        let fixture = Fixture::new();
        let registry = fixture.registry();
        assert_eq!(
            registry
                .resolve(&fixture.location())
                .unwrap()
                .unwrap()
                .prover_key,
            b"prover"
        );
        assert!(
            registry
                .resolve(&KeyLocation("midnight/zswap/output".into()))
                .unwrap()
                .is_none()
        );
        assert!(
            registry
                .resolve(&KeyLocation("test".into()))
                .unwrap()
                .is_none()
        );
        let other = KeyLocation(
            fixture
                .location()
                .0
                .replace(&"ab".repeat(32), &"CD".repeat(32))
                .into(),
        );
        assert!(registry.resolve(&other).unwrap().is_some());
    }

    #[test]
    fn rejects_unknown_hash_malformed_location_and_traversal() {
        let fixture = Fixture::new();
        let registry = fixture.registry();
        for location in [
            format!("contract:{}/test?vk={}", "ab".repeat(32), "00".repeat(32)),
            format!(
                "contract:{}/../test?vk={}",
                "ab".repeat(32),
                "00".repeat(32)
            ),
            "contract:invalid".to_owned(),
        ] {
            assert!(registry.resolve(&KeyLocation(location.into())).is_err());
        }
    }

    #[test]
    fn check_does_not_read_prover_and_cold_proving_rejects_tampering() {
        let fixture = Fixture::new();
        let registry = fixture.registry();
        fs::write(fixture.0.join("keys/test.prover"), b"broken").unwrap();
        assert_eq!(
            registry.resolve_ir(&fixture.location()).unwrap().unwrap(),
            b"ir"
        );
        assert!(registry.resolve(&fixture.location()).is_err());
    }

    #[test]
    fn validates_material_again_on_each_resolution() {
        let fixture = Fixture::new();
        let registry = fixture.registry();
        assert!(registry.resolve(&fixture.location()).unwrap().is_some());
        fs::write(fixture.0.join("keys/test.prover"), b"broken").unwrap();
        assert!(registry.resolve(&fixture.location()).is_err());
    }

    #[test]
    fn rejects_conflicting_bundles_for_the_same_identity() {
        let first = Fixture::new();
        let second = Fixture::new();
        let path = second.0.join("compiler/contract-manifest.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        fs::write(second.0.join("keys/test.prover"), b"other!").unwrap();
        manifest["keys"]["test.prover"]["hash"] =
            Value::String(hex::encode(Sha256::digest(b"other!")));
        fs::write(path, manifest.to_string()).unwrap();
        assert!(ArtifactRegistry::load(&[first.0.clone(), second.0.clone()]).is_err());
    }

    #[test]
    fn rejects_corrupt_verifier_at_startup() {
        let fixture = Fixture::new();
        fs::write(fixture.0.join("keys/test.verifier"), b"modified").unwrap();
        assert!(ArtifactRegistry::load(std::slice::from_ref(&fixture.0)).is_err());
    }
}
