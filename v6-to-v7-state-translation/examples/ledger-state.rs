use base_crypto::cost_model::CostDuration;
use storage::arena::Sp;
use storage::db::InMemoryDB;
use storage::state_translation::TypedTranslationState;
use v6_to_v7_state_translation::StateTranslationTable;

fn main() {
    let v6_state = ledger_v6::structure::LedgerState::new("local-test");
    let tl_state = TypedTranslationState::<
        ledger_v6::structure::LedgerState<InMemoryDB>,
        ledger_v7::structure::LedgerState<InMemoryDB>,
        StateTranslationTable,
        InMemoryDB,
    >::start(Sp::new(v6_state))
    .unwrap();
    let cost = CostDuration::from_picoseconds(1_000_000_000_000);

    let mut v7_state = tl_state.run(cost).unwrap();
    while None == v7_state.result().unwrap() {
        v7_state = tl_state.run(cost).unwrap();
    }
}
