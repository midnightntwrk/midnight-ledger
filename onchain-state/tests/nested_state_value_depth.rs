// Independent-report vector: deeply nested one-element `StateValue::Array`
// values. Every node is a legitimate production state variant satisfying the
// 16-element array bound, so this reaches the arena decoder without relying on
// any malformed MPT path.

use midnight_onchain_state::state::StateValue;
use serialize::Serializable;
use storage::DefaultDB;
use storage::storage::default_storage;

fn nested_arrays(depth: usize) -> StateValue<DefaultDB> {
    let mut value = StateValue::Null;
    for _ in 0..depth {
        value = StateValue::Array(vec![value].into());
    }
    value
}

/// Serializes a depth-`depth` chain, then decodes it in a fresh arena.
/// Returns the payload size and the decode outcome.
fn round_trip(depth: usize) -> (usize, Result<(), String>) {
    // The state is built through the default storage, the same arena the
    // decoder allocates into.
    let arena = default_storage::<DefaultDB>().arena.clone();
    let sp = arena.alloc(nested_arrays(depth));
    let mut bytes = Vec::new();
    sp.serialize(&mut bytes).unwrap();

    let res = arena.deserialize_sp::<StateValue<DefaultDB>, _>(&mut &bytes[..], 0);
    let outcome = match res {
        Ok(_) => Ok(()),
        Err(e) => Err(e.to_string()),
    };
    (bytes.len(), outcome)
}

#[test]
fn nested_state_value_arrays_are_depth_bounded() {
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            // Shallow nesting must still decode: the bound must not break
            // legitimate contract state.
            let (size, outcome) = round_trip(5);
            println!("depth    5: {size:>6} bytes -> {outcome:?}");
            assert!(outcome.is_ok(), "shallow nesting must still decode");

            // The report's control case: decoded successfully before the fix,
            // despite RECURSION_LIMIT being 250.
            let (size, outcome) = round_trip(300);
            println!("depth  300: {size:>6} bytes -> {outcome:?}");
            assert!(outcome.is_err(), "depth 300 must be rejected");

            // The report's aborting case.
            let (size, outcome) = round_trip(1_500);
            println!("depth 1500: {size:>6} bytes -> {outcome:?}");
            assert!(outcome.is_err(), "depth 1500 must be rejected");
        })
        .unwrap()
        .join()
        .unwrap();
}
