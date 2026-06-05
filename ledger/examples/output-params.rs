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

//! Outputs the current parameters as a serialized file, ready for constructing a parameter update
//! transaction with the toolkit. This file serves a practical purpose to update the parameters,
//! and as a paper-trail for why an update was performed.

use std::path::PathBuf;

use base_crypto::{
    cost_model::{CostDuration, SyntheticCost},
    time::Duration,
};
use midnight_ledger::structure::{
    INITIAL_LIMITS, INITIAL_PARAMETERS, INITIAL_TRANSACTION_COST_MODEL, LedgerParameters,
    TransactionCostModel, TransactionLimits,
};
use onchain_vm::cost_model::{CostModel, INITIAL_COST_MODEL};
use serialize::tagged_serialize;

// As output by generate-cost-model on our benchmark machine; ran against ledger-8.1.0-rc.1
const UPDATED_COST_MODEL: CostModel = CostModel {
    read_time_batched_4k: INITIAL_COST_MODEL.read_time_batched_4k,
    read_time_synchronous_4k: INITIAL_COST_MODEL.read_time_synchronous_4k,
    idxc_map_constant: CostDuration::from_picoseconds(6732706),
    idxc_map_coeff_key_size: CostDuration::from_picoseconds(9259),
    idxc_map_coeff_container_log_size: CostDuration::from_picoseconds(19174),
    idxc_array: CostDuration::from_picoseconds(6699277),
    idxc_bmt_constant: CostDuration::from_picoseconds(5733330),
    idxc_bmt_coeff_container_log_size: CostDuration::from_picoseconds(2719),
    idxc_bmt_coeff_key_size: CostDuration::from_picoseconds(0),
    ins_bmt_constant: CostDuration::from_picoseconds(0),
    ins_bmt_coeff_container_log_size: CostDuration::from_picoseconds(79117465),
    ins_bmt_coeff_key_size: CostDuration::from_picoseconds(0),
    ins_array: CostDuration::from_picoseconds(15357618),
    ins_map_constant: CostDuration::from_picoseconds(24461203),
    ins_map_coeff_container_log_size: CostDuration::from_picoseconds(3837901),
    ins_map_coeff_key_size: CostDuration::from_picoseconds(0),
    insc_map_constant: CostDuration::from_picoseconds(19107146),
    insc_map_coeff_container_log_size: CostDuration::from_picoseconds(2786533),
    insc_map_coeff_key_size: CostDuration::from_picoseconds(16322),
    insc_array: CostDuration::from_picoseconds(15363397),
    insc_bmt_constant: CostDuration::from_picoseconds(0),
    insc_bmt_coeff_key_size: CostDuration::from_picoseconds(0),
    insc_bmt_coeff_container_log_size: CostDuration::from_picoseconds(80546170),
    idx_map_constant: CostDuration::from_picoseconds(6296427),
    idx_map_coeff_key_size: CostDuration::from_picoseconds(8572),
    idx_map_coeff_container_log_size: CostDuration::from_picoseconds(14000),
    idx_array: CostDuration::from_picoseconds(6450697),
    idx_bmt_constant: CostDuration::from_picoseconds(5403970),
    idx_bmt_coeff_container_log_size: CostDuration::from_picoseconds(9627),
    idx_bmt_coeff_key_size: CostDuration::from_picoseconds(96494),
    new_null: CostDuration::from_picoseconds(561022),
    new_bmt: CostDuration::from_picoseconds(1142292),
    new_array: CostDuration::from_picoseconds(1644092),
    new_cell: CostDuration::from_picoseconds(1053503),
    new_map: CostDuration::from_picoseconds(1718775),
    concatc_constant: CostDuration::from_picoseconds(1384371),
    concatc_coeff_total_size: CostDuration::from_picoseconds(1925),
    push_array: CostDuration::from_picoseconds(1330694),
    push_null: CostDuration::from_picoseconds(186530),
    push_cell: CostDuration::from_picoseconds(289193),
    push_bmt: CostDuration::from_picoseconds(273144),
    push_map: CostDuration::from_picoseconds(1611790),
    proof_verify_constant: CostDuration::from_picoseconds(2893777103),
    proof_verify_coeff_size: CostDuration::from_picoseconds(5068214),
    concat_constant: CostDuration::from_picoseconds(969207),
    concat_coeff_total_size: CostDuration::from_picoseconds(1699),
    gc_rcmap_constant: CostDuration::from_picoseconds(189950858),
    gc_rcmap_coeff_keys_removed_size: CostDuration::from_picoseconds(144672913),
    noop_constant: CostDuration::from_picoseconds(75842),
    noop_coeff_arg: CostDuration::from_picoseconds(0),
    add: CostDuration::from_picoseconds(1505993),
    pushs_cell: CostDuration::from_picoseconds(284304),
    pushs_map: CostDuration::from_picoseconds(1618386),
    pushs_array: CostDuration::from_picoseconds(1312048),
    pushs_bmt: CostDuration::from_picoseconds(271829),
    pushs_null: CostDuration::from_picoseconds(187494),
    ckpt: CostDuration::from_picoseconds(74379),
    ec_mul: CostDuration::from_picoseconds(119319993),
    jmp_constant: CostDuration::from_picoseconds(76142),
    jmp_coeff_arg: CostDuration::from_picoseconds(1),
    idxp_bmt_constant: CostDuration::from_picoseconds(6604369),
    idxp_bmt_coeff_key_size: CostDuration::from_picoseconds(16221),
    idxp_bmt_coeff_container_log_size: CostDuration::from_picoseconds(0),
    idxp_array: CostDuration::from_picoseconds(8497073),
    idxp_map_constant: CostDuration::from_picoseconds(9253607),
    idxp_map_coeff_key_size: CostDuration::from_picoseconds(8384),
    idxp_map_coeff_container_log_size: CostDuration::from_picoseconds(6422),
    or: CostDuration::from_picoseconds(1507480),
    neg: CostDuration::from_picoseconds(1120478),
    root: CostDuration::from_picoseconds(1097938),
    dup_constant: CostDuration::from_picoseconds(2235956),
    dup_coeff_arg: CostDuration::from_picoseconds(924851),
    verifier_key_load: CostDuration::from_picoseconds(1497733226),
    member_constant: CostDuration::from_picoseconds(3547620),
    member_coeff_container_log_size: CostDuration::from_picoseconds(6439),
    member_coeff_key_size: CostDuration::from_picoseconds(2086),
    remc_map_constant: CostDuration::from_picoseconds(5474818),
    remc_map_coeff_key_size: CostDuration::from_picoseconds(4307),
    remc_map_coeff_container_log_size: CostDuration::from_picoseconds(1592083),
    remc_bmt_constant: CostDuration::from_picoseconds(0),
    remc_bmt_coeff_container_log_size: CostDuration::from_picoseconds(74131908),
    remc_bmt_coeff_key_size: CostDuration::from_picoseconds(0),
    lt: CostDuration::from_picoseconds(1559156),
    idxpc_map_constant: CostDuration::from_picoseconds(9068016),
    idxpc_map_coeff_container_log_size: CostDuration::from_picoseconds(11751),
    idxpc_map_coeff_key_size: CostDuration::from_picoseconds(8303),
    idxpc_bmt_constant: CostDuration::from_picoseconds(6112132),
    idxpc_bmt_coeff_key_size: CostDuration::from_picoseconds(4779),
    idxpc_bmt_coeff_container_log_size: CostDuration::from_picoseconds(0),
    idxpc_array: CostDuration::from_picoseconds(8373711),
    transient_hash: CostDuration::from_picoseconds(68557749),
    type_array: CostDuration::from_picoseconds(1176668),
    type_bmt: CostDuration::from_picoseconds(1053767),
    type_cell: CostDuration::from_picoseconds(1079466),
    type_map: CostDuration::from_picoseconds(1172548),
    type_null: CostDuration::from_picoseconds(941330),
    signature_verify_constant: CostDuration::from_picoseconds(72992416),
    signature_verify_coeff_size: CostDuration::from_picoseconds(631),
    rem_bmt_constant: CostDuration::from_picoseconds(0),
    rem_bmt_coeff_key_size: CostDuration::from_picoseconds(0),
    rem_bmt_coeff_container_log_size: CostDuration::from_picoseconds(76003850),
    rem_map_constant: CostDuration::from_picoseconds(5709797),
    rem_map_coeff_container_log_size: CostDuration::from_picoseconds(1548852),
    rem_map_coeff_key_size: CostDuration::from_picoseconds(4412),
    sub: CostDuration::from_picoseconds(1502481),
    branch_constant: CostDuration::from_picoseconds(472461),
    branch_coeff_arg: CostDuration::from_picoseconds(2),
    subi: CostDuration::from_picoseconds(1189900),
    pop: CostDuration::from_picoseconds(991317),
    popeqc_constant: CostDuration::from_picoseconds(495298),
    popeqc_coeff_value_size: CostDuration::from_picoseconds(285),
    update_rcmap_constant: CostDuration::from_picoseconds(431624324),
    update_rcmap_coeff_keys_added_size: CostDuration::from_picoseconds(92595795),
    swap_constant: CostDuration::from_picoseconds(1552837),
    swap_coeff_arg: CostDuration::from_picoseconds(883050),
    size_bmt: CostDuration::from_picoseconds(999373),
    size_array: CostDuration::from_picoseconds(2202483),
    size_map: CostDuration::from_picoseconds(2547159),
    ec_add: CostDuration::from_picoseconds(350872),
    pedersen_valid: CostDuration::from_picoseconds(262516047),
    get_writes_constant: CostDuration::from_picoseconds(165778),
    get_writes_coeff_keys_added_size: CostDuration::from_picoseconds(6794119),
    hash_to_curve: CostDuration::from_picoseconds(315977087),
    addi: CostDuration::from_picoseconds(1195496),
    and: CostDuration::from_picoseconds(1510914),
    log_null_constant: CostDuration::from_picoseconds(1308234),
    log_null_coeff_value_size: CostDuration::from_picoseconds(0),
    log_cell_constant: CostDuration::from_picoseconds(1430912),
    log_cell_coeff_value_size: CostDuration::from_picoseconds(368),
    log_bmt_constant: CostDuration::from_picoseconds(0),
    log_bmt_coeff_value_size: CostDuration::from_picoseconds(28051),
    log_map_constant: CostDuration::from_picoseconds(0),
    log_map_coeff_value_size: CostDuration::from_picoseconds(17464),
    log_array_constant: CostDuration::from_picoseconds(2762164),
    log_array_coeff_value_size: CostDuration::from_picoseconds(279532),
    popeq_constant: CostDuration::from_picoseconds(478785),
    popeq_coeff_value_size: CostDuration::from_picoseconds(290),
    eq: CostDuration::from_picoseconds(1529569),
};
const UPDATED_PARAMS: LedgerParameters = LedgerParameters {
    cost_model: TransactionCostModel {
        runtime_cost_model: UPDATED_COST_MODEL,
        ..INITIAL_TRANSACTION_COST_MODEL
    },
    limits: TransactionLimits {
        block_limits: SyntheticCost {
            read_time: CostDuration::from_picoseconds(2_000_000_000_000),
            compute_time: CostDuration::from_picoseconds(2_000_000_000_000),
            block_usage: 1_000_000,
            bytes_written: 50_000,
            bytes_churned: 50_000_000,
        },
        ..INITIAL_LIMITS
    },
    global_ttl: Duration::from_hours(336),
    ..INITIAL_PARAMETERS
};

fn main() {
    let Some(fname) = std::env::args_os().nth(1) else {
        eprintln!("provide parameter name to output");
        return;
    };
    let mut path = PathBuf::new();
    path.push("params");
    path.push(fname);
    let f_binary = std::fs::File::create(path.with_added_extension("bin")).unwrap();
    let mut f_json = std::fs::File::create(path.with_added_extension("json")).unwrap();
    tagged_serialize(&UPDATED_PARAMS, f_binary).unwrap();
    serde_json::to_writer_pretty(&mut f_json, &UPDATED_PARAMS).unwrap();
}
