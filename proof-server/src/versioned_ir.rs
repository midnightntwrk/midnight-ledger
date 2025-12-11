use std::sync::Arc;

use ledger::prove::Resolver;
use rand::rngs::OsRng;
use serialize::tagged_deserialize;
use transient_crypto::proofs::{Proof, ProofPreimage, Zkir};
use zkir as zkir_v2;

use crate::endpoints::PUBLIC_PARAMS;

#[cfg(feature = "experimental")]
pub(crate) fn k(request: &[u8]) -> Result<u8, &'static str> {
    if let Ok(ir_v2) = tagged_deserialize::<zkir_v2::IrSource>(request) {
        Ok(ir_v2.k())
    } else if let Ok(ir_v3) = tagged_deserialize::<zkir_v3::IrSource>(request) {
        Ok(ir_v3.k())
    } else {
        Err("Unsupported ZKIR version")
    }
}

#[cfg(not(feature = "experimental"))]
pub(crate) fn k(request: &[u8]) -> Result<u8, &'static str> {
    if let Ok(ir_v2) = tagged_deserialize::<zkir_v2::IrSource>(request) {
        Ok(ir_v2.k())
    } else {
        Err("Unsupported ZKIR version")
    }
}

#[cfg(feature = "experimental")]
pub(crate) fn check(ppi: Arc<ProofPreimage>, ir: &[u8]) -> Result<Vec<Option<usize>>, String> {
    if let Ok(ir_v2) = tagged_deserialize::<zkir_v2::IrSource>(ir) {
        ppi.check(&ir_v2).map_err(|e| e.to_string())
    } else if let Ok(ir_v3) = tagged_deserialize::<zkir_v3::IrSource>(ir) {
        ppi.check(&ir_v3).map_err(|e| e.to_string())
    } else {
        Err("Unsupported ZKIR version".to_string())
    }
}

#[cfg(not(feature = "experimental"))]
pub(crate) fn check(ppi: Arc<ProofPreimage>, ir: &[u8]) -> Result<Vec<Option<usize>>, String> {
    if let Ok(ir_v2) = tagged_deserialize::<zkir_v2::IrSource>(ir) {
        ppi.check(&ir_v2).map_err(|e| e.to_string())
    } else {
        Err("Unsupported ZKIR version".to_string())
    }
}

#[cfg(feature = "experimental")]
pub(crate) async fn prove(
    ppi: Arc<ProofPreimage>,
    ir_source: &[u8],
    resolver: &Resolver,
) -> Result<(Proof, Vec<Option<usize>>), String> {
    if let Ok(_ir_v2) = tagged_deserialize::<zkir_v2::IrSource>(ir_source) {
        ppi.prove::<zkir_v2::IrSource>(OsRng, &*PUBLIC_PARAMS, resolver)
            .await
            .map_err(|e| e.to_string())
    } else if let Ok(_ir_v3) = tagged_deserialize::<zkir_v3::IrSource>(ir_source) {
        ppi.prove::<zkir_v3::IrSource>(OsRng, &*PUBLIC_PARAMS, resolver)
            .await
            .map_err(|e| e.to_string())
    } else {
        Err("Unsupported ZKIR version".into())
    }
}

#[cfg(not(feature = "experimental"))]
pub(crate) async fn prove(
    ppi: Arc<ProofPreimage>,
    ir_source: &[u8],
    resolver: &Resolver,
) -> Result<(Proof, Vec<Option<usize>>), String> {
    if let Ok(_ir_v2) = tagged_deserialize::<zkir_v2::IrSource>(ir_source) {
        ppi.prove::<zkir_v2::IrSource>(OsRng, &*PUBLIC_PARAMS, resolver)
            .await
            .map_err(|e| e.to_string())
    } else {
        Err("Unsupported ZKIR version".into())
    }
}
