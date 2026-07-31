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

use crate::result_mode::{ResultMode, ResultModeVerify};
use base_crypto::fab::AlignedValue;
use base_crypto::repr::MemWrite;
use derive_where::derive_where;
#[cfg(feature = "proptest")]
use proptest::prelude::*;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
use runtime_state::state::StateValue;
use serde::{Deserialize, Serialize};
#[cfg(feature = "proptest")]
use serialize::randomised_serialization_test;
use serialize::{Deserializable, Serializable, Tagged, tag_enforcement_test};
use std::fmt::Debug;
use storage::db::DB;
#[cfg(feature = "proptest")]
use storage::db::InMemoryDB;
use storage::storable::Loader;
use storage::storage::Array;
use storage::{DefaultDB, Storable, arena::ArenaKey};
use transient_crypto::curve::Fr;
use transient_crypto::repr::FieldRepr;

#[derive(Clone, Hash, PartialEq, Eq, Serialize, Deserialize, Serializable, Storable)]
#[storable(base)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[serde(tag = "tag", content = "value", rename_all = "camelCase")]
#[tag = "impact-idx-key"]
pub enum Key {
    Value(AlignedValue),
    Stack,
}
tag_enforcement_test!(Key);

impl Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Key::Value(v) => v.fmt(f),
            Key::Stack => write!(f, "STK"),
        }
    }
}

impl TryFrom<Key> for AlignedValue {
    type Error = ();
    fn try_from(value: Key) -> Result<Self, Self::Error> {
        match value {
            Key::Value(v) => Ok(v),
            Key::Stack => Err(()),
        }
    }
}

impl FieldRepr for Key {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        match self {
            Key::Stack => writer.write(&[(-1).into()]),
            Key::Value(v) => v.field_repr(writer),
        }
    }

    fn field_size(&self) -> usize {
        match self {
            Key::Stack => 1,
            Key::Value(v) => v.field_size(),
        }
    }
}

#[non_exhaustive]
#[derive_where(Clone, Eq, PartialEq; M)]
#[derive(Serialize, Deserialize, Storable)]
#[serde(bound(
    serialize = "M::ReadResult : Serialize",
    deserialize = "M::ReadResult : Deserialize<'de>"
))]
#[serde(rename_all = "lowercase", expecting = "operation")]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[storable(db = D)]
#[tag = "impact-op[v1]"]
#[phantom(M)]
pub enum Op<M: ResultMode<D>, D: DB = DefaultDB> {
    Noop {
        #[cfg_attr(
            feature = "proptest",
            proptest(strategy = "any::<u32>().prop_map(|x| x % 0x1FFFFF)")
        )]
        n: u32,
    },
    Lt,
    Eq,
    Type,
    Size,
    New,
    And,
    Or,
    Neg,
    Log,
    Root,
    Pop,
    Popeq {
        cached: bool,
        result: M::ReadResult,
    },
    Addi {
        #[cfg_attr(
            feature = "proptest",
            proptest(strategy = "any::<u32>().prop_map(|x| x % 0x1FFFFF)")
        )]
        immediate: u32,
    },
    Subi {
        #[cfg_attr(
            feature = "proptest",
            proptest(strategy = "any::<u32>().prop_map(|x| x % 0x1FFFFF)")
        )]
        immediate: u32,
    },
    Push {
        storage: bool,
        value: StateValue<D>,
    },
    Branch {
        #[cfg_attr(
            feature = "proptest",
            proptest(strategy = "any::<u32>().prop_map(|x| x % 0x1FFFFF)")
        )]
        skip: u32,
    },
    Jmp {
        #[cfg_attr(
            feature = "proptest",
            proptest(strategy = "any::<u32>().prop_map(|x| x % 0x1FFFFF)")
        )]
        skip: u32,
    },
    Add,
    Sub,
    Concat {
        cached: bool,
        #[cfg_attr(
            feature = "proptest",
            proptest(strategy = "any::<u32>().prop_map(|x| x % 0x1FFFFF)")
        )]
        n: u32,
    },
    Member,
    Rem {
        cached: bool,
    },
    Dup {
        #[cfg_attr(
            feature = "proptest",
            proptest(strategy = "any::<u8>().prop_map(|x| x % 16)")
        )]
        n: u8,
    },
    Swap {
        #[cfg_attr(
            feature = "proptest",
            proptest(strategy = "any::<u8>().prop_map(|x| x % 16)")
        )]
        n: u8,
    },
    Idx {
        cached: bool,
        #[serde(rename = "pushPath")]
        push_path: bool,
        #[cfg_attr(
            feature = "proptest",
            proptest(
                strategy = "proptest::collection::vec(Key::arbitrary(), 1..2).prop_map_into()"
            )
        )]
        path: Array<Key, D>,
    },
    Ins {
        cached: bool,
        #[cfg_attr(
            feature = "proptest",
            proptest(strategy = "any::<u8>().prop_map(|x| x % 16)", filter = "|v| *v != 0")
        )]
        n: u8,
    },
    Ckpt,
}
tag_enforcement_test!(Op<ResultModeVerify>);

#[macro_export]
macro_rules! key {
    (stack) => {
        Key::Stack
    };
    ($val:expr_2021) => {
        Key::Value($val.into())
    };
}

#[macro_export]
macro_rules! op {
    (noop $val:expr_2021) => { Op::Noop { n: $val } };
    (lt) => { Op::Lt };
    (eq) => { Op::Eq };
    (type) => { Op::Type };
    (size) => { Op::Size };
    (new) => { Op::New };
    (and) => { Op::And };
    (or) => { Op::Or };
    (neg) => { Op::Neg };
    (log) => { Op::Log };
    (root) => { Op::Root };
    (pop) => { Op::Pop };
    (popeq $res:expr_2021) => { Op::Popeq { cached: false, result: $res } };
    (popeqc $res:expr_2021) => { Op::Popeq { cached: true, result: $res } };
    (addi $imm:expr_2021) => { Op::Addi { immediate: $imm } };
    (subi $imm:expr_2021) => { Op::Subi { immediate: $imm } };
    (push $val:tt) => { Op::Push { storage: false, value: stval! $val } };
    (pushs $val:tt) => { Op::Push { storage: true, value: stval! $val } };
    (branch $skip:expr_2021) => { Op::Branch { skip: $skip } };
    (jmp $skip:expr_2021) => { Op::Jmp { skip: $skip } };
    (add) => { Op::Add };
    (sub) => { Op::Sub };
    (concat $n:expr_2021) => { Op::Concat { cached: false, n: $n } };
    (concatc $n:expr_2021) => { Op::Concat { cached: true, n: $n } };
    (member) => { Op::Member };
    (rem) => { Op::Rem { cached: false } };
    (remc) => { Op::Rem { cached: true } };
    (dup $n:expr_2021) => { Op::Dup { n: $n } };
    (swap $n:expr_2021) => { Op::Swap { n: $n } };
    (idx [$($key:tt),*]) => { Op::Idx { cached: false, push_path: false, path: vec![$(key!($key)),*].into_iter().collect() }};
    (idxc [$($key:tt),*]) => { Op::Idx { cached: true, push_path: false, path: vec![$(key!($key)),*].into_iter().collect() }};
    (idxp [$($key:tt),*]) => { Op::Idx { cached: false, push_path: true, path: vec![$(key!($key)),*].into_iter().collect() }};
    (idxpc [$($key:tt),*]) => { Op::Idx { cached: true, push_path: true, path: vec![$(key!($key)),*].into_iter().collect() }};
    (ins $n:expr_2021) => { Op::Ins { cached: false, n: $n } };
    (insc $n:expr_2021) => { Op::Ins { cached: true, n: $n } };
    (ckpt) => { Op::Ckpt };
}

#[macro_export]
macro_rules! ops_int {
    [] => { std::iter::empty() };
    [;] => { std::iter::empty() };
    [$op0:tt ; $($ops:tt)*] => { std::iter::once(op!($op0)).chain(ops_int!($($ops)*)) };
    [$op0:tt $op1:tt ; $($ops:tt)*] => { std::iter::once(op!($op0 $op1)).chain(ops_int!($($ops)*)) };
    [$op0:tt $op1:tt $op2:tt ; $($ops:tt)*] => { std::iter::once(op!($op0 $op1 $op2)).chain(ops_int!($($ops)*)) };
    [$op0:tt $op1:tt $op2:tt $op3:tt ; $($ops:tt)*] => { std::iter::once(op!($op0 $op1 $op2 $op3)).chain(ops_int!($($ops)*)) };
    [$($ops:tt)*] => { std::iter::once(op!($($ops)*)) };
}

#[macro_export]
macro_rules! ops {
    [$($tts:tt)*] => { ops_int!($($tts)*).collect::<Vec<_>>() };
}

impl<M: ResultMode<D>, D: DB> Debug for Op<M, D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use Op::*;
        match self {
            Noop { n } => write!(f, "noop {n}"),
            Lt => write!(f, "lt"),
            Eq => write!(f, "eq"),
            Type => write!(f, "type"),
            Size => write!(f, "size"),
            New => write!(f, "new"),
            And => write!(f, "and"),
            Or => write!(f, "or"),
            Neg => write!(f, "neg"),
            Log => write!(f, "log"),
            Root => write!(f, "root"),
            Pop => write!(f, "pop"),
            Popeq {
                cached: false,
                result,
            } => write!(f, "popeq {result:?}"),
            Popeq {
                cached: true,
                result,
            } => write!(f, "popeqc {result:?}"),
            Addi { immediate } => write!(f, "addi {immediate:?}"),
            Subi { immediate } => write!(f, "subi {immediate:?}"),
            Push {
                storage: false,
                value,
            } => write!(f, "push {value:?}"),
            Push {
                storage: true,
                value,
            } => write!(f, "pushs {value:?}"),
            Branch { skip } => write!(f, "branch {skip}"),
            Jmp { skip } => write!(f, "jmp {skip}"),
            Add => write!(f, "add"),
            Sub => write!(f, "sub"),
            Concat { cached: false, n } => write!(f, "concat {n}"),
            Concat { cached: true, n } => write!(f, "concatc {n}"),
            Member => write!(f, "member"),
            Rem { cached: false } => write!(f, "rem"),
            Rem { cached: true } => write!(f, "remc"),
            Dup { n } => write!(f, "dup {n}"),
            Swap { n } => write!(f, "swap {n}"),
            Idx {
                cached,
                push_path,
                path,
            } => {
                write!(f, "idx")?;
                if *push_path {
                    write!(f, "p")?;
                }
                if *cached {
                    write!(f, "c")?;
                }
                write!(f, " [")?;
                let mut is_first = true;
                for key in path.iter() {
                    if is_first {
                        is_first = false;
                    } else {
                        write!(f, ", ")?;
                    }
                    write!(f, "{key:?}")?;
                }
                write!(f, "]")
            }
            Ins { cached: false, n } => write!(f, "ins {n}"),
            Ins { cached: true, n } => write!(f, "insc {n}"),
            Ckpt => write!(f, "ckpt"),
        }
    }
}

impl<M: ResultMode<D>, D: DB> Op<M, D> {
    pub fn translate<M2: ResultMode<D>, F: FnOnce(M::ReadResult) -> M2::ReadResult>(
        self,
        f: F,
    ) -> Op<M2, D> {
        match self {
            Op::Noop { n } => Op::Noop { n },
            Op::Lt => Op::Lt,
            Op::Eq => Op::Eq,
            Op::Type => Op::Type,
            Op::Size => Op::Size,
            Op::New => Op::New,
            Op::And => Op::And,
            Op::Or => Op::Or,
            Op::Neg => Op::Neg,
            Op::Log => Op::Log,
            Op::Root => Op::Root,
            Op::Pop => Op::Pop,
            Op::Popeq { cached, result } => Op::Popeq {
                cached,
                result: f(result),
            },
            Op::Addi { immediate } => Op::Addi { immediate },
            Op::Subi { immediate } => Op::Subi { immediate },
            Op::Push { storage, value } => Op::Push { storage, value },
            Op::Branch { skip } => Op::Branch { skip },
            Op::Jmp { skip } => Op::Jmp { skip },
            Op::Add => Op::Add,
            Op::Sub => Op::Sub,
            Op::Concat { cached, n } => Op::Concat { cached, n },
            Op::Member => Op::Member,
            Op::Rem { cached } => Op::Rem { cached },
            Op::Dup { n } => Op::Dup { n },
            Op::Swap { n } => Op::Swap { n },
            Op::Idx {
                cached,
                push_path,
                path,
            } => Op::Idx {
                cached,
                push_path,
                path,
            },
            Op::Ins { cached, n } => Op::Ins { cached, n },
            Op::Ckpt => Op::Ckpt,
        }
    }
}

#[cfg(feature = "proptest")]
#[allow(dead_code)]
type SimpleOp = Op<ResultModeVerify, InMemoryDB>;
#[cfg(feature = "proptest")]
randomised_serialization_test!(SimpleOp);

impl<D: DB> FieldRepr for Op<ResultModeVerify, D> {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        use Op::*;
        match self {
            Noop { n } => writer.write(&vec![0x00.into(); *n as usize]),
            Lt => writer.write(&[0x01.into()]),
            Eq => writer.write(&[0x02.into()]),
            Type => writer.write(&[0x03.into()]),
            Size => writer.write(&[0x04.into()]),
            New => writer.write(&[0x05.into()]),
            And => writer.write(&[0x06.into()]),
            Or => writer.write(&[0x07.into()]),
            Neg => writer.write(&[0x08.into()]),
            Log => writer.write(&[0x09.into()]),
            Root => writer.write(&[0x0a.into()]),
            Pop => writer.write(&[0x0b.into()]),
            Popeq { cached, result } => {
                writer.write(&[(0x0c + *cached as u8).into()]);
                result.field_repr(writer);
            }
            Addi { immediate } => {
                writer.write(&[0x0e.into()]);
                immediate.field_repr(writer);
            }
            Subi { immediate } => {
                writer.write(&[0x0f.into()]);
                immediate.field_repr(writer);
            }
            Push { storage, value } => {
                writer.write(&[(0x10 + *storage as u8).into()]);
                value.field_repr(writer);
            }
            Branch { skip } => writer.write(&[0x12.into(), (*skip).into()]),
            Jmp { skip } => writer.write(&[0x13.into(), (*skip).into()]),
            Add => writer.write(&[0x14.into()]),
            Sub => writer.write(&[0x15.into()]),
            Concat { cached: false, n } => writer.write(&[0x16.into(), (*n).into()]),
            Concat { cached: true, n } => writer.write(&[0x17.into(), (*n).into()]),
            Member => writer.write(&[0x18.into()]),
            Rem { cached: false } => writer.write(&[0x19.into()]),
            Rem { cached: true } => writer.write(&[0x1a.into()]),
            Dup { n } => writer.write(&[(0x30 | *n).into()]),
            Swap { n } => writer.write(&[(0x40 | *n).into()]),
            Idx {
                cached,
                push_path,
                path,
            } => {
                if !path.is_empty() {
                    let opcode = match (*cached, *push_path) {
                        (false, false) => 0x50,
                        (true, false) => 0x60,
                        (false, true) => 0x70,
                        (true, true) => 0x80,
                    } | (path.len() as u8 - 1);
                    writer.write(&[opcode.into()]);
                    for entry in path.iter() {
                        entry.field_repr(writer);
                    }
                }
            }
            Ins { cached: false, n } => writer.write(&[(0x90 | *n).into()]),
            Ins { cached: true, n } => writer.write(&[(0xa0 | *n).into()]),
            Ckpt => writer.write(&[0xff.into()]),
        }
    }

    fn field_size(&self) -> usize {
        use Op::*;
        match self {
            Lt
            | Eq
            | Type
            | Size
            | New
            | And
            | Or
            | Neg
            | Log
            | Root
            | Pop
            | Add
            | Sub
            | Member
            | Rem { .. }
            | Dup { .. }
            | Swap { .. }
            | Ins { .. }
            | Ckpt => 1,
            Noop { n } => *n as usize,
            Branch { .. } | Jmp { .. } | Concat { .. } => 2,
            Addi { immediate } | Subi { immediate } => 1 + immediate.field_size(),
            Popeq { result, .. } => 1 + result.field_size(),
            Push { value, .. } => 1 + value.field_size(),
            Idx { path, .. } => 1 + path.iter().map(|item| item.field_size()).sum::<usize>(),
        }
    }
}

pub use {key, op, ops, ops_int};

#[cfg(test)]
mod tests {
    use storage::DefaultDB;

    use super::Op;
    use crate::result_mode::ResultModeGather;
    use runtime_state::state::StateValue;
    use storage::storage::HashMap;

    #[test]
    fn diagnostic_test_map_serialization_stability() {
        let op: Op<ResultModeGather, DefaultDB> = Op::Push {
            storage: false,
            value: StateValue::Map(HashMap::new()),
        };
        let mut ser = Vec::new();
        serialize::Serializable::serialize(&op, &mut ser).unwrap();
        let op2: Op<ResultModeGather, DefaultDB> =
            serialize::Deserializable::deserialize(&mut &ser[..], 0).unwrap();
        assert_eq!(op, op2);
    }
}

#[cfg(all(test, feature = "proptest"))]
mod op_field_repr_properties {
    use super::*;
    use crate::result_mode::ResultModeVerify;
    use base_crypto::fab::AlignedValue;
    use runtime_state::state::StateValue;
    use serialize::{Deserializable, Serializable};
    use storage::db::InMemoryDB;
    use transient_crypto::repr::FieldRepr;

    /// Round-trip an op through the checked deserialization path (the untrusted
    /// decode boundary runs `Op::invariant` via `do_check`). This is the layer
    /// the operand bound lives at; `field_repr` itself is unchanged.
    fn decode(op: &TestOp) -> std::io::Result<TestOp> {
        let mut buf = Vec::new();
        Serializable::serialize(op, &mut buf)?;
        Deserializable::deserialize(&mut &buf[..], 0)
    }

    type TestOp = Op<ResultModeVerify, InMemoryDB>;

    /// A generator over the full argument range of every `Op` variant. Numeric
    /// arguments span their whole type (`u8`/`u32`), rather than being masked
    /// to the small ranges the derived `Arbitrary` uses. `Noop`'s counter is
    /// capped only to avoid multi-gigabyte allocations while still ranging far
    /// beyond its encoded byte size (which is what C2 is about).
    fn arb_op() -> impl Strategy<Value = TestOp> {
        let simple_value =
            prop_oneof![Just(StateValue::Null), any::<u64>().prop_map(StateValue::from)];
        let simple_result = any::<u8>().prop_map(AlignedValue::from);
        prop_oneof![
            (0u32..=100_000).prop_map(|n| Op::Noop { n }),
            Just(Op::Lt),
            Just(Op::Eq),
            Just(Op::Type),
            Just(Op::Size),
            Just(Op::New),
            Just(Op::And),
            Just(Op::Or),
            Just(Op::Neg),
            Just(Op::Log),
            Just(Op::Root),
            Just(Op::Pop),
            Just(Op::Add),
            Just(Op::Sub),
            Just(Op::Member),
            Just(Op::Ckpt),
            any::<u32>().prop_map(|immediate| Op::Addi { immediate }),
            any::<u32>().prop_map(|immediate| Op::Subi { immediate }),
            any::<u32>().prop_map(|skip| Op::Branch { skip }),
            any::<u32>().prop_map(|skip| Op::Jmp { skip }),
            (any::<bool>(), any::<u32>()).prop_map(|(cached, n)| Op::Concat { cached, n }),
            any::<bool>().prop_map(|cached| Op::Rem { cached }),
            // Full u8 range -- NOT masked to 0..16 as the derived Arbitrary does.
            any::<u8>().prop_map(|n| Op::Dup { n }),
            any::<u8>().prop_map(|n| Op::Swap { n }),
            (any::<bool>(), any::<u8>()).prop_map(|(cached, n)| Op::Ins { cached, n }),
            // Path length includes 0 (the empty path) and >16.
            (
                any::<bool>(),
                any::<bool>(),
                proptest::collection::vec(
                    any::<u8>().prop_map(|v| Key::Value(AlignedValue::from(v))),
                    0..3
                ),
            )
                .prop_map(|(cached, push_path, keys)| Op::Idx {
                    cached,
                    push_path,
                    path: keys.into_iter().collect(),
                }),
            (any::<bool>(), simple_value).prop_map(|(storage, value)| Op::Push { storage, value }),
            (any::<bool>(), simple_result)
                .prop_map(|(cached, result)| Op::Popeq { cached, result }),
        ]
    }

    /// A generator biased towards the variants whose `field_repr` collides, so
    /// the injectivity property reliably samples colliding pairs: `Noop` with a
    /// tiny counter, `Idx` with an empty path, and `Dup`/`Swap`/`Ins` over the
    /// full `u8` range (whose low nibble is OR-ed into a fixed opcode).
    fn arb_collision_prone_op() -> impl Strategy<Value = TestOp> {
        prop_oneof![
            (0u32..=3).prop_map(|n| Op::Noop { n }),
            (any::<bool>(), any::<bool>()).prop_map(|(cached, push_path)| Op::Idx {
                cached,
                push_path,
                path: Vec::new().into_iter().collect(),
            }),
            any::<u8>().prop_map(|n| Op::Dup { n }),
            any::<u8>().prop_map(|n| Op::Swap { n }),
            (any::<bool>(), any::<u8>()).prop_map(|(cached, n)| Op::Ins { cached, n }),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1024))]

        /// (C1) Distinct `Op`s produce distinct `field_repr` outputs.
        ///
        /// `field_repr` is the proof's sole program binding, and it is only
        /// injective for in-range operands (`Dup{n}`/`Dup{n+16}` alias; an
        /// empty-path `Idx` aliases `Noop{0}`). The decode bound is exactly what
        /// makes it injective over the reachable domain, so the property is
        /// stated over ops the decoder admits.
        #[test]
        fn field_repr_is_injective(ops in proptest::collection::vec(arb_collision_prone_op(), 2..48)) {
            let ops: Vec<TestOp> = ops.into_iter().filter(|o| decode(o).is_ok()).collect();
            for i in 0..ops.len() {
                for j in (i + 1)..ops.len() {
                    if ops[i] != ops[j] {
                        prop_assert_ne!(
                            ops[i].field_vec(),
                            ops[j].field_vec(),
                            "distinct decodable ops share a field_repr: {:?} vs {:?}",
                            ops[i], ops[j]
                        );
                    }
                }
            }
        }

        /// Sanity: the binary encoding IS injective for distinct ops. This
        /// isolates the defect to `field_repr` (it passes on the current base).
        #[test]
        fn serialization_is_injective(a in arb_op(), b in arb_op()) {
            if a != b {
                let mut ba = Vec::new();
                let mut bb = Vec::new();
                Serializable::serialize(&a, &mut ba).unwrap();
                Serializable::serialize(&b, &mut bb).unwrap();
                prop_assert_ne!(ba, bb, "distinct ops share an encoding: {:?} vs {:?}", a, b);
            }
        }

        /// `field_vec().len()` must equal `field_size()` (the latter is the
        /// allocation hint). This holds for every decodable op; an empty-path
        /// `Idx` (`field_size() == 1`, emits nothing) is now rejected on decode.
        #[test]
        fn field_size_matches_field_repr_length(op in arb_op()) {
            prop_assume!(decode(&op).is_ok());
            prop_assert_eq!(op.field_vec().len(), op.field_size(), "for {:?}", op);
        }
    }

    // --- Regression tests: the decode boundary rejects the ops whose
    // `field_repr` would collide (the proof's program binding). ---

    /// `Dup{n}`/`Swap{n}`/`Ins{_,n}` OR `n` into the opcode nibble, so `n >= 16`
    /// aliases the low-nibble op in `field_repr`; the decoder must reject them.
    #[test]
    fn dup_swap_ins_out_of_range_rejected() {
        for op in [
            Op::Dup { n: 16 },
            Op::Swap { n: 16 },
            Op::Ins { cached: false, n: 16 },
            Op::Ins { cached: true, n: 16 },
        ] {
            let op: TestOp = op;
            assert!(decode(&op).is_err(), "out-of-range {op:?} must be rejected on decode");
        }
        for op in [
            Op::Dup { n: 15 },
            Op::Swap { n: 0 },
            Op::Ins { cached: true, n: 15 },
        ] {
            let op: TestOp = op;
            assert!(decode(&op).is_ok(), "in-range {op:?} must decode");
        }
    }

    /// An empty-path `Idx` emits no field elements (aliasing `Noop{0}`) and
    /// over-reports `field_size`; a path longer than 16 overflows the opcode
    /// nibble. Both are rejected on decode; a 1..=16 path and `Noop{0}` decode.
    #[test]
    fn idx_path_length_bounds_enforced() {
        let empty: TestOp = Op::Idx {
            cached: false,
            push_path: false,
            path: Vec::new().into_iter().collect(),
        };
        assert!(decode(&empty).is_err(), "empty-path Idx must be rejected");

        let long: TestOp = Op::Idx {
            cached: false,
            push_path: false,
            path: (0u8..17).map(|v| Key::Value(AlignedValue::from(v))).collect(),
        };
        assert!(decode(&long).is_err(), "17-entry Idx path must be rejected");

        let ok: TestOp = Op::Idx {
            cached: false,
            push_path: false,
            path: (0u8..16).map(|v| Key::Value(AlignedValue::from(v))).collect(),
        };
        assert!(decode(&ok).is_ok(), "16-entry Idx path must decode");
        let noop0: TestOp = Op::Noop { n: 0 };
        assert!(decode(&noop0).is_ok(), "Noop{{0}} must still decode");
    }
}
