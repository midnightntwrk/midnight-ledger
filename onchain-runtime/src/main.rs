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

use onchain_runtime_state::state::StateValue;
use onchain_vm::cost_model::INITIAL_COST_MODEL;
use onchain_vm::ops::Op;
use onchain_vm::result_mode::ResultModeGather;
use onchain_vm::vm::run_program;
use onchain_vm::vm_value::{ValueStrength, VmValue};
use serialize::{Deserializable, Serializable};
use std::fs::File;
use std::{
    env::args,
    io::{BufRead, BufReader},
    process::exit,
};
use storage::DefaultDB;

fn main() {
    let args = args().collect::<Vec<_>>();
    if args.len() != 3 && args.len() != 4 {
        println!("Usage: {} <STATE-IN> <PROGRAM> [<STATE-OUT>]", args[0]);
    } else {
        let state_in: StateValue<DefaultDB> =
            <StateValue<DefaultDB> as Deserializable>::deserialize(
                &mut File::open(&args[1]).unwrap(),
                0,
            )
            .unwrap();
        println!("Input state: {:#?}", &state_in);
        let prog_in: Vec<Op<ResultModeGather, DefaultDB>> = {
            let mut prog = Vec::new();
            let mut file = BufReader::new(File::open(&args[2]).unwrap());
            while !file.fill_buf().unwrap().is_empty() {
                prog.push(Deserializable::deserialize(&mut file, 0).unwrap());
            }
            prog
        };
        println!("Input program: {:#?}", &prog_in);

        match run_program(
            &[VmValue::new(ValueStrength::Strong, state_in)],
            &prog_in[..],
            None,
            &INITIAL_COST_MODEL,
        ) {
            Ok(res) => {
                println!("Result stack: {:#?}", &res.stack);
                println!("Events output: {:#?}", &res.events);
                println!("Gas cost: {:#?}", &res.gas_cost);

                if args.len() == 4 {
                    Serializable::serialize(
                        &res.stack[0].value,
                        &mut File::create(&args[3]).unwrap(),
                    )
                    .unwrap();
                }
            }
            Err(err) => {
                println!("Program failed with error: {err:?}");
                exit(1);
            }
        }
    }
}
