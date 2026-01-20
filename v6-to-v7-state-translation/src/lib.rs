use base_crypto::cost_model::CostDuration;
use std::any::Any;
use std::io;
use storage_v7::{arena::Sp, db::DB, state_translation::*};

struct LedgerV6ToV7Translation;

impl<D: DB, D2: storage_v6::db::DB>
    DirectTranslation<
        ledger_v6::structure::LedgerState<D2>,
        ledger_v7::structure::LedgerState<D>,
        D,
    > for LedgerV6ToV7Translation
{
    fn required_translations() -> Vec<TranslationId> {
        todo!()
    }

    fn child_translations(
        source: &ledger_v6::structure::LedgerState<D2>,
    ) -> Vec<(TranslationId, Sp<dyn Any + Send + Sync, D>)> {
        todo!()
    }

    fn finalize(
        source: &ledger_v6::structure::LedgerState<D2>,
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
