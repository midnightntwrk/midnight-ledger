use base_crypto::cost_model::CostDuration;
use serialize::Tagged;
use std::{any::Any, borrow::Cow, io, marker::PhantomData};
use storage::{arena::Sp, db::DB, state_translation::*, storage::Map};

struct LedgerV6ToV7Translation;

impl LedgerV6ToV7Translation {
    fn param_tlid() -> TranslationId {
        TranslationId(
            ledger_v6::structure::LedgerParameters::tag(),
            ledger_v7::structure::LedgerParameters::tag(),
        )
    }

    fn bridge_receiving_tlid<D: DB>() -> TranslationId {
        TranslationId(
            Map::<coin_structure_v6::coin::UserAddress, u128, D>::tag(),
            Map::<coin_structure_v7::coin::UserAddress, u128, D>::tag(),
        )
    }

    fn unclaimed_block_rewards_tlid<D: DB>() -> TranslationId {
        TranslationId(
            Map::<coin_structure_v6::coin::UserAddress, u128, D, ledger_v6::annotation::NightAnn>::tag(),
            Map::<coin_structure_v7::coin::UserAddress, u128, D, ledger_v7::annotation::NightAnn>::tag()
        )
    }

    fn treasury_tlid<D: DB>() -> TranslationId {
        TranslationId(
            Map::<coin_structure_v6::coin::TokenType, u128, D>::tag(),
            Map::<coin_structure_v7::coin::TokenType, u128, D>::tag(),
        )
    }

    fn zswap_tlid<D: DB>() -> TranslationId {
        TranslationId(
            zswap_v6::ledger::State::<D>::tag(),
            zswap_v7::ledger::State::<D>::tag(),
        )
    }

    fn contract_tlid<D: DB>() -> TranslationId {
        TranslationId(
            Map::<
                coin_structure_v6::contract::ContractAddress,
                onchain_state_v6::state::ContractState<D>,
                D,
                ledger_v6::annotation::NightAnn,
            >::tag(),
            Map::<
                coin_structure_v7::contract::ContractAddress,
                onchain_state_v7::state::ContractState<D>,
                D,
                ledger_v7::annotation::NightAnn,
            >::tag(),
        )
    }

    fn utxo_tlid<D: DB>() -> TranslationId {
        TranslationId(
            ledger_v6::structure::UtxoState::<D>::tag(),
            ledger_v7::structure::UtxoState::<D>::tag(),
        )
    }

    fn replay_protection_tlid<D: DB>() -> TranslationId {
        TranslationId(
            ledger_v6::structure::ReplayProtectionState::<D>::tag(),
            ledger_v7::structure::ReplayProtectionState::<D>::tag(),
        )
    }

    fn dust_tlid<D: DB>() -> TranslationId {
        TranslationId(
            ledger_v6::dust::DustState::<D>::tag(),
            ledger_v7::dust::DustState::<D>::tag(),
        )
    }
}

impl<D: DB>
    DirectTranslation<ledger_v6::structure::LedgerState<D>, ledger_v7::structure::LedgerState<D>, D>
    for LedgerV6ToV7Translation
{
    fn required_translations() -> Vec<TranslationId> {
        vec![
            Self::param_tlid(),
            Self::bridge_receiving_tlid::<D>(),
            Self::unclaimed_block_rewards_tlid::<D>(),
            Self::treasury_tlid::<D>(),
            Self::zswap_tlid::<D>(),
            Self::contract_tlid::<D>(),
            Self::utxo_tlid::<D>(),
            Self::replay_protection_tlid::<D>(),
            Self::dust_tlid::<D>(),
        ]
    }

    fn child_translations(
        source: &ledger_v6::structure::LedgerState<D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        vec![
            (Self::param_tlid(), source.parameters.upcast()),
            (
                Self::bridge_receiving_tlid::<D>(),
                source.bridge_receiving.mpt.upcast(),
            ),
            (
                Self::unclaimed_block_rewards_tlid::<D>(),
                source.unclaimed_block_rewards.mpt.upcast(),
            ),
            (Self::treasury_tlid::<D>(), source.treasury.mpt.upcast()),
            (Self::zswap_tlid::<D>(), source.zswap.upcast()),
            (Self::contract_tlid::<D>(), source.contract.mpt.upcast()),
            (Self::utxo_tlid::<D>(), source.utxo.upcast()),
            (
                Self::replay_protection_tlid::<D>(),
                source.replay_protection.upcast(),
            ),
            (Self::dust_tlid::<D>(), source.dust.upcast()),
        ]
    }

    fn finalize(
        source: &ledger_v6::structure::LedgerState<D>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<ledger_v7::structure::LedgerState<D>>> {
        let network_id = source.network_id.clone();
        let Some(parameters) = cache.lookup(&Self::param_tlid(), source.parameters.as_child())
        else {
            return Ok(None);
        };
        let locked_pool = source.locked_pool;
        let Some(bridge_receiving) = cache.lookup(
            &Self::bridge_receiving_tlid::<D>(),
            source.bridge_receiving.mpt.as_child(),
        ) else {
            return Ok(None);
        };
        let reserve_pool = source.reserve_pool;
        let block_reward_pool = source.block_reward_pool;
        let Some(unclaimed_block_rewards) = cache.lookup(
            &Self::unclaimed_block_rewards_tlid::<D>(),
            source.unclaimed_block_rewards.mpt.as_child(),
        ) else {
            return Ok(None);
        };
        let Some(treasury) =
            cache.lookup(&Self::treasury_tlid::<D>(), source.treasury.mpt.as_child())
        else {
            return Ok(None);
        };
        let Some(zswap) = cache.lookup(&Self::zswap_tlid::<D>(), source.zswap.as_child()) else {
            return Ok(None);
        };
        let Some(contract) =
            cache.lookup(&Self::contract_tlid::<D>(), source.contract.mpt.as_child())
        else {
            return Ok(None);
        };
        let Some(utxo) = cache.lookup(&Self::utxo_tlid::<D>(), source.utxo.as_child()) else {
            return Ok(None);
        };
        let Some(replay_protection) = cache.lookup(
            &Self::replay_protection_tlid::<D>(),
            source.replay_protection.as_child(),
        ) else {
            return Ok(None);
        };
        let Some(dust) = cache.lookup(&Self::dust_tlid::<D>(), source.dust.as_child()) else {
            return Ok(None);
        };

        Ok(Some(ledger_v7::structure::LedgerState {
            network_id,
            parameters: parameters.force_downcast(),
            locked_pool,
            bridge_receiving: Map {
                mpt: bridge_receiving.force_downcast(),
                key_type: PhantomData,
            },
            reserve_pool,
            block_reward_pool,
            unclaimed_block_rewards: Map {
                mpt: unclaimed_block_rewards.force_downcast(),
                key_type: PhantomData,
            },
            treasury: Map {
                mpt: treasury.force_downcast(),
                key_type: PhantomData,
            },
            zswap: zswap.force_downcast(),
            contract: Map {
                mpt: contract.force_downcast(),
                key_type: PhantomData,
            },
            utxo: utxo.force_downcast(),
            replay_protection: replay_protection.force_downcast(),
            dust: dust.force_downcast(),
        }))
    }
}

pub struct StateTranslationTable;

impl<D: DB> TranslationTable<D> for StateTranslationTable {
    const TABLE: &[(TranslationId, &dyn TypelessTranslation<D>)] = &[(
        TranslationId(
            Cow::Borrowed("ledger-state[v12]"),
            Cow::Borrowed("ledger-state[v13]"),
        ),
        &DirectSpTranslation::<_, _, LedgerV6ToV7Translation, _>(PhantomData),
    )];
}
