use base_crypto::cost_model::CostDuration;
use serialize::Tagged;
use std::{any::Any, borrow::Cow, io, marker::PhantomData};
use storage::{arena::Sp, db::DB, state_translation::*, storage::Map};

struct LedgerV6ToV7Translation;

impl<D: DB>
    DirectTranslation<ledger_v6::structure::LedgerState<D>, ledger_v7::structure::LedgerState<D>, D>
    for LedgerV6ToV7Translation
{
    fn required_translations() -> Vec<TranslationId> {
        vec![TranslationId(
            ledger_v6::structure::LedgerParameters::tag(),
            ledger_v7::structure::LedgerParameters::tag(),
        ),
        TranslationId(
            Map::<coin_structure_v6::coin::UserAddress, u128, D, ledger_v6::annotation::NightAnn>::tag(),
            Map::<coin_structure_v7::coin::UserAddress, u128, D, ledger_v7::annotation::NightAnn>::tag()
        ),
        TranslationId(
            Map::<coin_structure_v6::coin::TokenType, u128, D>::tag(),
            Map::<coin_structure_v7::coin::TokenType, u128, D>::tag(),
        ),
        TranslationId(
            zswap_v6::ledger::State::<D>::tag(), 
            zswap_v7::ledger::State::<D>::tag(), 
        ),
        TranslationId(
            Map::<coin_structure_v6::contract::ContractAddress, ledger_v6::verify::ContractState<D>, D, ledger_v6::annotation::NightAnn>::tag(),
            Map::<coin_structure_v7::contract::ContractAddress, ledger_v7::verify::ContractState<D>, D, ledger_v7::annotation::NightAnn>::tag(),
        ),
        TranlationId(
            ledger_v6::structure::UtxoState::<D>::tag(),
            ledger_v7::structure::UtxoState::<D>::tag(),
        ),
        TranslationId(
            ledger_v6::structure::ReplayProtectionState::<D>::tag(),
            ledger_v7::structure::ReplayProtectionState::<D>::tag(),
        ),
        TranslationId(
            ledger_v6::structure::DustState::<D>::tag(),
            ledger_v7::structure::DustState::<D>::tag(),
        )
        ]
    }

    fn child_translations(
        source: &ledger_v6::structure::LedgerState<D>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        todo!()
    }

    fn finalize(
        source: &ledger_v6::structure::LedgerState<D>,
        _limit: &mut CostDuration,
        cache: &TranslationCache<D>,
    ) -> io::Result<Option<ledger_v7::structure::LedgerState<D>>> {
        todo!()
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
