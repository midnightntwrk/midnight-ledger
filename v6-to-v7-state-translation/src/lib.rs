use base_crypto::cost_model::CostDuration;
use serialize::Tagged;
use std::{any::Any, borrow::Cow, io, marker::PhantomData};
use storage::{arena::Sp, db::DB, state_translation::*, storage::Map};

struct LedgerV6ToV7Translation;

impl LedgerV6ToV7Translation {
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
}

impl<D: DB>
    DirectTranslation<ledger_v6::structure::LedgerState<D>, ledger_v7::structure::LedgerState<D>, D>
    for LedgerV6ToV7Translation
{
    fn required_translations() -> Vec<TranslationId> {
        vec![Self::contract_tlid::<D>()]
    }

    fn child_translations(
        source: &ledger_v6::structure::LedgerState<D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        vec![(Self::contract_tlid::<D>(), source.contract.mpt.upcast())]
    }

    fn finalize(
        source: &ledger_v6::structure::LedgerState<D>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<ledger_v7::structure::LedgerState<D>>> {
        let network_id = source.network_id.clone();
        // TODO: recast children
        let Some(contract) =
            cache.lookup(&Self::contract_tlid::<D>(), source.contract.mpt.as_child())
        else {
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

// TODO: resurect MPT state translation
// TODO: contract state translation

struct ContractStateTranslation;

impl<D: DB>
    DirectTranslation<
        onchain_state_v6::state::ContractState<D>,
        onchain_state_v7::state::ContractState<D>,
        D,
    > for ContractStateTranslation
{
    fn finalize(
        source: &onchain_state_v6::state::ContractState<D>,
        limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<onchain_state_v6::state::ContractState<D>>> {
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
