//! SCALE-encode a fully-proven `Transaction` into the byte form
//! `Midnight.send_mn_transaction` expects. Uses ledger's
//! tagged_serialize — the same encoding the indexer surfaces
//! under `Transaction.raw: HexEncoded` in schema-v4.

use serialize::{Serializable, Tagged, tagged_serialize};

use super::TxError;

#[allow(dead_code)] // Wired by Wallet::create_did in Task 11.
pub(crate) fn scale_encode<T: Serializable + Tagged>(
    tx: &T,
) -> Result<Vec<u8>, TxError> {
    let mut buf = Vec::new();
    tagged_serialize(tx, &mut buf)
        .map_err(|e| TxError::ScaleEncode(e.to_string()))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_a_contract_deploy() {
        use crate::did::deploy::compose_deploy;

        let pk = [0xabu8; 32];
        let ts = 1_777_840_000_000u64;
        let nonce = [0x99u8; 32];
        let deploy = compose_deploy(pk, ts, nonce, Vec::new());

        let bytes = scale_encode(&deploy).expect("encode");
        assert!(!bytes.is_empty(), "produced empty bytes");

        let back: ledger::structure::ContractDeploy<storage::DefaultDB> =
            serialize::tagged_deserialize(&bytes[..]).expect("round-trip");
        assert_eq!(deploy.address().0.0, back.address().0.0);
    }
}
