//! Stress test runner.
//!
//! The test runner is a separate binary because we want to be able to kill the
//! stress test asynchronously. Unlike Haskell, Rust does not support async
//! exceptions, so we can't simply use a thread. But a subprocess can be killed
//! using normal OS process management.
//!
//! As an added bonus, this also allows us to have tests which blow up the
//! stack, which otherwise would kill the whole test process.
//!
//! See [`midnight_storage::stress_test`] for more information.

#[cfg(feature = "parity-db")]
use midnight_storage::db::ParityDb;
#[cfg(feature = "sqlite")]
use midnight_storage::db::SqlDB;

/// A stress testing function as stored in the `TESTS` map.
type TestFn = Box<dyn Fn(&[String]) + Sync + Send>;

// Wrap a no-arg fn to take args and assert they're empty.
fn no_args<F>(f: F) -> TestFn
where
    F: Fn() + Sync + Send + 'static,
{
    Box::new(move |args: &[String]| {
        if !args.is_empty() {
            panic!("this stress test doesn't take any args: {args:?}")
        }
        f();
    })
}

use std::{collections::HashMap, sync::LazyLock};
static TESTS: LazyLock<HashMap<&str, TestFn>> = {
    LazyLock::new(|| {
        HashMap::from([
            (
                "stress_test::stress_tests::run_pass",
                no_args(midnight_storage::stress_test::stress_tests::run_pass),
            ),
            (
                "stress_test::stress_tests::run_fail_oom",
                no_args(midnight_storage::stress_test::stress_tests::run_fail_oom),
            ),
            (
                "stress_test::stress_tests::run_fail_timeout",
                no_args(midnight_storage::stress_test::stress_tests::run_fail_timeout),
            ),
            (
                "stress_test::stress_tests::run_fail_capture_interleave",
                no_args(midnight_storage::stress_test::stress_tests::run_fail_capture_interleave),
            ),
            (
                "stress_test::stress_tests::run_fail_match_output",
                no_args(midnight_storage::stress_test::stress_tests::run_fail_match_output),
            ),
            (
                "arena::stress_tests::array_nesting",
                no_args(midnight_storage::arena::stress_tests::array_nesting),
            ),
            (
                "arena::stress_tests::drop_deeply_nested_data",
                no_args(midnight_storage::arena::stress_tests::drop_deeply_nested_data),
            ),
            (
                "arena::stress_tests::serialize_deeply_nested_data",
                no_args(midnight_storage::arena::stress_tests::serialize_deeply_nested_data),
            ),
            #[cfg(feature = "sqlite")]
            (
                "arena::stress_tests::thrash_the_cache_variations_sqldb",
                Box::new(midnight_storage::arena::stress_tests::thrash_the_cache_variations_sqldb),
            ),
            #[cfg(feature = "parity-db")]
            (
                "arena::stress_tests::thrash_the_cache_variations_paritydb",
                Box::new(
                    midnight_storage::arena::stress_tests::thrash_the_cache_variations_paritydb,
                ),
            ),
            #[cfg(feature = "sqlite")]
            (
                "arena::stress_tests::load_large_tree_sqldb",
                Box::new(midnight_storage::arena::stress_tests::load_large_tree_sqldb),
            ),
            #[cfg(feature = "parity-db")]
            (
                "arena::stress_tests::load_large_tree_paritydb",
                Box::new(midnight_storage::arena::stress_tests::load_large_tree_paritydb),
            ),
            #[cfg(feature = "sqlite")]
            (
                "arena::stress_tests::read_write_map_loop_sqldb",
                Box::new(midnight_storage::arena::stress_tests::read_write_map_loop::<SqlDB>),
            ),
            #[cfg(feature = "parity-db")]
            (
                "arena::stress_tests::read_write_map_loop_paritydb",
                Box::new(midnight_storage::arena::stress_tests::read_write_map_loop::<ParityDb>),
            ),
        ])
    })
};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} TEST_NAME [ARGS]", args[0]);
        std::process::exit(1);
    }
    let test_name = &args[1];
    if let Some(test) = TESTS.get(&test_name[..]) {
        let test_args = args[2..].to_vec();
        test(&test_args);
    } else {
        eprintln!("Unknown test: \"{}\"", args[1]);
        eprintln!("Available tests: {:#?}", TESTS.keys());
        std::process::exit(1);
    }
}
