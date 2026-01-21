use base_crypto::cost_model::CostDuration;
use std::any::Any;
use std::io;
use storage::{arena::Sp, db::DB, state_translation::*};

struct LedgerV6ToV7Translation;

impl<D: DB>
    DirectTranslation<ledger_v6::structure::LedgerState<D>, ledger_v7::structure::LedgerState<D>, D>
    for LedgerV6ToV7Translation
{
    fn required_translations() -> Vec<TranslationId> {
        todo!()
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

struct StateTranslationTable;

impl<D: DB> TranslationTable<D> for StateTranslationTable {
    const TABLE: &[(TranslationId, &dyn TypelessTranslation<D>)] = &[];
}
