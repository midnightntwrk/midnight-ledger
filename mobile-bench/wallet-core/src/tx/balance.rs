//! Cover the DUST fees of an unproven deploy by spending UTXOs
//! from the wallet's DustLocalState. Ported from
//! `ledger::test_utilities::TestState::balance_tx`'s DUST branch
//! (test_utilities.rs:572-643), simplified to the deploy case:
//! no shielded coins, no fallible segments.

use base_crypto::signatures::{Signature, SigningKey};
use base_crypto::time::Timestamp;
use coin_structure::coin::{Info as CoinInfo, QualifiedInfo as QualifiedCoinInfo, TokenType};
use ledger::dust::{DustActions, DustLocalState, DustSecretKey};
use ledger::structure::{
    Intent, LedgerParameters, ProofPreimageMarker, StandardTransaction, Transaction,
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use storage::DefaultDB;
use storage::arena::Sp;
use storage::storage::{Array, HashMap};
use transient_crypto::commitment::PedersenRandomness;
use zswap::keys::SecretKeys;
use zswap::local::State as ZswapState;
use zswap::{Offer as ZswapOffer, Output as ZswapOutput};

use super::TxError;
use super::build::UnprovenTx;

/// Segment slot for the dust-balance Intent. Must NOT collide
/// with the guaranteed segment (0) where the deploy lives.
/// Matches test_utilities::balance_tx's choice (0xFEED).
const DUST_BALANCE_SEGMENT: u16 = 0xFEED;

pub(crate) struct BalanceCtx<'a> {
    pub dust_state: &'a mut DustLocalState<DefaultDB>,
    pub dust_key: &'a DustSecretKey,
    pub params: &'a LedgerParameters,
    /// Current chain time — used by `dust_state.spend()` to age
    /// DUST UTXOs against the spend's nominal timestamp.
    pub time: Timestamp,
    /// Time-to-live for the dust intent. Must be ≥ current chain
    /// `tblock + slot_duration + skipped_margin` once validation
    /// runs. Same value the deploy intent uses (`time + 3600`).
    pub ttl: Timestamp,
    pub network_id: &'a str,
}

#[allow(dead_code)] // Wired by Wallet::create_did in Task 11.
pub(crate) fn balance(
    tx: UnprovenTx,
    ctx: &mut BalanceCtx<'_>,
) -> Result<UnprovenTx, TxError> {
    // Snapshot the input tx and the input dust state so each
    // iteration can rebuild the dust intent from scratch (matches
    // test_utilities::balance_tx's pattern — it merges the latest
    // dust intent into the ORIGINAL tx each iteration, never
    // accumulating across iterations, so the segment key
    // 0xFEED doesn't collide on repeated merges).
    let original_tx = tx.clone();
    let original_dust = ctx.dust_state.clone();
    // From-entropy, not seed-0. The dust intent's hash includes
    // its `binding_commitment` (rng-derived) plus ttl, but NOT
    // its `dust_actions` (see `to_hash_data` in
    // `ledger/src/structure.rs:883`). Two attempts within the
    // same second with the same RNG seed would produce the same
    // intent hash, and the chain's replay-protection map would
    // reject the second as `IntentAlreadyExists` — surfaces as
    // `Malformed(TransactionApplicationError)` → `Invalid
    // Transaction (1010)`.
    let mut rng = ChaCha20Rng::from_entropy();
    let mut last_dust: u128 = 0;
    let mut current = tx;

    loop {
        let fees = current
            .fees(ctx.params, false)
            .map_err(|e| TxError::Balance(format!("fees: {e}")))?;
        let balance_map = current
            .balance(Some(fees))
            .map_err(|e| TxError::Balance(format!("balance: {e}")))?;
        let dust_short = balance_map
            .get(&(TokenType::Dust, 0))
            .and_then(|v| (*v < 0).then_some((-*v) as u128))
            .unwrap_or(0);
        if dust_short == 0 {
            return Ok(current);
        }

        let dust_to_cover = dust_short + last_dust;
        last_dust = dust_to_cover;

        // Reset dust_state to the input snapshot — the loop's
        // earlier spends are abandoned. The intent we build below
        // covers the full running total, not just the increment.
        *ctx.dust_state = original_dust.clone();

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
            // Skip fully-decayed UTXOs. A UTXO whose backing NIGHT was already
            // spent (finite `gen.dtime` in the past) has decayed to a current
            // value of 0, so it contributes nothing to the fee. Building a
            // spend for it is not just wasteful — it produces a witness the
            // dust-spend circuit rejects: Rust's `updated_value` clamps to 0
            // via `saturating_sub`, but the Compact circuit's exact field
            // arithmetic for a UTXO whose decay term exceeds its accrued value
            // disagrees, so `assert(v == updatedValue())` (dust.compact) fails
            // at prove time as "Failed direct assertion". Single-tenant
            // standalone wallets never hit this (their dust never decays); a
            // long-lived wallet on a shared network (preprod) accumulates such
            // residual UTXOs and trips it on the first deploy/call.
            if current_value == 0 {
                continue;
            }
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
            Intent::empty(&mut rng, ctx.ttl);
        intent.dust_actions = Some(Sp::new(DustActions {
            spends,
            registrations: Array::new(),
            ctime: ctx.time,
        }));
        let mut intents = HashMap::new();
        intents = intents.insert(DUST_BALANCE_SEGMENT, intent);
        let merge_with = Transaction::Standard(StandardTransaction::new(
            ctx.network_id,
            intents,
            None,
            HashMap::new(),
        ));
        // Merge into the ORIGINAL — every iteration produces a
        // single-dust-intent tx, never accumulating.
        current = original_tx
            .merge(&merge_with)
            .map_err(|e| TxError::Balance(format!("merge dust intent: {e}")))?;
    }
}

/// Shielded pre-pass: cover the tx's shielded shortfall by spending a
/// specific wallet-owned coin. Used by the vault deposit, where
/// `depositFunds(coin)` re-homes a chosen note to the contract: the
/// compose builds the contract-owned output, and we add the matching
/// wallet `Input` (with its Merkle path) here. Mirrors the shielded
/// branch of `ledger::test_utilities::balance_tx`, but spends the
/// caller-selected `coin` rather than an arbitrary cover set so the
/// note re-homed on-chain is exactly the one declared in the circuit
/// arg. If the coin exceeds the shortfall, the remainder is returned to
/// the wallet as a change output. Returns the tx unchanged when there
/// is no shielded shortfall (e.g. claim, whose input is contract-owned
/// and supplied by the compose).
///
/// The returned tx is still unproven — the downstream `prove` step
/// (`tx/prove.rs`, whose resolver bundles the zswap spend/output keys)
/// proves the spend + change alongside the contract call.
#[allow(dead_code)] // Wired by Wallet::vault_deposit in the deposit slice.
pub(crate) fn balance_shielded_with_coin(
    tx: UnprovenTx,
    coin: &QualifiedCoinInfo,
    zswap_state: &mut ZswapState<DefaultDB>,
    keys: &SecretKeys,
    params: &LedgerParameters,
    network_id: &str,
) -> Result<UnprovenTx, TxError> {
    let mut rng = ChaCha20Rng::from_entropy();
    let fees = tx
        .fees(params, false)
        .map_err(|e| TxError::Balance(format!("fees: {e}")))?;
    let balance_map = tx
        .balance(Some(fees))
        .map_err(|e| TxError::Balance(format!("balance: {e}")))?;
    // The deposit produces exactly one shielded shortfall, for the
    // coin's token type. `None` => nothing to balance (claim path).
    let shortfall = balance_map.iter().find_map(|((tt, seg), v)| match tt {
        TokenType::Shielded(stt) if *v < 0 && *stt == coin.type_ => {
            Some((*seg, (-*v) as u128))
        }
        _ => None,
    });
    let Some((seg, short)) = shortfall else {
        return Ok(tx);
    };

    let (next_state, input) = zswap_state
        .spend(&mut rng, keys, coin, Some(seg))
        .map_err(|e| TxError::Balance(format!("zswap spend: {e}")))?;
    *zswap_state = next_state;

    let mut outputs = Vec::new();
    if coin.value > short {
        let change = CoinInfo {
            nonce: rng.r#gen(),
            type_: coin.type_,
            value: coin.value - short,
        };
        let out = ZswapOutput::new(
            &mut rng,
            &change,
            Some(seg),
            &keys.coin_public_key(),
            Some(keys.enc_public_key()),
        )
        .map_err(|e| TxError::Balance(format!("zswap change output: {e}")))?;
        outputs.push(out);
    }

    let offer = ZswapOffer::new(vec![input], outputs, vec![])
        .ok_or_else(|| TxError::Balance("empty zswap offer".into()))?;
    let merge_with = if seg == 0 {
        Transaction::Standard(StandardTransaction::new(
            network_id,
            HashMap::new(),
            Some(offer),
            HashMap::new(),
        ))
    } else {
        let mut fallible = HashMap::new();
        fallible = fallible.insert(seg, offer);
        Transaction::Standard(StandardTransaction::new(
            network_id,
            HashMap::new(),
            None,
            fallible,
        ))
    };
    tx.merge(&merge_with)
        .map_err(|e| TxError::Balance(format!("merge zswap offer: {e}")))
}

/// Segment slot for the unshielded-balance Intent. Distinct from the dust
/// (`0xFEED`) and maintenance (`1`) segments and from the contract-call
/// segment, so merging never trips `IntentSegmentIdCollision`. The funding
/// offer is placed in `guaranteed_unshielded_offer`, which the ledger always
/// accounts at balance-segment 0 — exactly where the contract's guaranteed
/// `receiveUnshielded` lands — so the two net to zero.
const UNSHIELDED_BALANCE_SEGMENT: u16 = 0xBEEF;

/// Unshielded pre-pass: cover the tx's native-NIGHT unshielded shortfall
/// (e.g. a contract `depositFunds` whose `receiveUnshielded(nativeToken(), n)`
/// pulls `n` NIGHT into the contract) by spending wallet-owned NIGHT UTXOs and
/// returning change to the wallet.
///
/// Crucially, this **signs** every funding input with the wallet's night key,
/// so the number of unshielded inputs matches the number of signatures. The
/// JS wallet SDK gets this wrong for contract-funding inputs, which the node
/// rejects with `1010 Custom error 192 (InputsSignaturesLengthMismatch)`;
/// signing here in Rust is what lets the Dioxus lock-funds path succeed.
///
/// Mirrors `balance_shielded_with_coin` but for the unshielded UTXO model.
/// Returns the tx unchanged when there is no unshielded shortfall (e.g. the
/// claim path, whose payout is debited from the contract's own balance).
#[allow(dead_code)] // Wired by Wallet::vault_deposit in the unshielded deposit slice.
pub(crate) fn balance_unshielded(
    tx: UnprovenTx,
    utxos: &crate::unshielded::UtxoSet,
    night_sk: &SigningKey,
    ttl: Timestamp,
    network_id: &str,
) -> Result<UnprovenTx, TxError> {
    use base_crypto::hash::HashOutput;
    use coin_structure::coin::{NIGHT, UserAddress};
    use ledger::structure::{IntentHash, UnshieldedOffer, UtxoOutput, UtxoSpend};

    let balance_map = tx
        .balance(None)
        .map_err(|e| TxError::Balance(format!("balance: {e}")))?;
    // Find the native-NIGHT shortfall AND its segment. The contract's
    // `receiveUnshielded` nets at balance-segment 0 if it's in the GUARANTEED
    // transcript, or at the contract intent's own segment if FALLIBLE. We must
    // fund at that same segment, else the ledger reports `BalanceCheckOverspend`
    // (1010 Custom error 138). `None` => nothing to fund (e.g. the claim path).
    for ((tt, seg), v) in balance_map.iter() {
        tracing::info!(target: "wallet_core", token = ?tt, segment = *seg, value = *v, "unshielded balance map");
    }
    let Some((short_seg, short)) = balance_map.iter().find_map(|((tt, seg), v)| match tt {
        TokenType::Unshielded(utt) if *utt == NIGHT && *v < 0 => Some((*seg, (-*v) as u128)),
        _ => None,
    }) else {
        return Ok(tx);
    };

    // Native unshielded NIGHT is the all-zero 32-byte token type at the indexer.
    let night_token = crate::unshielded::TokenType(vec![0u8; 32]);
    let picked = utxos.pick_for_amount(&night_token, short).ok_or_else(|| {
        TxError::Balance(format!(
            "insufficient unshielded NIGHT: need {short} base units \
             (fund the wallet's unshielded balance via the faucet, then re-sync)"
        ))
    })?;

    let night_vk = night_sk.verifying_key();
    let mut total: u128 = 0;
    let mut inputs: Vec<UtxoSpend> = Vec::with_capacity(picked.len());
    for u in &picked {
        total = total.saturating_add(u.value);
        inputs.push(UtxoSpend {
            value: u.value,
            owner: night_vk.clone(),
            type_: NIGHT,
            intent_hash: IntentHash(HashOutput(u.id.intent_hash)),
            output_no: u.id.output_index,
        });
    }
    let mut outputs: Vec<UtxoOutput> = Vec::new();
    if total > short {
        outputs.push(UtxoOutput {
            value: total - short,
            owner: UserAddress::from(night_vk),
            type_: NIGHT,
        });
    }
    // The ledger requires inputs + outputs sorted (errors 189/190).
    inputs.sort();
    outputs.sort();
    let n_inputs = inputs.len();
    tracing::info!(
        target: "wallet_core",
        short,
        short_seg,
        total,
        n_inputs,
        change = total.saturating_sub(short),
        n_outputs = outputs.len(),
        "unshielded balance: funding receiveUnshielded with signed NIGHT spend(s)"
    );

    let offer: UnshieldedOffer<Signature, DefaultDB> = UnshieldedOffer {
        inputs: inputs.into(),
        outputs: outputs.into(),
        signatures: Array::new(),
    };
    let mut rng = ChaCha20Rng::from_entropy();
    // One signature per input, all from the wallet's night key (every funding
    // UTXO is wallet-owned). `Intent::sign` checks `owner == sk.verifying_key()`
    // and emits exactly one signature per input → counts match (no 192).
    let signing_keys: Vec<SigningKey> =
        std::iter::repeat(night_sk.clone()).take(n_inputs).collect();

    if short_seg == 0 {
        // Guaranteed receive: a standalone intent's guaranteed offer nets at
        // segment 0, so merge it in (no collision — fresh segment).
        let mut intent: Intent<Signature, ProofPreimageMarker, PedersenRandomness, DefaultDB> =
            Intent::empty(&mut rng, ttl);
        intent.guaranteed_unshielded_offer = Some(Sp::new(offer));
        let signed = intent
            .sign(&mut rng, UNSHIELDED_BALANCE_SEGMENT, &signing_keys, &[], &[])
            .map_err(|e| TxError::Balance(format!("sign unshielded offer: {e:?}")))?;
        let mut intents = HashMap::new();
        intents = intents.insert(UNSHIELDED_BALANCE_SEGMENT, signed);
        let merge_with = Transaction::Standard(StandardTransaction::new(
            network_id,
            intents,
            None,
            HashMap::new(),
        ));
        return tx
            .merge(&merge_with)
            .map_err(|e| TxError::Balance(format!("merge unshielded offer: {e}")));
    }

    // Fallible receive: the funding offer must live INSIDE the contract intent
    // (which sits at `short_seg`), because `merge` rejects two intents sharing a
    // segment. Graft the offer onto that intent's fallible slot and sign it.
    let Transaction::Standard(stx) = tx else {
        return Err(TxError::Balance("unproven vault tx is not a Standard tx".into()));
    };
    let mut new_intents = HashMap::new();
    let mut grafted = false;
    for (seg, intent) in stx.intents.clone().into_iter() {
        if seg == short_seg {
            let mut modified = intent;
            modified.fallible_unshielded_offer = Some(Sp::new(offer.clone()));
            let signed = modified
                .sign(&mut rng, seg, &[], &signing_keys, &[])
                .map_err(|e| TxError::Balance(format!("sign unshielded offer: {e:?}")))?;
            new_intents = new_intents.insert(seg, signed);
            grafted = true;
        } else {
            new_intents = new_intents.insert(seg, intent);
        }
    }
    if !grafted {
        return Err(TxError::Balance(format!(
            "no intent at shortfall segment {short_seg} to fund the receiveUnshielded"
        )));
    }
    Ok(Transaction::Standard(StandardTransaction {
        network_id: stx.network_id,
        intents: new_intents,
        guaranteed_coins: stx.guaranteed_coins,
        fallible_coins: stx.fallible_coins,
        binding_randomness: stx.binding_randomness,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Compile-only typecheck. The real exercise is the live
    /// integration test — synthesising a populated DustLocalState
    /// fixture isn't worth the code at this layer.
    #[test]
    fn signature_typechecks() {
        let _: fn(UnprovenTx, &mut BalanceCtx<'_>) -> Result<UnprovenTx, TxError> = balance;
        let _: fn(
            UnprovenTx,
            &QualifiedCoinInfo,
            &mut ZswapState<DefaultDB>,
            &SecretKeys,
            &LedgerParameters,
            &str,
        ) -> Result<UnprovenTx, TxError> = balance_shielded_with_coin;
    }
}
