//! Cover the DUST fees of an unproven deploy by spending UTXOs
//! from the wallet's DustLocalState. Ported from
//! `ledger::test_utilities::TestState::balance_tx`'s DUST branch
//! (test_utilities.rs:572-643), simplified to the deploy case:
//! no shielded coins, no fallible segments.

use base_crypto::signatures::Signature;
use base_crypto::time::Timestamp;
use coin_structure::coin::TokenType;
use ledger::dust::{DustActions, DustLocalState, DustSecretKey};
use ledger::structure::{
    GUARANTEED_SEGMENT, Intent, LedgerParameters, ProofPreimageMarker, StandardTransaction,
    Transaction,
};
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use storage::DefaultDB;
use storage::arena::Sp;
use storage::storage::{Array, HashMap};
use transient_crypto::commitment::PedersenRandomness;

use super::TxError;
use super::build::UnprovenTx;

pub(crate) struct BalanceCtx<'a> {
    pub dust_state: &'a mut DustLocalState<DefaultDB>,
    pub dust_key: &'a DustSecretKey,
    pub params: &'a LedgerParameters,
    pub time: Timestamp,
    pub network_id: &'a str,
}

#[allow(dead_code)] // Wired by Wallet::create_did in Task 11.
pub(crate) fn balance(
    mut tx: UnprovenTx,
    ctx: &mut BalanceCtx<'_>,
) -> Result<UnprovenTx, TxError> {
    let mut rng = ChaCha20Rng::seed_from_u64(0);
    let mut last_dust: u128 = 0;

    loop {
        let fees = tx
            .fees(ctx.params, false)
            .map_err(|e| TxError::Balance(format!("fees: {e}")))?;
        let balance_map = tx
            .balance(Some(fees))
            .map_err(|e| TxError::Balance(format!("balance: {e}")))?;
        let dust_short = balance_map
            .get(&(TokenType::Dust, 0))
            .and_then(|v| (*v < 0).then_some((-*v) as u128))
            .unwrap_or(0);
        if dust_short == 0 {
            return Ok(tx);
        }

        let dust_to_cover = dust_short + last_dust;
        last_dust = dust_to_cover;

        let mut spends = Array::new();
        let utxos: Vec<_> = ctx.dust_state.utxos().collect();
        let mut remaining = dust_to_cover;
        for qdo in utxos {
            if remaining == 0 {
                break;
            }
            let gen_info = ctx
                .dust_state
                .generation_info(&qdo)
                .ok_or_else(|| TxError::Balance("missing generation info".into()))?;
            let current_value = ledger::dust::DustOutput::from(qdo.clone()).updated_value(
                &gen_info,
                ctx.time,
                &ctx.params.dust,
            );
            let v = u128::min(current_value, remaining);
            remaining = remaining.saturating_sub(current_value);
            let (next_state, spend) = ctx
                .dust_state
                .clone()
                .spend(ctx.dust_key, &qdo, v, ctx.time)
                .map_err(|e| TxError::Balance(format!("dust spend: {e}")))?;
            *ctx.dust_state = next_state;
            spends = spends.push(spend);
        }
        if remaining > 0 {
            return Err(TxError::Balance(format!(
                "insufficient DUST: short by {remaining} atomic units"
            )));
        }

        let mut intent: Intent<Signature, ProofPreimageMarker, PedersenRandomness, DefaultDB> =
            Intent::empty(&mut rng, ctx.time);
        intent.dust_actions = Some(Sp::new(DustActions {
            spends,
            registrations: Array::new(),
            ctime: ctx.time,
        }));
        let mut intents = HashMap::new();
        intents = intents.insert(GUARANTEED_SEGMENT, intent);
        let merge_with = Transaction::Standard(StandardTransaction::new(
            ctx.network_id,
            intents,
            None,
            HashMap::new(),
        ));
        tx = tx
            .merge(&merge_with)
            .map_err(|e| TxError::Balance(format!("merge dust intent: {e}")))?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-only typecheck. The real exercise is Task 12's
    /// live integration test — synthesising a populated
    /// DustLocalState fixture isn't worth the code at this layer.
    #[test]
    fn signature_typechecks() {
        let _: fn(UnprovenTx, &mut BalanceCtx<'_>) -> Result<UnprovenTx, TxError> = balance;
    }
}
