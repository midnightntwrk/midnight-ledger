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

use crate::cost_model::CostModel;
use crate::error::OnchainProgramError;
use crate::ops::*;
use crate::result_mode::ResultMode;
use crate::state_value_ext::*;
use crate::vm_value::*;
use base_crypto::cost_model::{CostDuration, RunningCost};
use base_crypto::fab::{AlignedValue, Alignment, InvalidBuiltinDecode, Value};
use base_crypto::hash::PERSISTENT_HASH_BYTES;
use rpds::{HashTrieMap, Vector};
use runtime_state::state::*;
use serialize::Serializable;
use std::fmt::{self, Debug, Formatter};
use storage::arena::Sp;
use storage::db::DB;
use storage::storage::HashMap;
use transient_crypto::merkle_tree::MerkleTree;

use ValueStrength::*;

// Maps initial stack positions to which parts of those stack objects are currently cached.
struct Cache(Vec<InnerCache>);

/// The size limit of the argument of the `log` instruction. Currently 512 kb
pub const MAX_LOG_SIZE: u64 = 1 << 19;

impl Cache {
    fn visit(&mut self, key: &CacheKey) -> bool {
        match &key.0 {
            None => true,
            Some((n, path)) => {
                if *n > self.0.len() {
                    false
                } else {
                    path.iter()
                        .fold((&mut self.0[*n], true), |(cache, found), key| match cache {
                            InnerCache(None) => (cache, found),
                            InnerCache(Some(map)) => {
                                if map.contains_key(key) {
                                    (map.get_mut(key).unwrap(), found)
                                } else {
                                    map.insert_mut(
                                        key.clone(),
                                        InnerCache(Some(HashTrieMap::new())),
                                    );
                                    (map.get_mut(key).unwrap(), false)
                                }
                            }
                        })
                        .1
                }
            }
        }
    }
}

// For a given data structure, either:
//  - If None, indicates the entire data structure is cached.
//  - If Some, maps keys in the data structure to the parts of those objects
//    that are currently cached.
// Note: If a value is copied *into another data structure*, it is no longer
// linked to its source in the cache structure. This is _not_ the case if it is
// copied through the stack.
#[derive(Clone)]
struct InnerCache(Option<HashTrieMap<AlignedValue, InnerCache>>);

#[derive(Clone)]
struct CacheKey(Option<(usize, Vector<AlignedValue>)>);

impl CacheKey {
    fn push_key(&self, key: &AlignedValue) -> Self {
        CacheKey(
            self.0
                .as_ref()
                .map(|(idx, path)| (*idx, path.push_back(key.clone()))),
        )
    }
}

impl Debug for CacheKey {
    fn fmt(&self, fmt: &mut Formatter) -> fmt::Result {
        match &self.0 {
            None => write!(fmt, "-"),
            Some((idx, path)) => {
                write!(fmt, "{}", idx)?;
                for key in path.iter() {
                    write!(fmt, "/{:?}", key)?;
                }
                Ok(())
            }
        }
    }
}

pub const MAX_STACK_HEIGHT: u32 = 1 << 16;

fn add<E, B: TryInto<u64, Error = E>, D: DB>(
    a: &Value,
    b: B,
) -> Result<AlignedValue, OnchainProgramError<D>>
where
    OnchainProgramError<D>: From<E>,
{
    let a_tmp: Result<u64, InvalidBuiltinDecode> = (&**a.as_ref()).try_into();
    let a = a_tmp?;
    let b: u64 = b.try_into()?;
    a.checked_add(b)
        .ok_or(OnchainProgramError::ArithmeticOverflow)
        .map(Into::into)
}

fn sub<E, B: TryInto<u64, Error = E>, D: DB>(
    a: &Value,
    b: B,
) -> Result<AlignedValue, OnchainProgramError<D>>
where
    OnchainProgramError<D>: From<E>,
{
    let a: Result<u64, InvalidBuiltinDecode> = (&**a.as_ref()).try_into();
    let b: u64 = b.try_into()?;
    a?.checked_sub(b)
        .ok_or(OnchainProgramError::ArithmeticOverflow)
        .map(Into::into)
}

fn lt<D: DB>(a: &Value, b: &Value) -> Result<AlignedValue, OnchainProgramError<D>> {
    let a: u64 = (&**a.as_ref()).try_into()?;
    let b: u64 = (&**b.as_ref()).try_into()?;
    Ok((a < b).into())
}

fn concat<D: DB>(
    a: &AlignedValue,
    b: &AlignedValue,
    bound: u32,
) -> Result<AlignedValue, OnchainProgramError<D>> {
    let size_a = Serializable::serialized_size(a);
    let size_b = Serializable::serialized_size(b);
    if size_a + size_b <= bound as usize {
        AlignedValue::new(
            Value::concat([AsRef::<Value>::as_ref(a), AsRef::<Value>::as_ref(b)]),
            Alignment::concat([AsRef::<Alignment>::as_ref(a), AsRef::<Alignment>::as_ref(b)]),
        )
        .ok_or(OnchainProgramError::TypeError(
            "alignment doesn't fit in concat".into(),
        ))
    } else {
        Err(OnchainProgramError::BoundsExceeded)
    }
}

fn eq_valid_input(x: &AlignedValue) -> bool {
    Serializable::serialized_size(x) <= 64
}

fn eq<D: DB>(a: &AlignedValue, b: &AlignedValue) -> Result<AlignedValue, OnchainProgramError<D>> {
    if !eq_valid_input(a) || !eq_valid_input(b) {
        Err(OnchainProgramError::TooLongForEqual)
    } else {
        Ok((a == b).into())
    }
}

fn idx<D: DB>(
    value: &(VmValue<D>, CacheKey),
    key: &AlignedValue,
    cache: &mut Cache,
    cached: bool,
) -> Result<(VmValue<D>, CacheKey), OnchainProgramError<D>> {
    let cache_key = value.1.push_key(key);
    let cache_miss = !cache.visit(&cache_key);
    match &value.0.value {
        StateValue::Array(arr) => {
            let idx: u8 = (&**AsRef::<Value>::as_ref(&key)).try_into()?;
            if idx as usize >= arr.len() {
                Err(OnchainProgramError::TypeError(format!(
                    "index out of bounds in idx: {} >= {}",
                    idx,
                    arr.len()
                )))
            } else {
                let res = arr.get(idx as usize).unwrap();
                if cached && cache_miss {
                    Err(OnchainProgramError::CacheMiss)
                } else {
                    Ok((VmValue::new(value.0.strength, res.clone()), cache_key))
                }
            }
        }
        StateValue::Map(map) => {
            let res = map
                .get(key)
                .map(|sp| (*sp).clone())
                .unwrap_or(StateValue::Null);
            if cached && cache_miss {
                Err(OnchainProgramError::CacheMiss)
            } else {
                Ok((VmValue::new(value.0.strength, res), cache_key))
            }
        }
        StateValue::BoundedMerkleTree(tree) => {
            let key = (&**AsRef::<Value>::as_ref(&key)).try_into()?;
            if key >= (1u64 << tree.height() as u64) {
                Err(OnchainProgramError::MissingKey)
            } else {
                Ok((
                    match tree.index(key) {
                        Some((hash, ())) => {
                            VmValue::new(value.0.strength, StateValue::Cell(Sp::new(hash.into())))
                        }
                        None => VmValue::new(value.0.strength, StateValue::Null),
                    },
                    cache_key,
                ))
            }
        }
        _ => Err(OnchainProgramError::TypeError(format!(
            "tried to idx, only map, array, and bmt are supported"
        ))),
    }
}

#[derive(Debug)]
pub struct VmResults<M: ResultMode<D>, D: DB> {
    pub stack: Vec<VmValue<D>>,
    pub events: Vec<M::Event>,
    pub gas_cost: RunningCost,
}

/// Run a VM program.
///
/// The starting stack `initial` is consumed from right to left, i.e. highest to
/// lowest index position; popping the stack returns the last element.
///
/// The op-code sequence `program` is evaluated from left to right, i.e. from
/// lowest to highest index position.
pub fn run_program<M: ResultMode<D>, D: DB>(
    initial: &[VmValue<D>],
    program: &[Op<M, D>],
    gas_limit: Option<RunningCost>,
    cost_model: &CostModel,
) -> Result<VmResults<M, D>, OnchainProgramError<D>> {
    let step_limit = None;
    run_program_step_limited(initial, program, step_limit, gas_limit, cost_model)
}

/// Run a VM program with an optional step limit.
///
/// If `step_limit = Some(limit)` is provided, the VM will execute at most that
/// many instructions.
///
/// See `run_program` for explanation of other arguments.
pub fn run_program_step_limited<M: ResultMode<D>, D: DB>(
    initial: &[VmValue<D>],
    program: &[Op<M, D>],
    step_limit: Option<usize>,
    gas_limit: Option<RunningCost>,
    cost_model: &CostModel,
) -> Result<VmResults<M, D>, OnchainProgramError<D>> {
    let initial_annot = initial
        .iter()
        .enumerate()
        .map(|(i, val)| (val.clone(), CacheKey(Some((i, Vector::new())))))
        .collect();
    let mut cache = Cache(
        initial
            .iter()
            .map(|val| {
                if val.strength == ValueStrength::Weak {
                    InnerCache(None)
                } else {
                    InnerCache(Some(HashTrieMap::new()))
                }
            })
            .collect(),
    );
    run_program_internal(
        initial_annot,
        Vec::new(),
        &mut cache,
        program,
        step_limit,
        gas_limit,
        cost_model,
    )
}

/// Implementation of VM, including gas fees and opcode semantics.
fn run_program_internal<M: ResultMode<D>, D: DB>(
    mut stack: Vec<(VmValue<D>, CacheKey)>,
    mut events: Vec<M::Event>,
    cache: &mut Cache,
    mut program: &[Op<M, D>],
    step_limit: Option<usize>,
    gas_limit: Option<RunningCost>,
    cost_model: &CostModel,
) -> Result<VmResults<M, D>, OnchainProgramError<D>> {
    use Op::*;
    let vnew = VmValue::new;
    let mut gas = RunningCost::default();
    let incr_gas_full = |mut gas: RunningCost, by: RunningCost| {
        gas = gas + by;
        if let Some(limit) = gas_limit {
            if gas.compute_time > limit.compute_time || gas.read_time > limit.read_time {
                return Err(OnchainProgramError::OutOfGas);
            }
        }
        Ok(gas)
    };
    let incr_gas = |gas: RunningCost, by: CostDuration| {
        incr_gas_full(
            gas,
            RunningCost {
                compute_time: by,
                read_time: CostDuration::ZERO,
                bytes_deleted: 0,
                bytes_written: 0,
            },
        )
    };
    let mut steps_executed: usize = 0;

    while !program.is_empty() {
        if let Some(limit) = step_limit {
            if steps_executed >= limit {
                break;
            }
        }
        steps_executed += 1;

        let op = &program[0];
        // dbg!(&op, &stack);
        let stack_req = match op {
            Op::Noop { .. } | Op::Push { .. } | Op::Jmp { .. } | Op::Ckpt => 0,
            Op::Type
            | Op::Size
            | Op::New
            | Op::Neg
            | Op::Log
            | Op::Root
            | Op::Pop
            | Op::Popeq { .. }
            | Op::Addi { .. }
            | Op::Subi { .. }
            | Op::Branch { .. } => 1,
            Op::Lt
            | Op::Eq
            | Op::And
            | Op::Or
            | Op::Add
            | Op::Sub
            | Op::Concat { .. }
            | Op::Member
            | Op::Rem { .. } => 2,
            Op::Dup { n } => *n as usize + 1,
            Op::Swap { n } => *n as usize + 2,
            Op::Idx { path, .. } => path.iter().filter(|key| &**key == &Key::Stack).count() + 1,
            Op::Ins { n, .. } => *n as usize * 2 + 1,
        };
        let stack_len = stack.len();
        if stack_len < stack_req {
            return Err(OnchainProgramError::RanOffStack);
        }
        // Implementation of opcode semantics, including gas cost accounting.
        match op {
            Noop { n } => {
                gas = incr_gas(
                    gas,
                    cost_model.noop_constant + cost_model.noop_coeff_arg * (*n as u64),
                )?;
            }
            // The actual branching is handled at the end.
            Branch { skip } => {
                gas = incr_gas(
                    gas,
                    cost_model.branch_constant + cost_model.branch_coeff_arg * *skip as u64,
                )?;
            }
            // The actual jumping is handled at the end.
            Jmp { skip } => {
                gas = incr_gas(
                    gas,
                    cost_model.jmp_constant + cost_model.jmp_coeff_arg * *skip as u64,
                )?;
            }
            Ckpt => {
                gas = incr_gas(gas, cost_model.ckpt)?;
            }
            Lt => {
                gas = incr_gas(gas, cost_model.lt)?;
                let b = stack.pop().unwrap().0.as_cell()?;
                let a = stack.pop().unwrap().0.as_cell()?;
                stack.push((vnew(Strong, lt(&a.value, &b.value)?.into()), CacheKey(None)));
            }
            Eq => {
                gas = incr_gas(gas, cost_model.eq)?;
                let a = stack.pop().unwrap().0.as_cell()?;
                let b = stack.pop().unwrap().0.as_cell()?;
                stack.push((vnew(Strong, eq(&*a, &*b)?.into()), CacheKey(None)));
            }
            Type => {
                let val = stack.pop().unwrap().0.value;
                let (ty, cost) = match &val {
                    StateValue::Cell(_) => (0, cost_model.type_cell),
                    StateValue::Null => (1, cost_model.type_null),
                    StateValue::Map(_) => (2, cost_model.type_map),
                    StateValue::Array(a) => (3 + a.len() as u8 * 8, cost_model.type_array),
                    StateValue::BoundedMerkleTree(t) => (4 + t.height() * 8, cost_model.type_bmt),
                    x => panic!("unhandled StateValue variant {x:?}"),
                };
                gas = incr_gas(gas, cost)?;
                stack.push((vnew(Strong, AlignedValue::from(ty).into()), CacheKey(None)));
            }
            Size => {
                let val = stack.pop().unwrap().0.value;
                let (new_gas, size) = match &val {
                    StateValue::Map(m) => (incr_gas(gas, cost_model.size_map)?, m.size() as u64),
                    StateValue::BoundedMerkleTree(t) => {
                        (incr_gas(gas, cost_model.size_bmt)?, t.height() as u64)
                    }
                    StateValue::Array(a) => (incr_gas(gas, cost_model.size_array)?, a.len() as u64),
                    _ => {
                        return Err(OnchainProgramError::TypeError(format!(
                            "attempted to take size, only map, array, and bmt are supported"
                        )));
                    }
                };
                gas = new_gas;
                stack.push((
                    vnew(Strong, AlignedValue::from(size).into()),
                    CacheKey(None),
                ));
            }
            New => {
                let a = stack.pop().unwrap();
                let val: u8 = (&**AsRef::<Value>::as_ref(&*a.0.value.as_cell()?)).try_into()?;
                // Type tag in lower 3 bits.
                let (new_gas, new_val) = match val & 0b111 {
                    0u8 => (
                        incr_gas(gas, cost_model.new_cell)?,
                        StateValue::Cell(Sp::new(().into())),
                    ),
                    1u8 => (incr_gas(gas, cost_model.new_null)?, StateValue::Null),
                    2u8 => (
                        incr_gas(gas, cost_model.new_map)?,
                        StateValue::Map(HashMap::new()),
                    ),
                    3u8 => {
                        let gas = incr_gas(gas, cost_model.new_array)?;
                        // Container size in upper 5 bits.
                        let size = val >> 3;
                        if size > 16 {
                            return Err(OnchainProgramError::InvalidArgs(format!(
                                "new: array length > 16: {size}"
                            )));
                        }
                        (
                            gas,
                            StateValue::Array(vec![StateValue::Null; size as usize].into()),
                        )
                    }
                    4u8 => (
                        incr_gas(gas, cost_model.new_bmt)?,
                        StateValue::BoundedMerkleTree(MerkleTree::blank(val >> 3)),
                    ),
                    tag => {
                        return Err(OnchainProgramError::InvalidArgs(format!(
                            "new: type tag > 4: {tag}",
                        )));
                    }
                };
                gas = new_gas;
                stack.push((vnew(Strong, new_val), CacheKey(None)));
            }
            And => {
                gas = incr_gas(gas, cost_model.and)?;
                let a = bool::try_from(&*stack.pop().unwrap().0.as_cell()?.value)?;
                let b = bool::try_from(&*stack.pop().unwrap().0.as_cell()?.value)?;
                stack.push((
                    vnew(Strong, AlignedValue::from(a && b).into()),
                    CacheKey(None),
                ));
            }
            Or => {
                gas = incr_gas(gas, cost_model.or)?;
                let a = bool::try_from(&*stack.pop().unwrap().0.as_cell()?.value)?;
                let b = bool::try_from(&*stack.pop().unwrap().0.as_cell()?.value)?;
                stack.push((
                    vnew(Strong, AlignedValue::from(a || b).into()),
                    CacheKey(None),
                ));
            }
            Neg => {
                gas = incr_gas(gas, cost_model.neg)?;
                let a = bool::try_from(&*stack.pop().unwrap().0.as_cell()?.value)?;
                stack.push((vnew(Strong, AlignedValue::from(!a).into()), CacheKey(None)));
            }
            Log => {
                let val = stack.pop().unwrap().0.value;
                let nodes = Sp::new(val.clone())
                    .serialize_to_node_list_bounded(MAX_LOG_SIZE)
                    .ok_or(OnchainProgramError::LogBoundExceeded)?;
                let size = nodes
                    .nodes
                    .iter()
                    .map(|n| PERSISTENT_HASH_BYTES as u64 + n.data.len() as u64)
                    .sum();
                gas = incr_gas(
                    gas,
                    match &val {
                        StateValue::Null => {
                            cost_model.log_null_constant
                                + cost_model.log_array_coeff_value_size * size
                        }
                        StateValue::Cell(_) => {
                            cost_model.log_cell_constant
                                + cost_model.log_cell_coeff_value_size * size
                        }
                        StateValue::Map(_) => {
                            cost_model.log_map_constant + cost_model.log_map_coeff_value_size * size
                        }
                        StateValue::BoundedMerkleTree(_) => {
                            cost_model.log_bmt_constant + cost_model.log_bmt_coeff_value_size * size
                        }
                        StateValue::Array(_) => {
                            cost_model.log_array_constant
                                + cost_model.log_array_coeff_value_size * size
                        }
                        _ => CostDuration::ZERO,
                    },
                )?;
                // Increment both writes and deletes to count the entire log as churn.
                gas = incr_gas_full(
                    gas,
                    RunningCost {
                        read_time: CostDuration::ZERO,
                        compute_time: CostDuration::ZERO,
                        bytes_written: size,
                        bytes_deleted: size,
                    },
                )?;
                if let Some(event) = M::process_log(&val) {
                    events.push(event);
                }
            }
            Root => {
                gas = incr_gas(gas, cost_model.root)?;
                let a = stack.pop().unwrap().0.value;
                stack.push((
                    vnew(
                        Strong,
                        AlignedValue::from(match &a {
                            StateValue::BoundedMerkleTree(tree) => tree.root().ok_or_else(|| OnchainProgramError::TypeError(format!(
                                "attempted to take root of non-rehashed bmt (this should not be possible!)"
                            )))?,
                            _ => {
                                return Err(OnchainProgramError::TypeError(format!(
                                    "attempted to take root of non bmt"
                                )));
                            }
                        })
                        .into(),
                    ),
                    CacheKey(None),
                ));
            }
            Pop => {
                gas = incr_gas(gas, cost_model.pop)?;
                drop(stack.pop().unwrap())
            }
            // popeq, popeqc
            // NOTE: cached is deprecated, it has no effect
            Popeq { cached: _, result } => {
                gas = incr_gas(gas, cost_model.popeq_constant)?;
                let value = &stack.pop().unwrap().0.value.as_cell()?;
                gas = incr_gas(
                    gas,
                    cost_model.popeq_coeff_value_size
                        * <AlignedValue as Serializable>::serialized_size(&*value) as u64,
                )?;
                if let Some(event) = M::process_read(result, value)? {
                    events.push(event);
                }
            }
            Addi { immediate } => {
                gas = incr_gas(gas, cost_model.addi)?;
                let a = stack.pop().unwrap().0.as_cell()?;
                stack.push((
                    vnew(Strong, add(&a.value, *immediate)?.into()),
                    CacheKey(None),
                ));
            }
            Subi { immediate } => {
                gas = incr_gas(gas, cost_model.subi)?;
                let a = stack.pop().unwrap().0.as_cell()?;
                stack.push((
                    vnew(Strong, sub(&a.value, *immediate)?.into()),
                    CacheKey(None),
                ));
            }
            // push, pushs
            Push { storage, value } => {
                gas = incr_gas(
                    gas,
                    match (storage, value) {
                        (true, StateValue::Null) => cost_model.pushs_null,
                        (false, StateValue::Null) => cost_model.push_null,
                        (true, StateValue::Cell(..)) => cost_model.pushs_cell,
                        (false, StateValue::Cell(..)) => cost_model.push_cell,
                        (true, StateValue::Map(..)) => cost_model.pushs_map,
                        (false, StateValue::Map(..)) => cost_model.push_map,
                        (true, StateValue::BoundedMerkleTree(..)) => cost_model.pushs_bmt,
                        (false, StateValue::BoundedMerkleTree(..)) => cost_model.push_bmt,
                        (true, StateValue::Array(..)) => cost_model.pushs_array,
                        (false, StateValue::Array(..)) => cost_model.push_array,
                        _ => CostDuration::ZERO,
                    },
                )?;
                stack.push((
                    vnew(if *storage { Strong } else { Weak }, value.clone()),
                    CacheKey(None),
                ))
            }
            Add => {
                gas = incr_gas(gas, cost_model.add)?;
                let a = stack.pop().unwrap().0.as_cell()?;
                let b = stack.pop().unwrap().0.as_cell()?;
                stack.push((
                    vnew(Strong, add(&a.value, &*b.as_slice())?.into()),
                    CacheKey(None),
                ));
            }
            Sub => {
                gas = incr_gas(gas, cost_model.sub)?;
                let a = stack.pop().unwrap().0.as_cell()?;
                let b = stack.pop().unwrap().0.as_cell()?;
                stack.push((
                    vnew(Strong, sub(&b.value, &*a.as_slice())?.into()),
                    CacheKey(None),
                ));
            }
            // concat, concatc
            // NOTE: cached is deprecated, it has no effect.
            Concat { cached: _, n } => {
                if *n > CELL_BOUND as u32 {
                    return Err(OnchainProgramError::CellBoundExceeded);
                }
                let a = stack.pop().unwrap().0.as_cell()?;
                let b = stack.pop().unwrap().0.as_cell()?;
                let total_len = <AlignedValue as Serializable>::serialized_size(&*a)
                    + <AlignedValue as Serializable>::serialized_size(&*b);
                gas = incr_gas(
                    gas,
                    cost_model.concat_constant
                        + cost_model.concat_coeff_total_size * total_len as u64,
                )?;
                stack.push((vnew(Strong, concat(&b, &a, *n)?.into()), CacheKey(None)));
            }
            Member => {
                let key = stack.pop().unwrap().0.as_cell()?;
                let map = stack.pop().unwrap().0.value;
                gas = incr_gas(
                    gas,
                    cost_model.member_constant
                        + cost_model.member_coeff_key_size
                            * <AlignedValue as Serializable>::serialized_size(&key) as u64
                        + cost_model.member_coeff_container_log_size * map.log_size() as u64,
                )?;
                gas = incr_gas_full(gas, cost_model.read_map(map.log_size(), true))?;
                stack.push((
                    vnew(
                        Strong,
                        AlignedValue::from(match &map {
                            StateValue::Map(map) => map.contains_key(&key),
                            _ => {
                                return Err(OnchainProgramError::TypeError(format!(
                                    "attempted to check membership, only map is supported"
                                )));
                            }
                        })
                        .into(),
                    ),
                    CacheKey(None),
                ));
            }
            // rem, remc
            Rem { cached } => {
                let key = stack.pop().unwrap().0.as_cell()?;
                let container = stack.pop().unwrap();
                gas = incr_gas(
                    gas,
                    match &container.0.value {
                        StateValue::Map(_) => {
                            cost_model.rem_map_constant
                                + cost_model.rem_map_coeff_key_size
                                    * <AlignedValue as Serializable>::serialized_size(&key) as u64
                                + cost_model.rem_map_coeff_container_log_size
                                    * container.0.value.log_size() as u64
                        }
                        StateValue::BoundedMerkleTree(_) => {
                            cost_model.rem_bmt_constant
                                + cost_model.rem_bmt_coeff_container_log_size
                                    * container.0.value.log_size() as u64
                        }
                        _ => CostDuration::ZERO,
                    },
                )?;
                if !cached {
                    gas = incr_gas_full(
                        gas,
                        match &container.0.value {
                            StateValue::Map(_) => {
                                cost_model.read_map(container.0.value.log_size(), true)
                            }
                            StateValue::BoundedMerkleTree(_) => {
                                cost_model.read_bmt(container.0.value.log_size(), true)
                            }
                            _ => RunningCost::ZERO,
                        },
                    )?;
                }
                let cache_hit = cache.visit(&container.1.push_key(&key));
                if *cached && !cache_hit {
                    return Err(OnchainProgramError::CacheMiss);
                }
                let container_nxt = match &container.0.value {
                    StateValue::Map(m) => StateValue::Map(m.remove(&key)),
                    StateValue::BoundedMerkleTree(t) => StateValue::BoundedMerkleTree(
                        t.update_hash((&*key.value).try_into()?, Default::default(), ())
                            .rehash(),
                    ),
                    _ => {
                        return Err(OnchainProgramError::TypeError(format!(
                            "attempted to rem, only map and bmt are supported",
                        )));
                    }
                };
                stack.push((vnew(container.0.strength, container_nxt), container.1));
            }
            Dup { n } => {
                gas = incr_gas(
                    gas,
                    cost_model.dup_constant + cost_model.dup_coeff_arg * *n as u64,
                )?;
                stack.push(stack[stack_len - *n as usize - 1].clone());
            }
            Swap { n } => {
                gas = incr_gas(
                    gas,
                    cost_model.swap_constant + cost_model.swap_coeff_arg * *n as u64,
                )?;
                stack.swap(stack_len - 1, stack_len - 2 - *n as usize);
            }
            // idx, idxc, idxp, idxpc
            Idx {
                cached,
                push_path,
                path,
            } => {
                let mut stack_keys = path
                    .iter()
                    .filter(|key| &**key == &Key::Stack)
                    .map(|_| stack.pop().unwrap().0)
                    .collect::<Vec<_>>()
                    .into_iter();
                let mut i = 0;
                let refined_path = path.iter().map(|key| match &*key {
                    Key::Stack => {
                        i += 1;
                        stack_keys.next().unwrap()
                    }
                    Key::Value(v) => vnew(Weak, StateValue::Cell(Sp::new(v.clone()))),
                });
                let mut curr = stack.pop().unwrap();
                for key in refined_path {
                    let (constant_cost, linear_cost, log_cost) =
                        match (&curr.0.value, cached, push_path) {
                            (StateValue::Map(_), true, true) => (
                                cost_model.idxpc_map_constant,
                                cost_model.idxpc_map_coeff_key_size,
                                cost_model.idxpc_map_coeff_container_log_size,
                            ),
                            (StateValue::Map(_), true, false) => (
                                cost_model.idxc_map_constant,
                                cost_model.idxc_map_coeff_key_size,
                                cost_model.idxc_map_coeff_container_log_size,
                            ),
                            (StateValue::BoundedMerkleTree(_), true, true) => (
                                cost_model.idxpc_bmt_constant,
                                cost_model.idxpc_bmt_coeff_key_size,
                                cost_model.idxpc_bmt_coeff_container_log_size,
                            ),
                            (StateValue::BoundedMerkleTree(_), true, false) => (
                                cost_model.idxc_bmt_constant,
                                cost_model.idxc_bmt_coeff_key_size,
                                cost_model.idxc_bmt_coeff_container_log_size,
                            ),
                            (StateValue::Array(_), true, true) => (
                                cost_model.idxpc_array,
                                CostDuration::ZERO,
                                CostDuration::ZERO,
                            ),
                            (StateValue::Array(_), true, false) => (
                                cost_model.idxc_array,
                                CostDuration::ZERO,
                                CostDuration::ZERO,
                            ),
                            (StateValue::Map(_), false, true) => (
                                cost_model.idxp_map_constant,
                                cost_model.idxp_map_coeff_key_size,
                                cost_model.idxp_map_coeff_container_log_size,
                            ),
                            (StateValue::Map(_), false, false) => (
                                cost_model.idx_map_constant,
                                cost_model.idx_map_coeff_key_size,
                                cost_model.idx_map_coeff_container_log_size,
                            ),
                            (StateValue::BoundedMerkleTree(_), false, true) => (
                                cost_model.idxp_bmt_constant,
                                cost_model.idxp_bmt_coeff_key_size,
                                cost_model.idxp_bmt_coeff_container_log_size,
                            ),
                            (StateValue::BoundedMerkleTree(_), false, false) => (
                                cost_model.idx_bmt_constant,
                                cost_model.idx_bmt_coeff_key_size,
                                cost_model.idx_bmt_coeff_container_log_size,
                            ),
                            (StateValue::Array(_), false, true) => (
                                cost_model.idxp_array,
                                CostDuration::ZERO,
                                CostDuration::ZERO,
                            ),
                            (StateValue::Array(_), false, false) => {
                                (cost_model.idx_array, CostDuration::ZERO, CostDuration::ZERO)
                            }
                            _ => (CostDuration::ZERO, CostDuration::ZERO, CostDuration::ZERO),
                        };
                    gas = incr_gas(
                        gas,
                        constant_cost
                            + linear_cost * Serializable::serialized_size(&key.value) as u64
                            + log_cost * curr.0.value.log_size() as u64,
                    )?;
                    if !*cached {
                        gas = incr_gas_full(
                            gas,
                            match &curr.0.value {
                                StateValue::Array(_) => cost_model.read_array(true),
                                StateValue::Map(_) => {
                                    cost_model.read_map(curr.0.value.log_size(), true)
                                }
                                StateValue::BoundedMerkleTree(_) => {
                                    cost_model.read_bmt(curr.0.value.log_size(), true)
                                }
                                _ => RunningCost::ZERO,
                            },
                        )?;
                    }
                    if *push_path {
                        stack.push(curr.clone());
                        stack.push((key.clone(), CacheKey(None)));
                    }
                    let key_val = key.as_cell_ref()?;
                    curr = idx(&curr, key_val, cache, *cached)?;
                    if !*cached {
                        let size = match &curr.0.value {
                            StateValue::Cell(val) => val.serialized_size(),
                            // Stand-in, should only mean 'read one block' anyhow.
                            _ => 1024,
                        };
                        gas = incr_gas_full(gas, cost_model.read_cell(size as u64, true))?;
                    }
                }
                stack.push(curr);
            }
            // ins, insc
            Ins { cached, n } => {
                let mut curr = stack.pop().unwrap();
                for _ in 0..*n {
                    let key = stack.pop().unwrap().0;
                    let container = stack.pop().unwrap();
                    let (constant_cost, linear_cost, log_cost) = match (&container.0.value, cached)
                    {
                        (StateValue::Map(_), false) => (
                            cost_model.ins_map_constant,
                            cost_model.ins_map_coeff_key_size,
                            cost_model.ins_map_coeff_container_log_size,
                        ),
                        (StateValue::BoundedMerkleTree(_), false) => (
                            cost_model.ins_bmt_constant,
                            cost_model.ins_bmt_coeff_key_size,
                            cost_model.ins_bmt_coeff_container_log_size,
                        ),
                        (StateValue::Array(_), false) => {
                            (cost_model.ins_array, CostDuration::ZERO, CostDuration::ZERO)
                        }
                        (StateValue::Map(_), true) => (
                            cost_model.insc_map_constant,
                            cost_model.insc_map_coeff_key_size,
                            cost_model.insc_map_coeff_container_log_size,
                        ),
                        (StateValue::BoundedMerkleTree(_), true) => (
                            cost_model.insc_bmt_constant,
                            cost_model.insc_bmt_coeff_key_size,
                            cost_model.insc_bmt_coeff_container_log_size,
                        ),
                        (StateValue::Array(_), true) => (
                            cost_model.insc_array,
                            CostDuration::ZERO,
                            CostDuration::ZERO,
                        ),
                        _ => (CostDuration::ZERO, CostDuration::ZERO, CostDuration::ZERO),
                    };
                    gas = incr_gas(
                        gas,
                        constant_cost
                            + linear_cost * Serializable::serialized_size(&key.value) as u64
                            + log_cost * container.0.value.log_size() as u64,
                    )?;
                    if !*cached {
                        gas = incr_gas_full(
                            gas,
                            match &container.0.value {
                                StateValue::Map(_) => {
                                    cost_model.read_map(container.0.value.log_size(), true)
                                }
                                StateValue::BoundedMerkleTree(_) => {
                                    cost_model.read_bmt(container.0.value.log_size(), true)
                                }
                                StateValue::Array(_) => cost_model.read_array(true),
                                _ => RunningCost::ZERO,
                            },
                        )?;
                    }
                    let key_cell = key.as_cell_ref()?;
                    let VmValue {
                        strength: container_str,
                        value: container_val,
                    } = container.0;
                    let cache_hit = cache.visit(&container.1.push_key(key_cell));
                    if *cached && !cache_hit {
                        if let StateValue::Array(_) = &container_val {
                            // The miss is okay. We're overwriting an array cell, we never need to read it.
                        } else {
                            return Err(OnchainProgramError::CacheMiss);
                        }
                    }
                    let next = match &container_val {
                        StateValue::Array(arr) => {
                            let idx: u8 = (&**AsRef::<Value>::as_ref(&key_cell)).try_into()?;
                            // The `arr.insert` enforces index checking.
                            let arr = arr.insert(idx as usize, curr.0.value).ok_or_else(|| {
                                OnchainProgramError::TypeError(format!(
                                    "index out of bounds in array ins: {} >= {}",
                                    idx,
                                    arr.len()
                                ))
                            })?;
                            StateValue::Array(arr)
                        }
                        StateValue::Map(m) => {
                            StateValue::Map(m.insert(key_cell.clone(), curr.0.value))
                        }
                        StateValue::BoundedMerkleTree(t) => {
                            let idx = (&**AsRef::<Value>::as_ref(&key_cell)).try_into()?;
                            if idx < (1u64 << t.height()) {
                                let cell_ref = curr.0.as_cell_ref().map_err(|e| {
                                    OnchainProgramError::TypeError(format!(
                                        "attempted to ins a non-cell value into a bmt: {e}"
                                    ))
                                })?;
                                let value_slice = &**AsRef::<Value>::as_ref(&cell_ref);
                                let hash = value_slice.try_into().map_err(|e| {
                                    OnchainProgramError::TypeError(format!(
                                        "attempt to ins cell that doesn't repr hash into bmt: {e}"
                                    ))
                                })?;
                                StateValue::BoundedMerkleTree(t.update_hash(idx, hash, ()).rehash())
                            } else {
                                return Err(OnchainProgramError::BoundsExceeded);
                            }
                        }
                        _ => {
                            return Err(OnchainProgramError::TypeError(format!(
                                "attempted to ins, only array, map, and bmt are supported"
                            )));
                        }
                    };
                    curr.0 = vnew(curr.0.strength & container_str, next);
                    curr.1 = container.1;
                }
                stack.push(curr);
            }
        }
        let skip = 1 + match op {
            Branch { skip } => {
                let a = stack.pop().unwrap().0.as_cell()?;
                if a.value.0.len() == 1 && a.value.0[0].0.is_empty() {
                    0
                } else {
                    *skip as usize
                }
            }
            Jmp { skip } => *skip as usize,
            _ => 0,
        };
        if skip > program.len() {
            return Err(OnchainProgramError::RanPastProgramEnd);
        }
        program = &program[skip..];
    }
    Ok(VmResults {
        stack: stack.into_iter().map(|(a, _)| a).collect(),
        events,
        gas_cost: gas,
    })
}
// See onchain-runtime/tests/vm.rs for tests.
