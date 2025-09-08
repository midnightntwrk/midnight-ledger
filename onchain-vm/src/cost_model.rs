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

use base_crypto::{
    cost_model::{CostDuration, RunningCost},
    hash::PERSISTENT_HASH_BYTES,
};
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use serde::Serialize;
#[cfg(feature = "proptest")]
use serialize::randomised_serialization_test;
use serialize::{self, Deserializable, Serializable, Tagged, tag_enforcement_test};
use storage::{
    Storable,
    arena::{ArenaKey, Sp},
    db::DB,
    storable::Loader,
};
use transient_crypto::curve::FR_BYTES;

/// A cost model for gas pricing of VM operations.
///
/// The field names here could be improved, but here's the idea. The cost model
/// defines the parameters of affine-linear op pricing functions with 0
/// (constant), 1, or 2 linear arguments. For some operations, the cost also
/// depends on the type of one of the arguments, these are captured as `<op>_<type>`.
///
/// For ops with constant pricing, the constant cost is given by the `<op>`
/// field. These ops are priced independent of any arguments (stack arguments or arguments built into
/// the op). For example, the `size` op is priced constant, and the cost is
/// given by the `size` field.
///
/// For ops with non-constant pricing, the constant part is given by the
/// `<op>_constant` field, and the scalars for the linear parts are given by the
/// `<op>_linear` and/or `<op>_logarithmic` fields. The meaning of these linear
/// scalars depends on the op in question, and this is where the names could be
/// improved. For example, the `rem` opcode has fields `rem_constant`,
/// `rem_linear`, and `rem_logarithmic`, and the cost of executing the `rem` op
/// to remove key `k` from container `c` is
///
/// ```text
/// rem_constant + rem_linear * serialized_size(k) + rem_logarithmic * log_2_size(c).
/// ```
///
/// Note in particular that this `rem` cost function does *not* take the type of
/// container into account, although that could change in the future.
///
/// These pricing functions are defined by the VM implementation itself, in
/// `midnight-onchain-vm::vm::run_program_internal`. The concrete cost model values used in
/// practice are expected to be learned by linear regression against VM
/// micro-benchmark data collected by
/// `:/onchain-runtime/benches/benchmarking.rs`.
#[derive(Clone, PartialEq, Eq, Debug, Serializable, Serialize, Storable)]
#[storable(base)]
#[tag = "impact-cost-model[v4]"]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub struct CostModel {
    pub noop_constant: CostDuration,
    pub noop_coeff_arg: CostDuration,
    pub lt: CostDuration,
    pub eq: CostDuration,
    pub type_null: CostDuration,
    pub type_cell: CostDuration,
    pub type_map: CostDuration,
    pub type_bmt: CostDuration,
    pub type_array: CostDuration,
    pub size_map: CostDuration,
    pub size_bmt: CostDuration,
    pub size_array: CostDuration,
    pub new_null: CostDuration,
    pub new_cell: CostDuration,
    pub new_map: CostDuration,
    pub new_bmt: CostDuration,
    pub new_array: CostDuration,
    pub and: CostDuration,
    pub or: CostDuration,
    pub neg: CostDuration,
    pub log_null_constant: CostDuration,
    pub log_null_coeff_value_size: CostDuration,
    pub log_cell_constant: CostDuration,
    pub log_cell_coeff_value_size: CostDuration,
    pub log_map_constant: CostDuration,
    pub log_map_coeff_value_size: CostDuration,
    pub log_bmt_constant: CostDuration,
    pub log_bmt_coeff_value_size: CostDuration,
    pub log_array_constant: CostDuration,
    pub log_array_coeff_value_size: CostDuration,
    pub root: CostDuration,
    pub pop: CostDuration,
    pub popeq_constant: CostDuration,
    pub popeq_coeff_value_size: CostDuration,
    pub popeqc_constant: CostDuration,
    pub popeqc_coeff_value_size: CostDuration,
    pub addi: CostDuration,
    pub subi: CostDuration,
    pub push_null: CostDuration,
    pub push_cell: CostDuration,
    pub push_map: CostDuration,
    pub push_bmt: CostDuration,
    pub push_array: CostDuration,
    pub pushs_null: CostDuration,
    pub pushs_cell: CostDuration,
    pub pushs_map: CostDuration,
    pub pushs_bmt: CostDuration,
    pub pushs_array: CostDuration,
    pub branch_constant: CostDuration,
    pub branch_coeff_arg: CostDuration,
    pub jmp_constant: CostDuration,
    pub jmp_coeff_arg: CostDuration,
    pub add: CostDuration,
    pub sub: CostDuration,
    pub concat_constant: CostDuration,
    pub concat_coeff_total_size: CostDuration,
    pub concatc_constant: CostDuration,
    pub concatc_coeff_total_size: CostDuration,
    pub member_constant: CostDuration,
    pub member_coeff_key_size: CostDuration,
    pub member_coeff_container_log_size: CostDuration,
    pub rem_map_constant: CostDuration,
    pub rem_map_coeff_key_size: CostDuration,
    pub rem_map_coeff_container_log_size: CostDuration,
    pub rem_bmt_constant: CostDuration,
    pub rem_bmt_coeff_key_size: CostDuration,
    pub rem_bmt_coeff_container_log_size: CostDuration,
    pub remc_map_constant: CostDuration,
    pub remc_map_coeff_key_size: CostDuration,
    pub remc_map_coeff_container_log_size: CostDuration,
    pub remc_bmt_constant: CostDuration,
    pub remc_bmt_coeff_key_size: CostDuration,
    pub remc_bmt_coeff_container_log_size: CostDuration,
    pub dup_constant: CostDuration,
    pub dup_coeff_arg: CostDuration,
    pub swap_constant: CostDuration,
    pub swap_coeff_arg: CostDuration,
    pub idx_map_constant: CostDuration,
    pub idx_map_coeff_key_size: CostDuration,
    pub idx_map_coeff_container_log_size: CostDuration,
    pub idx_bmt_constant: CostDuration,
    pub idx_bmt_coeff_key_size: CostDuration,
    pub idx_bmt_coeff_container_log_size: CostDuration,
    pub idx_array: CostDuration,
    pub idxp_map_constant: CostDuration,
    pub idxp_map_coeff_key_size: CostDuration,
    pub idxp_map_coeff_container_log_size: CostDuration,
    pub idxp_bmt_constant: CostDuration,
    pub idxp_bmt_coeff_key_size: CostDuration,
    pub idxp_bmt_coeff_container_log_size: CostDuration,
    pub idxp_array: CostDuration,
    pub idxc_map_constant: CostDuration,
    pub idxc_map_coeff_key_size: CostDuration,
    pub idxc_map_coeff_container_log_size: CostDuration,
    pub idxc_bmt_constant: CostDuration,
    pub idxc_bmt_coeff_key_size: CostDuration,
    pub idxc_bmt_coeff_container_log_size: CostDuration,
    pub idxc_array: CostDuration,
    pub idxpc_map_constant: CostDuration,
    pub idxpc_map_coeff_key_size: CostDuration,
    pub idxpc_map_coeff_container_log_size: CostDuration,
    pub idxpc_bmt_constant: CostDuration,
    pub idxpc_bmt_coeff_key_size: CostDuration,
    pub idxpc_bmt_coeff_container_log_size: CostDuration,
    pub idxpc_array: CostDuration,
    pub ins_map_constant: CostDuration,
    pub ins_map_coeff_key_size: CostDuration,
    pub ins_map_coeff_container_log_size: CostDuration,
    pub ins_bmt_constant: CostDuration,
    pub ins_bmt_coeff_key_size: CostDuration,
    pub ins_bmt_coeff_container_log_size: CostDuration,
    pub ins_array: CostDuration,
    pub insc_map_constant: CostDuration,
    pub insc_map_coeff_key_size: CostDuration,
    pub insc_map_coeff_container_log_size: CostDuration,
    pub insc_bmt_constant: CostDuration,
    pub insc_bmt_coeff_key_size: CostDuration,
    pub insc_bmt_coeff_container_log_size: CostDuration,
    pub insc_array: CostDuration,
    pub ckpt: CostDuration,
    pub signature_verify_constant: CostDuration,
    pub signature_verify_coeff_size: CostDuration,
    pub pedersen_valid: CostDuration,
    pub verifier_key_load: CostDuration,
    pub proof_verify_constant: CostDuration,
    pub proof_verify_coeff_size: CostDuration,
    pub hash_to_curve: CostDuration,
    pub ec_add: CostDuration,
    pub ec_mul: CostDuration,
    pub transient_hash: CostDuration,
    pub get_writes_constant: CostDuration,
    pub get_writes_coeff_keys_added_size: CostDuration,
    pub update_rcmap_constant: CostDuration,
    pub update_rcmap_coeff_keys_added_size: CostDuration,
    pub gc_rcmap_constant: CostDuration,
    pub gc_rcmap_coeff_keys_removed_size: CostDuration,
    pub read_time_batched_4k: CostDuration,
    pub read_time_synchronous_4k: CostDuration,
}
tag_enforcement_test!(CostModel);

#[cfg(feature = "proptest")]
randomised_serialization_test!(CostModel);

// NOTE: I'm using benchmarks for *sequential* reads for the batched 4k, and
// *random* reads for the synchronous 4k. This does align with actual
// performance, because sequential read benchmarks are typically parallel, and "random"
// ones are synchronous. The performance is *not* about the randomness on
// SSDs (as it would be on HDDs).
// Populated using mid-range SSD benchmark results, specifically:
// https://ssd.userbenchmark.com/SpeedTest/182182/Samsung-SSD-960-PRO-512GB
// Note that these are MB/s, to get to time per 4k read, it's:
// x MB/s => x MB/s / 4 kB/block = x/4 k block/s => 0.004/x s / block
const BATCHED_4K_READ_TIME: CostDuration = CostDuration::from_picoseconds(2_000_000);
const SYNCHRONOUS_4K_READ_TIME: CostDuration = CostDuration::from_picoseconds(85_000_000);

include!("../gen/const_declaration.rs");

impl CostModel {
    pub fn read_map(&self, log_size: usize, sync: bool) -> RunningCost {
        let layers = log_size.div_ceil(4);
        self.read_cell(PERSISTENT_HASH_BYTES as u64 * 16, sync) * layers as u64
    }
    pub fn read_cell(&self, size: u64, sync: bool) -> RunningCost {
        let blocks = u64::max(1, size.div_ceil(4096));
        if sync {
            let time = self.read_time_synchronous_4k + self.read_time_batched_4k * (blocks - 1);
            RunningCost {
                compute_time: time,
                read_time: time,
                bytes_deleted: 0,
                bytes_written: 0,
            }
        } else {
            RunningCost {
                compute_time: CostDuration::ZERO,
                read_time: self.read_time_batched_4k * blocks,
                bytes_deleted: 0,
                bytes_written: 0,
            }
        }
    }
    pub fn read_array(&self, sync: bool) -> RunningCost {
        self.read_cell(PERSISTENT_HASH_BYTES as u64 * 16, sync)
    }
    pub fn read_bmt(&self, log_size: usize, sync: bool) -> RunningCost {
        self.read_cell((FR_BYTES * 3 + 2) as u64, sync) * log_size as u64
    }
    pub fn tree_copy<T: Storable<D>, D: DB>(&self, value: Sp<T, D>) -> RunningCost {
        let topo_nodes = value.serialize_to_node_list();
        let bytes_written = topo_nodes
            .nodes
            .iter()
            .map(|n| {
                n.data.len() as u64 + n.child_indices.len() as u64 * PERSISTENT_HASH_BYTES as u64
            })
            .sum::<u64>();
        RunningCost {
            read_time: CostDuration::ZERO,
            compute_time: CostDuration::ZERO,
            bytes_written,
            bytes_deleted: 0,
        }
    }
}
