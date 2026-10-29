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

#[cfg(test)]
mod tests {
    use base_crypto::fab::{Aligned, AlignedValue};
    use coin_structure::contract::ContractAddress;
    use midnight_onchain_runtime::program_fragments::*;
    use midnight_onchain_runtime::state::*;
    use midnight_onchain_runtime::test_utilities::{run_program, run_program_step_limited};
    use onchain_runtime_state::stval;
    use onchain_vm::error::OnchainProgramError;
    use onchain_vm::ops::*;
    use onchain_vm::result_mode::*;
    use onchain_vm::vm_value::{ValueStrength, VmValue};
    use onchain_vm::vmval;
    use std::slice;
    use storage::arena::Sp;
    use storage::db::InMemoryDB;
    use storage::storage::HashMap;
    use transient_crypto::fab::{AlignmentExt, ValueReprAlignedValue};
    use transient_crypto::merkle_tree::{MerkleTree, MerkleTreeDigest, leaf_hash};

    macro_rules! read {
        ($val:expr_2021) => {
            GatherEvent::Read($val.into())
        };
    }

    #[test]
    fn run_program_step_limited_respects_limit() {
        let run_program_step_limited = run_program_step_limited::<InMemoryDB, ResultModeGather>;
        let stack = [vmval!([null]), vmval!([null]), vmval!([null])];
        let ops = ops![pop; pop; pop];

        let step_limit = Some(1);
        let result = run_program_step_limited(&stack, &ops, step_limit).unwrap();
        assert_eq!(result.0.len(), 2);

        let step_limit = Some(2);
        let result = run_program_step_limited(&stack, &ops, step_limit).unwrap();
        assert_eq!(result.0.len(), 1);

        let step_limit = None;
        let result = run_program_step_limited(&stack, &ops, step_limit).unwrap();
        assert_eq!(result.0.len(), 0);
    }

    #[test]
    fn individual_vm_instructions() {
        let logic_operands = vec![(true, true), (true, false), (false, true), (false, false)];
        let cmp_operands = vec![(4u64, 5u64), (5u64, 5u64), (6u64, 5u64)];
        let arithmetic_operands = vec![(5u64, 4u64)];

        let run_program = run_program::<InMemoryDB, ResultModeGather>;

        // noop
        assert_eq!(
            run_program(&[vmval!([null])], &ops![noop(0)]),
            Ok((vec![vmval!([null])], vec![]))
        );

        for ops in cmp_operands {
            // lt
            assert_eq!(
                run_program(&[vmval!((ops.0)), vmval!((ops.1))], &ops![lt]),
                Ok((vec![vmval!((ops.0 < ops.1))], vec![]))
            );

            // eq
            assert_eq!(
                run_program(&[vmval!((ops.0)), vmval!((ops.1))], &ops![eq]),
                Ok((vec![vmval!((ops.0 == ops.1))], vec![]))
            );
        }

        for ops in logic_operands {
            // and
            assert_eq!(
                run_program(&[vmval!((ops.0)), vmval!((ops.1))], &ops![and]),
                Ok((vec![vmval!((ops.0 && ops.1))], vec![]))
            );

            // or
            assert_eq!(
                run_program(&[vmval!((ops.0)), vmval!((ops.1))], &ops![or]),
                Ok((vec![vmval!((ops.0 || ops.1))], vec![]))
            );
        }

        for ops in arithmetic_operands {
            // add
            assert_eq!(
                run_program(&[vmval!((ops.0)), vmval!((ops.1))], &ops![add]),
                Ok((vec![vmval!((ops.0 + ops.1))], vec![]))
            );

            // addi
            assert_eq!(
                run_program(&[vmval!((ops.0))], &ops![addi(ops.1 as u32)]),
                Ok((vec![vmval!((ops.0 + ops.1))], vec![]))
            );

            // sub
            assert_eq!(
                run_program(&[vmval!((ops.0)), vmval!((ops.1))], &ops![sub]),
                Ok((vec![vmval!((ops.0 - ops.1))], vec![]))
            );

            // subi
            assert_eq!(
                run_program(&[vmval!((ops.0))], &ops![subi(ops.1 as u32)]),
                Ok((vec![vmval!((ops.0 - ops.1))], vec![]))
            );
        }

        // size
        assert_eq!(
            run_program(&[vmval!([(6u64)])], &ops![size]),
            Ok((vec![vmval!((1u64))], vec![]))
        );

        // new
        assert_eq!(
            run_program(&[vmval!((4u64))], &ops![new]),
            Ok((vec![vmval!({MT(1) {}})], vec![]))
        );

        // neg
        assert_eq!(
            run_program(&[vmval!((true))], &ops![neg]),
            Ok((vec![vmval!((false))], vec![]))
        );

        assert_eq!(
            run_program(&[vmval!((false))], &ops![neg]),
            Ok((vec![vmval!((true))], vec![]))
        );

        // log
        assert_eq!(
            run_program(&[vmval!((4u64))], &ops![log]),
            Ok((
                vec![],
                vec![GatherEvent::Log(VersionedLogItem {
                    version: 0,
                    event_type: LogEventType::Misc,
                    data: stval!((4u64))
                })]
            ))
        );

        assert_eq!(
            run_program(&[vmval!([(2u32), (8u8), (4u64)])], &ops![log]),
            Ok((
                vec![],
                vec![GatherEvent::Log(VersionedLogItem {
                    version: 2,
                    event_type: LogEventType::Paused,
                    data: stval!((4u64))
                })]
            ))
        );

        // pop
        assert_eq!(
            run_program(&[vmval!((4u64)), vmval!((4u64))], &ops![pop]),
            Ok((vec![vmval!((4u64))], vec![]))
        );

        // popeq
        assert_eq!(
            run_program(&[vmval!((4u64)), vmval!((4u64))], &ops![popeq()]),
            Ok((
                vec![vmval!((4u64))],
                vec![GatherEvent::Read(AlignedValue::from(4u64))]
            ))
        );

        // popeqc
        assert_eq!(
            run_program(&[vmval!((4u64)), vmval!((4u64))], &ops![popeqc()]),
            Ok((
                vec![vmval!((4u64))],
                vec![GatherEvent::Read(AlignedValue::from(4u64))]
            ))
        );

        // push
        assert_eq!(
            run_program(&[], &ops![push((2u64))]),
            Ok((vec![vmval!(# (2u64))], vec![]))
        );

        // pushs
        assert_eq!(
            run_program(&[], &ops![push((2u64))]),
            Ok((vec![vmval!(# (2u64))], vec![]))
        );

        // branch
        assert_eq!(
            run_program(&[vmval!((2u64))], &ops![branch (1); noop (0)]),
            Ok((vec![], vec![]))
        );

        // jmp
        assert_eq!(
            run_program(&[], &ops![jmp (1); noop (0)]),
            Ok((vec![], vec![]))
        );

        // member
        assert_eq!(
            run_program(
                &[
                    vmval!([{10u32 => null, 16u32 => null}]),
                    vmval!([{10u32 => null, 16u32 => null}]),
                ],
                &ops![idx [0u8]; push ((16u32)); member]
            ),
            Ok((
                vec![vmval!([{10u32 => null, 16u32 => null}]), vmval!((true))],
                vec![]
            ))
        );

        // rem
        assert_eq!(
            run_program(
                &[vmval!([
                        {10u32 => null, 16u32 => null}
                ])],
                &ops![idx [0u8]; push ((16u32)); rem]
            ),
            Ok((vec![vmval!({10u32 => null})], vec![]))
        );

        // dup
        assert_eq!(
            run_program(&[vmval!((5u64)), vmval!((5u64))], &ops![dup(1)]),
            Ok((vec![vmval!((5u64)), vmval!((5u64)), vmval!((5u64))], vec![]))
        );

        // swap
        assert_eq!(
            run_program(&[vmval!((5u64)), vmval!((5u64))], &ops![dup(1)]),
            Ok((vec![vmval!((5u64)), vmval!((5u64)), vmval!((5u64))], vec![]))
        );

        // idx
        assert_eq!(
            run_program(&[vmval!({MT(1) {}})], &ops![idx[0u8]]),
            Ok((vec!(vmval!(null)), vec![]))
        );

        // idxc
        assert_eq!(
            run_program(&[vmval!(# [null])], &ops![idxc[0u8]]),
            Ok((vec!(vmval!(# null)), vec![]))
        );

        // idxp
        assert_eq!(
            run_program(
                &[
                    vmval!((42u32)),
                    vmval!((24u64)),
                    vmval!([{42u32 => (24u64)}]),
                ],
                &ops![idxp[0u8]; insc 1]
            ),
            Ok((
                vec!(
                    vmval!((42u32)),
                    vmval!((24u64)),
                    vmval!([{42u32 => (24u64)}]),
                ),
                vec![]
            ))
        );

        // ckpt
        assert_eq!(run_program(&[], &ops![ckpt]), Ok((vec![], vec![])));

        // type
        assert_eq!(
            run_program(&[vmval!([(4u64)])], &ops![type]),
            Ok((vec![vmval!((11u8))], vec![]))
        );

        // root is covered by the merkle tree tests below

        // concat, catching errors
        run_program(&[vmval!((4u64)), vmval!((5u64))], &ops![concat(4)]).unwrap();
        run_program(&[vmval!((4u64)), vmval!((5u64))], &ops![concatc(4)]).unwrap();
    }

    #[test]
    fn counter_flow() {
        let f = [Key::Value(0u8.into())];
        let run_program = run_program::<InMemoryDB, ResultModeGather>;
        assert_eq!(
            run_program(&[vmval!([null])], &Counter_reset_to_default!(f, false)),
            Ok((vec![vmval!([(0u64)])], vec![]))
        );
        assert_eq!(
            run_program(&[vmval!([(0u64)])], &Counter_increment!(f, false, 5)),
            Ok((vec![vmval!([(5u64)])], vec![]))
        );
        assert_eq!(
            run_program(&[vmval!([(5u64)])], &Counter_read!(f, false)),
            Ok((vec![vmval!([(5u64)])], vec![read!(5u64)]))
        );
        assert_eq!(
            run_program(&[vmval!([(5u64)])], &Counter_decrement!(f, false, 1)),
            Ok((vec![vmval!([(4u64)])], vec![]))
        );
        assert_eq!(
            run_program(&[vmval!([(4u64)])], &Counter_less_than!(f, false, 4u32)),
            Ok((vec![vmval!([(4u64)])], vec![read!(false)]))
        );
        assert_eq!(
            run_program(&[vmval!([(4u64)])], &Counter_less_than!(f, false, 5u32)),
            Ok((vec![vmval!([(4u64)])], vec![read!(true)]))
        );
    }

    #[test]
    fn counter_overflow() {
        let f = [Key::Value(0u8.into())];
        let run_program = run_program::<InMemoryDB, ResultModeGather>;
        // overflow u64 max
        assert_eq!(
            run_program(&[vmval!([(u64::MAX)])], &Counter_increment!(f, false, 5u32)),
            Err(OnchainProgramError::ArithmeticOverflow)
        );
        // underflow below 0
        assert_eq!(
            run_program(&[vmval!([(0u64)])], &Counter_decrement!(f, false, 1u32)),
            Err(OnchainProgramError::ArithmeticOverflow)
        );
    }

    #[test]
    fn cell_flow() {
        let f = [Key::Value(0u8.into())];
        let run_program = run_program::<InMemoryDB, ResultModeGather>;
        assert_eq!(
            run_program(
                &[vmval!([null])],
                &Cell_write!(f, false, Maybe<u64>, (false, 0u64))
            ),
            Ok((vec![vmval!([((false, 0u64))])], vec![]))
        );
        assert_eq!(
            run_program(
                &[vmval!([((false, 0u64))])],
                &Cell_write!(f, false, Maybe<u64>, (true, 26u64))
            ),
            Ok((vec![vmval!([((true, 26u64))])], vec![]))
        );
        assert_eq!(
            run_program(
                &[vmval!([((true, 26u64))])],
                &Cell_read!(f, false, Maybe<u64>)
            ),
            Ok((vec![vmval!([((true, 26u64))])], vec![read!((true, 26u64))]))
        );
    }

    #[test]
    fn set_flow() {
        let f = [Key::Value(0u8.into())];
        let run_program = run_program::<InMemoryDB, ResultModeGather>;
        assert_eq!(
            run_program(&[vmval!([null])], &Set_reset_to_default!(f, false, u32)),
            Ok((vec![vmval!([{}])], vec![]))
        );
        assert_eq!(
            run_program(&[vmval!([{}])], &Set_is_empty!(f, false, u32)),
            Ok((vec![vmval!([{}])], vec![read!(true)]))
        );
        assert_eq!(
            run_program(&[vmval!([{}])], &Set_size!(f, false, u32),),
            Ok((vec![vmval!([{}])], vec![read!(0u64)]))
        );
        assert_eq!(
            run_program(&[vmval!([{}])], &Set_insert!(f, false, u32, 10u32),),
            Ok((vec![vmval!([{10u32 => null}])], vec![]))
        );
        assert_eq!(
            run_program(&[vmval!([{10u32 => null}])], &Set_is_empty!(f, false, u32),),
            Ok((vec![vmval!([{10u32 => null}])], vec![read!(false)]))
        );
        assert_eq!(
            run_program(&[vmval!([{10u32 => null}])], &Set_size!(f, false, u32),),
            Ok((vec![vmval!([{10u32 => null}])], vec![read!(1u64)]))
        );
        assert_eq!(
            run_program(
                &[vmval!([{10u32 => null}])],
                &Set_insert!(f, false, u32, 16u32),
            ),
            Ok((vec![vmval!([{10u32 => null, 16u32 => null}])], vec![]))
        );
        assert_eq!(
            run_program(
                &[vmval!([{10u32 => null, 16u32 => null}])],
                &Set_member!(f, false, u32, 16u32),
            ),
            Ok((
                vec![vmval!([{10u32 => null, 16u32 => null}])],
                vec![read!(true)]
            ))
        );
        assert_eq!(
            run_program(
                &[vmval!([{10u32 => null, 16u32 => null}])],
                &Set_remove!(f, false, u32, 16u32),
            ),
            Ok((vec![vmval!([{10u32 => null}])], vec![]))
        );
        assert_eq!(
            run_program(
                &[vmval!([{10u32 => null}])],
                &Set_member!(f, false, u32, 16u32),
            ),
            Ok((vec![vmval!([{10u32 => null}])], vec![read!(false)]))
        );
    }

    #[test]
    fn map_flow() {
        let f = [Key::Value(0u8.into())];
        let run_program = run_program::<InMemoryDB, ResultModeGather>;
        assert_eq!(
            run_program(
                &[vmval!([null])],
                &Map_reset_to_default!(f, false, u32, u64),
            ),
            Ok((vec![vmval!([{}])], vec![]))
        );
        assert_eq!(
            run_program(&[vmval!([{}])], &Map_is_empty!(f, false, u32, u64),),
            Ok((vec![vmval!([{}])], vec![read!(true)]))
        );
        assert_eq!(
            run_program(&[vmval!([{}])], &Map_size!(f, false, u32, u64),),
            Ok((vec![vmval!([{}])], vec![read!(0u64)]))
        );
        assert_eq!(
            run_program(
                &[vmval!([{}])],
                &Map_insert!(f, false, u32, u64, 42u32, 24u64),
            ),
            Ok((vec![vmval!([{42u32 => (24u64)}])], vec![]))
        );
        assert_eq!(
            run_program(
                &[vmval!([{42u32 => (24u64)}])],
                &Map_insert!(f, false, u32, u64, 42u32, 23u64),
            ),
            Ok((vec![vmval!([{42u32 => (23u64)}])], vec![]))
        );
        assert_eq!(
            run_program(
                &[vmval!([{42u32 => (23u64)}])],
                &Map_is_empty!(f, false, u32, u64),
            ),
            Ok((vec![vmval!([{42u32 => (23u64)}])], vec![read!(false)]))
        );
        assert_eq!(
            run_program(
                &[vmval!([{42u32 => (23u64)}])],
                &Map_size!(f, false, u32, u64),
            ),
            Ok((vec![vmval!([{42u32 => (23u64)}])], vec![read!(1u64)]))
        );
        assert_eq!(
            run_program(
                &[vmval!([{42u32 => (23u64)}])],
                &Map_lookup!(f, false, u32, u64, 42u32),
            ),
            Ok((vec![vmval!([{42u32 => (23u64)}])], vec![read!(23u64)]))
        );
        assert_eq!(
            run_program(
                &[vmval!([{42u32 => (23u64)}])],
                &Map_lookup!(f, false, u32, u64, 24u32),
            ),
            Err(OnchainProgramError::ExpectedCell(StateValue::Null))
        );
        assert_eq!(
            run_program(
                &[vmval!([{42u32 => (23u64)}])],
                &Map_member!(f, false, u32, u64, 42u32),
            ),
            Ok((vec![vmval!([{42u32 => (23u64)}])], vec![read!(true)]))
        );
        assert_eq!(
            run_program(
                &[vmval!([{42u32 => (23u64)}])],
                &Map_member!(f, false, u32, u64, 24u32),
            ),
            Ok((vec![vmval!([{42u32 => (23u64)}])], vec![read!(false)]))
        );
        assert_eq!(
            run_program(
                &[vmval!([{42u32 => (23u64)}])],
                &Map_insert!(f, false, u32, u64, 24u32, 5u64),
            ),
            Ok((vec![vmval!([{24u32 => (5u64), 42u32 => (23u64)}])], vec![]))
        );
        assert_eq!(
            run_program(
                &[vmval!([{24u32 => (5u64), 42u32 => (23u64)}])],
                &Map_remove!(f, false, u32, u64, 42u32),
            ),
            Ok((vec![vmval!([{24u32 => (5u64)}])], vec![]))
        );
    }

    #[test]
    fn list_flow() {
        let f = [Key::Value(0u8.into())];
        let run_program = run_program::<InMemoryDB, ResultModeGather>;
        let empty_list = vmval!([[null, null, (0u64)]]);
        assert_eq!(
            run_program(&[vmval!([null])], &List_reset_to_default!(f, false, u64),),
            Ok((vec![empty_list.clone()], vec![]))
        );
        assert_eq!(
            run_program(slice::from_ref(&empty_list), &List_head!(f, false, u64),),
            Ok((vec![empty_list.clone()], vec![read!((false, 0u64))])),
        );
        assert_eq!(
            run_program(slice::from_ref(&empty_list), &List_is_empty!(f, false, u64),),
            Ok((vec![empty_list.clone()], vec![read!(true)]))
        );
        assert_eq!(
            run_program(slice::from_ref(&empty_list), &List_length!(f, false, u64),),
            Ok((vec![empty_list.clone()], vec![read!(0u64)]))
        );
        let list_5 = vmval!([[(5u64), [null, null, (0u64)], (1u64)]]);
        assert_eq!(
            run_program(
                slice::from_ref(&empty_list),
                &List_push_front!(f, false, u64, 5u64),
            ),
            Ok((vec![list_5.clone()], vec![]))
        );
        let list_5_12 = vmval!([[(12u64), [(5u64), [null, null, (0u64)], (1u64)], (2u64)]]);
        assert_eq!(
            run_program(
                slice::from_ref(&list_5),
                &List_push_front!(f, false, u64, 12u64),
            ),
            Ok((vec![list_5_12.clone()], vec![]))
        );
        assert_eq!(
            run_program(slice::from_ref(&list_5_12), &List_head!(f, false, u64),),
            Ok((vec![list_5_12.clone()], vec![read!((true, 12u64))]))
        );
        assert_eq!(
            run_program(slice::from_ref(&list_5_12), &List_is_empty!(f, false, u64),),
            Ok((vec![list_5_12.clone()], vec![read!(false)]))
        );
        assert_eq!(
            run_program(slice::from_ref(&list_5_12), &List_length!(f, false, u64)),
            Ok((vec![list_5_12.clone()], vec![read!(2u64)]))
        );
        assert_eq!(
            run_program(slice::from_ref(&list_5_12), &List_pop_front!(f, false, u64),),
            Ok((vec![list_5.clone()], vec![]))
        );
    }

    #[test]
    fn merkle_tree_flow() {
        let f = [Key::Value(0u8.into())];
        let run_program = run_program::<InMemoryDB, ResultModeGather>;
        fn root(value: VmValue<InMemoryDB>) -> MerkleTreeDigest {
            match &value.value {
                StateValue::BoundedMerkleTree(t) => t.rehash().root().unwrap(),
                _ => unreachable!(),
            }
        }
        let empty_mtree = vmval!([[{MT(8) {}}, (0u64)]]);
        assert_eq!(
            run_program(
                &[vmval!([null])],
                &MerkleTree_reset_to_default!(f, false, 8, u8),
            ),
            Ok((vec![empty_mtree.clone()], vec![]))
        );
        assert_eq!(
            run_program(
                slice::from_ref(&empty_mtree),
                &MerkleTree_check_root!(f, false, 8, u8, root(vmval!({MT(8) {}}))),
            ),
            Ok((vec![empty_mtree.clone()], vec![read!(true)]))
        );
        let fake_hash_5 = leaf_hash(&5u8);
        leaf_hash(&ValueReprAlignedValue(AlignedValue::from(5u8)));
        let mtree_12 = vmval!([[{MT(8) {12 => fake_hash_5}}, (13u64)]]);
        assert_eq!(
            run_program(
                slice::from_ref(&empty_mtree),
                &MerkleTree_insert_index!(f, false, 8, u8, 5u8, 12u64),
            ),
            Ok((vec![mtree_12.clone()], vec![]))
        );
        let mtree_5_12 = vmval!([[
            {MT(8) {5 => fake_hash_5, 12 => fake_hash_5}},
            (13u64)
        ]]);
        assert_eq!(
            run_program(
                slice::from_ref(&mtree_12),
                &MerkleTree_insert_index!(f, false, 8, u8, 5u8, 5u64),
            ),
            Ok((vec![mtree_5_12.clone()], vec![]))
        );
        assert_eq!(
            run_program(
                slice::from_ref(&mtree_5_12),
                &MerkleTree_insert_index!(f, false, 8, u8, 5u8, 256u64),
            ),
            Err(OnchainProgramError::BoundsExceeded)
        );
        let full_tree = vmval!([[
            {MT(8) {5 => fake_hash_5, 12 => fake_hash_5, 255 => fake_hash_5}},
            (256u64)
        ]]);
        assert_eq!(
            run_program(
                slice::from_ref(&mtree_5_12),
                &MerkleTree_insert_index!(f, false, 8, u8, 5u8, 255u64),
            ),
            Ok((vec![full_tree.clone()], vec![]))
        );
        assert_eq!(
            run_program(
                slice::from_ref(&full_tree),
                &MerkleTree_is_full!(f, false, 8, u8),
            ),
            Ok((vec![full_tree.clone()], vec![read!(true)]))
        );
        assert_eq!(
            run_program(
                slice::from_ref(&mtree_5_12),
                &MerkleTree_is_full!(f, false, 8, u8),
            ),
            Ok((vec![mtree_5_12.clone()], vec![read!(false)]))
        );
        assert_eq!(
            run_program(
                slice::from_ref(&full_tree),
                &MerkleTree_insert!(f, false, 8, u8, 5u8),
            ),
            Err(OnchainProgramError::BoundsExceeded)
        );
        let mtree_5_12_13 = vmval!([[
            {MT(8) {5 => fake_hash_5, 12 => fake_hash_5, 13 => fake_hash_5}},
            (14u64)
        ]]);
        assert_eq!(
            run_program(
                slice::from_ref(&mtree_5_12),
                &MerkleTree_insert!(f, false, 8, u8, 5u8),
            ),
            Ok((vec![mtree_5_12_13.clone()], vec![]))
        );
        let correct_root =
            root(vmval!({MT(8) {5 => fake_hash_5, 12 => fake_hash_5, 13 => fake_hash_5}}));
        assert_eq!(
            run_program(
                slice::from_ref(&mtree_5_12_13),
                &MerkleTree_check_root!(f, false, 8, u8, correct_root),
            ),
            Ok((vec![mtree_5_12_13.clone()], vec![read!(true)]))
        );
        let old_root = root(vmval!({MT(8) {5 => fake_hash_5, 12 => fake_hash_5}}));
        assert_eq!(
            run_program(
                slice::from_ref(&mtree_5_12_13),
                &MerkleTree_check_root!(f, false, 8, u8, old_root),
            ),
            Ok((vec![mtree_5_12_13.clone()], vec![read!(false)]))
        );
    }

    #[test]
    fn historic_merkle_tree_flow() {
        let f = [Key::Value(0u8.into())];
        let run_program = run_program::<InMemoryDB, ResultModeGather>;
        fn root(value: VmValue<InMemoryDB>) -> MerkleTreeDigest {
            match &value.value {
                StateValue::BoundedMerkleTree(t) => t.rehash().root().unwrap(),
                _ => unreachable!(),
            }
        }
        let empty_mtree = vmval!([[
            {MT(8) {}},
            (0u64),
            {root(vmval!({MT(8) {}})) => null}
        ]]);
        assert_eq!(
            run_program(
                &[vmval!([null])],
                &HistoricMerkleTree_reset_to_default!(f, false, 8, u8),
            ),
            Ok((vec![empty_mtree.clone()], vec![]))
        );
        assert_eq!(
            run_program(
                slice::from_ref(&empty_mtree),
                &HistoricMerkleTree_check_root!(f, false, 8, u8, root(vmval!({MT(8) {}}))),
            ),
            Ok((vec![empty_mtree.clone()], vec![read!(true)]))
        );
        let fake_hash_5 = leaf_hash(&5u8);
        let mtree_12 = vmval!([[
            {MT(8) {12 => fake_hash_5}},
            (13u64),
        {
            root(vmval!({MT(8) {}})) => null,
            root(vmval!({MT(8) {12 => fake_hash_5}})) => null
        }
        ]]);
        assert_eq!(
            run_program(
                slice::from_ref(&empty_mtree),
                &HistoricMerkleTree_insert_index!(f, false, 8, u8, 5u8, 12u64),
            ),
            Ok((vec![mtree_12.clone()], vec![]))
        );
        let mtree_5_12 = vmval!([[
            {MT(8) {5 => fake_hash_5, 12 => fake_hash_5}},
            (13u64),
        {
            root(vmval!({MT(8) {}})) => null,
            root(vmval!{{MT(8) {12 => fake_hash_5}}}) => null,
            root(vmval!({MT(8) {5 => fake_hash_5, 12 => fake_hash_5}})) => null
        }
        ]]);
        assert_eq!(
            run_program(
                slice::from_ref(&mtree_12),
                &HistoricMerkleTree_insert_index!(f, false, 8, u8, 5u8, 5u64),
            ),
            Ok((vec![mtree_5_12.clone()], vec![]))
        );
        assert_eq!(
            run_program(
                slice::from_ref(&mtree_5_12),
                &HistoricMerkleTree_is_full!(f, false, 8, u8),
            ),
            Ok((vec![mtree_5_12.clone()], vec![read!(false)]))
        );
        let mtree_5_12_13 = vmval!([[
            {MT(8) {5 => fake_hash_5, 12 => fake_hash_5, 13 => fake_hash_5}},
            (14u64),
        {
            root(vmval!({MT(8) {}})) => null,
            root(vmval!{{MT(8) {12 => fake_hash_5}}}) => null,
            root(vmval!({MT(8) {5 => fake_hash_5, 12 => fake_hash_5}})) => null,
            root(vmval!({MT(8) {5 => fake_hash_5, 12 => fake_hash_5, 13 => fake_hash_5}})) => null
        }
        ]]);
        assert_eq!(
            run_program(
                slice::from_ref(&mtree_5_12),
                &HistoricMerkleTree_insert!(f, false, 8, u8, 5u8),
            ),
            Ok((vec![mtree_5_12_13.clone()], vec![]))
        );
        let current_root =
            root(vmval!({MT(8) {5 => fake_hash_5, 12 => fake_hash_5, 13 => fake_hash_5}}));
        assert_eq!(
            run_program(
                slice::from_ref(&mtree_5_12_13),
                &HistoricMerkleTree_check_root!(f, false, 8, u8, current_root),
            ),
            Ok((vec![mtree_5_12_13.clone()], vec![read!(true)]))
        );
        let old_root = root(vmval!({MT(8) {5 => fake_hash_5, 12 => fake_hash_5}}));
        assert_eq!(
            run_program(
                slice::from_ref(&mtree_5_12_13),
                &HistoricMerkleTree_check_root!(f, false, 8, u8, old_root),
            ),
            Ok((vec![mtree_5_12_13.clone()], vec![read!(true)]))
        );
        let mtree_5_12_13_no_hist = vmval!([[
            {MT(8) {5 => fake_hash_5, 12 => fake_hash_5, 13 => fake_hash_5}},
            (14u64),
        {
            root(vmval!({MT(8) {5 => fake_hash_5, 12 => fake_hash_5, 13 => fake_hash_5}})) => null
        }
        ]]);
        assert_eq!(
            run_program(
                &[mtree_5_12_13],
                &HistoricMerkleTree_reset_history!(f, false, 8, u8),
            ),
            Ok((vec![mtree_5_12_13_no_hist.clone()], vec![]))
        );
        assert_eq!(
            run_program(
                slice::from_ref(&mtree_5_12_13_no_hist),
                &HistoricMerkleTree_check_root!(f, false, 8, u8, old_root),
            ),
            Ok((vec![mtree_5_12_13_no_hist], vec![read!(false)]))
        );
    }

    #[test]
    fn test_context_self() {
        let run_program = run_program::<InMemoryDB, ResultModeGather>;
        let dummy_with_context = vec![
            vmval!(# [(AlignedValue::from(ContractAddress::default())), null, null]),
            vmval!(# [{}, {}, {}, {}, {}]),
            vmval!(null),
        ];
        assert_eq!(
            run_program(&dummy_with_context, &kernel_self!((), ()),),
            Ok((
                dummy_with_context.clone(),
                vec![read!(AlignedValue::from(ContractAddress::default()))]
            ))
        );
    }

    mod cached_idx {
        use super::*;
        use proptest::prelude::*;
        use std::collections::BTreeMap;
        use storage::storage::default_storage;

        type D = InMemoryDB;

        #[derive(Debug, Clone, Copy)]
        enum Container {
            Map,
            Bmt,
        }

        #[derive(Debug, Clone, Copy)]
        enum Relocation {
            None,
            FreshArray,
            FreshMap,
            UsedSlot,
            Aliased,
        }

        fn container(kind: Container, height: u8, leaves: &BTreeMap<u64, u64>) -> StateValue<D> {
            match kind {
                Container::Map => {
                    StateValue::Map(leaves.iter().fold(HashMap::new(), |m, (k, v)| {
                        m.insert(AlignedValue::from(*k), StateValue::from(*v))
                    }))
                }
                Container::Bmt => StateValue::BoundedMerkleTree(
                    leaves
                        .iter()
                        .fold(MerkleTree::blank(height), |t, (i, v)| {
                            t.try_update_hash(*i, leaf_hash(v), ()).unwrap()
                        })
                        .rehash(),
                ),
            }
        }

        fn expected_leaf(kind: Container, leaf: Option<&u64>) -> StateValue<D> {
            match (kind, leaf) {
                (_, None) => StateValue::Null,
                (Container::Map, Some(v)) => StateValue::from(*v),
                (Container::Bmt, Some(v)) => StateValue::Cell(Sp::new(leaf_hash(v).into())),
            }
        }

        // Round-trips through the arena so the container's nodes are only
        // loaded on demand, as they are for contract state read from disk.
        fn lazily_loaded(value: StateValue<D>) -> StateValue<D> {
            let mut sp = Sp::<_, D>::new(value);
            sp.persist();
            let key = sp.as_typed_key();
            drop(sp);
            let lazy = default_storage::<D>().arena.get_lazy(&key).unwrap();
            assert!(lazy.is_lazy());
            (*lazy).clone()
        }

        // Moves the value on top of the stack into another container and
        // takes it back out. `UsedSlot` moves it into slot 0 of the persistent
        // container `dest` stack entries down, after reading `key` from that
        // slot's previous occupant. `Aliased` moves it into that slot, keeps
        // the copy taken back out, then puts the previous occupant back after
        // reading `key` from it.
        fn relocate(
            relocation: Relocation,
            key: &AlignedValue,
            dest: u8,
        ) -> Vec<Op<ResultModeGather, D>> {
            let new_tag: u8 = match relocation {
                Relocation::None => return vec![],
                // Array of length 1.
                Relocation::FreshArray => 3 | (1 << 3),
                Relocation::FreshMap => 2,
                Relocation::UsedSlot => {
                    return ops![
                        dup dest;
                        idx [(AlignedValue::from(0u8)), (key.clone())];
                        pop;
                        dup dest;
                        push ((0u8));
                        dup 2;
                        ins 1;
                        idx [(AlignedValue::from(0u8))]
                    ];
                }
                Relocation::Aliased => {
                    return ops![
                        dup dest;
                        push ((0u8));
                        dup 2;
                        ins 1;
                        dup 0;
                        idx [(AlignedValue::from(0u8))];
                        dup (dest + 2);
                        idx [(AlignedValue::from(0u8))];
                        dup 0;
                        idx [(key.clone())];
                        pop;
                        dup 2;
                        push ((0u8));
                        dup 2;
                        ins 1;
                        pop;
                        pop
                    ];
                }
            };
            ops![
                push ((new_tag));
                new;
                push ((0u8));
                dup 2;
                ins 1;
                idx [(AlignedValue::from(0u8))]
            ]
        }

        fn read_case() -> impl Strategy<Value = (u8, BTreeMap<u64, u64>, u64)> {
            (1u8..=32)
                .prop_flat_map(|height| {
                    let index = 0..1u64 << height;
                    (
                        Just(height),
                        prop::collection::btree_map(index.clone(), any::<u64>(), 0..8),
                        index,
                    )
                })
                .prop_flat_map(|(height, leaves, fresh)| {
                    let keys: Vec<u64> = leaves.keys().copied().chain([fresh]).collect();
                    (Just(height), Just(leaves), prop::sample::select(keys))
                })
        }

        fn container_kind() -> impl Strategy<Value = Container> {
            prop_oneof![Just(Container::Map), Just(Container::Bmt)]
        }

        fn relocation() -> impl Strategy<Value = Relocation> {
            prop_oneof![
                Just(Relocation::None),
                Just(Relocation::FreshArray),
                Just(Relocation::FreshMap),
                Just(Relocation::UsedSlot),
                Just(Relocation::Aliased),
            ]
        }

        proptest! {
            // A cached read is priced as already paid for, so it must only
            // succeed on a leaf this run has already read, however the
            // container holding it was moved around beforehand.
            #[test]
            fn cached_leaf_read_requires_prior_read(
                kind in container_kind(),
                (height, leaves, key) in read_case(),
                wrappers in 0usize..=2,
                relocation in relocation(),
                relocate_at in 0usize..=2,
                warm: bool,
                lazy: bool,
            ) {
                let relocate_at = relocate_at.min(wrappers);
                let mut state = container(kind, height, &leaves);
                for _ in 0..wrappers {
                    state = StateValue::Array(vec![state].into());
                }
                let decoy = container(kind, height, &BTreeMap::from([(key, 0)]));
                let mut dest = StateValue::Array(vec![decoy].into());
                if lazy {
                    state = lazily_loaded(state);
                    dest = lazily_loaded(dest);
                }
                let key_av = AlignedValue::from(key);
                let mut program: Vec<Op<ResultModeGather, D>> = Vec::new();
                for level in 0..wrappers {
                    if level == relocate_at {
                        program.extend(relocate(relocation, &key_av, 1));
                    }
                    program.extend(ops![idx [(AlignedValue::from(0u8))]]);
                }
                if relocate_at == wrappers {
                    program.extend(relocate(relocation, &key_av, 1));
                }
                if warm {
                    program.extend(ops![dup 0; idx [(key_av.clone())]; pop]);
                }
                program.extend(ops![idxc [(key_av)]]);

                let result = run_program::<D, ResultModeGather>(
                    &[
                        VmValue::new(ValueStrength::Strong, dest),
                        VmValue::new(ValueStrength::Strong, state),
                    ],
                    &program,
                );
                let expected = expected_leaf(kind, leaves.get(&key));
                match (warm, relocation) {
                    (false, _) => prop_assert_eq!(result, Err(OnchainProgramError::CacheMiss)),
                    (true, Relocation::None) => {
                        let (stack, _) = result?;
                        prop_assert_eq!(&stack.last().unwrap().value, &expected);
                    }
                    // Whether a warmed read stays warm across a move is open;
                    // only a wrong value is a violation.
                    (true, _) => match result {
                        Ok((stack, _)) => prop_assert_eq!(&stack.last().unwrap().value, &expected),
                        Err(e) => prop_assert_eq!(e, OnchainProgramError::CacheMiss),
                    },
                }
            }
        }
    }

    // `ins` and `rem` replace a slot without reading it, so they must not mark
    // it as read: a copy of the container left on the stack still holds the
    // old value, and a cached `idx` on it would read that value for free.
    mod overwrite_does_not_mark_read {
        use super::*;

        type D = InMemoryDB;

        fn array(v: StateValue<D>) -> StateValue<D> {
            StateValue::Array(vec![v].into())
        }

        fn map(k: u64, v: u64) -> StateValue<D> {
            StateValue::Map(HashMap::new().insert(AlignedValue::from(k), StateValue::from(v)))
        }

        fn run(state: StateValue<D>, ops: &[Op<ResultModeGather, D>]) -> OnchainProgramError<D> {
            run_program::<D, ResultModeGather>(
                &[VmValue::new(ValueStrength::Strong, state)],
                ops,
            )
            .unwrap_err()
        }

        fn insc_then_read_copy() -> Vec<Op<ResultModeGather, D>> {
            ops![
                dup 0;
                push ((0u8));
                push ((7u64));
                insc 1;
                pop;
                idxc [(AlignedValue::from(0u8))]
            ]
        }

        #[test]
        fn cold_read_misses() {
            let ops = ops![idxc [(AlignedValue::from(0u8))]];
            assert_eq!(
                run(array(StateValue::from(42u64)), &ops),
                OnchainProgramError::CacheMiss
            );
        }

        #[test]
        fn cached_ins_into_array() {
            assert_eq!(
                run(array(StateValue::from(42u64)), &insc_then_read_copy()),
                OnchainProgramError::CacheMiss
            );
        }

        #[test]
        fn cached_ins_walk_through_nested_arrays() {
            let state = array(array(array(StateValue::from(42u64))));
            let ops: Vec<_> = (0..3).flat_map(|_| insc_then_read_copy()).collect();
            assert_eq!(run(state, &ops), OnchainProgramError::CacheMiss);
        }

        #[test]
        fn ins_into_map() {
            let ops = ops![
                dup 0;
                push ((1u64));
                push ((7u64));
                ins 1;
                pop;
                idxc [(AlignedValue::from(1u64))]
            ];
            assert_eq!(run(map(1, 42), &ops), OnchainProgramError::CacheMiss);
        }

        #[test]
        fn rem_from_map() {
            let ops = ops![
                dup 0;
                push ((1u64));
                rem;
                pop;
                idxc [(AlignedValue::from(1u64))]
            ];
            assert_eq!(run(map(1, 42), &ops), OnchainProgramError::CacheMiss);
        }
    }
}
