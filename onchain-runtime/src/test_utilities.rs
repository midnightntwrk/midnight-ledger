// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
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

use storage::db::DB;

use crate::cost_model::INITIAL_COST_MODEL;
use crate::ops::*;
use crate::result_mode::*;
use crate::vm_error::OnchainProgramError;
use crate::vm_value::VmValue;

#[allow(clippy::type_complexity)]
pub fn run_program<D: DB, M: ResultMode<D>>(
    initial: &[VmValue<D>],
    program: &[Op<M, D>],
) -> Result<(Vec<VmValue<D>>, Vec<M::Event>), OnchainProgramError<D>> {
    let step_limit = None;
    run_program_step_limited(initial, program, step_limit)
}

#[allow(clippy::type_complexity)]
pub fn run_program_step_limited<D: DB, M: ResultMode<D>>(
    initial: &[VmValue<D>],
    program: &[Op<M, D>],
    step_limit: Option<usize>,
) -> Result<(Vec<VmValue<D>>, Vec<M::Event>), OnchainProgramError<D>> {
    let gas_limit = None;
    let res = crate::vm::run_program_step_limited(
        initial,
        program,
        step_limit,
        gas_limit,
        &INITIAL_COST_MODEL,
    )?;
    Ok((res.stack, res.events))
}
