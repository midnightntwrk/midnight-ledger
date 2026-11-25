// This file is part of midnight-ledger.
// Copyright (C) Midnight Foundation
// SPDX-License-Identifier: Apache-2.0
// Licensed under the Apache License, Version 2.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at
// http://www.apache.org/licenses/LICENSE-2.0
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! The time to dismiss a transaction and the most its size allows, as the
//! figures `cost(params, true)` compares and reports when it rejects.

use super::transactions::{generated, recipe};
use crate::structure::*;
use proptest::prelude::*;

/// `params` with the given per-byte and minimum times to dismiss.
fn with_limits(per_byte: CostDuration, min: CostDuration) -> LedgerParameters {
    let mut params = INITIAL_PARAMETERS;
    params.limits.time_to_dismiss_per_byte = per_byte;
    params.limits.min_time_to_dismiss = min;
    params
}

#[test]
fn limits_allowance_is_per_byte_size_floored_at_minimum() {
    let limits = with_limits(
        CostDuration::from_picoseconds(7),
        CostDuration::from_picoseconds(700),
    )
    .limits;
    for (size, expected) in [(0, 700), (99, 700), (100, 700), (101, 707), (1_000, 7_000)] {
        assert_eq!(
            limits.allowed_time_to_dismiss(size),
            CostDuration::from_picoseconds(expected),
            "size {size}"
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn allowance_is_per_byte_size_floored_at_minimum(r in recipe()) {
        let tx = generated(&r, false);
        let size = tx.est_size() as u64;
        // The initial limits, then each term of the `max` made the larger.
        for params in [
            INITIAL_PARAMETERS,
            with_limits(INITIAL_PARAMETERS.limits.time_to_dismiss_per_byte, CostDuration::ZERO),
            with_limits(CostDuration::ZERO, CostDuration::SECOND),
        ] {
            let expected = CostDuration::max(
                params.limits.time_to_dismiss_per_byte * size,
                params.limits.min_time_to_dismiss,
            );
            prop_assert_eq!(tx.allowed_time_to_dismiss(&params), expected);
        }
    }

    #[test]
    fn cost_rejects_exactly_past_allowance_with_both_figures(r in recipe()) {
        let tx = generated(&r, false);
        let Transaction::Standard(stx) = &tx else { unreachable!() };
        prop_assume!(stx.disjoint_check().is_ok());
        let time_to_dismiss = tx.time_to_dismiss(&INITIAL_PARAMETERS).unwrap();

        let at_limit = with_limits(CostDuration::ZERO, time_to_dismiss);
        prop_assert_eq!(tx.allowed_time_to_dismiss(&at_limit), time_to_dismiss);
        prop_assert_eq!(tx.cost(&at_limit, true), tx.cost(&at_limit, false));

        let below = time_to_dismiss - CostDuration::from_picoseconds(1);
        let over_limit = with_limits(CostDuration::ZERO, below);
        prop_assert_eq!(tx.time_to_dismiss(&over_limit), Ok(time_to_dismiss));
        prop_assert_eq!(
            tx.cost(&over_limit, true),
            Err(FeeCalculationError::OutsideTimeToDismiss {
                time_to_dismiss,
                allowed_time_to_dismiss: tx.allowed_time_to_dismiss(&over_limit),
                size: tx.est_size() as u64,
            })
        );
        prop_assert_eq!(tx.allowed_time_to_dismiss(&over_limit), below);
        prop_assert!(tx.cost(&over_limit, false).is_ok());
    }
}
