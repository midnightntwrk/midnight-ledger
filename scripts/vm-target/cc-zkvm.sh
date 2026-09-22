#!/usr/bin/env bash

# This file is part of midnight-ledger.
# Copyright (C) Midnight Foundation
# SPDX-License-Identifier: Apache-2.0
# Licensed under the Apache License, Version 2.0 (the "License");
# You may not use this file except in compliance with the License.
# You may obtain a copy of the License at
# http://www.apache.org/licenses/LICENSE-2.0
# Unless required by applicable law or agreed to in writing, software
# distributed under the License is distributed on an "AS IS" BASIS,
# WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
# See the License for the specific language governing permissions and
# limitations under the License.

# C compiler wrapper for the `riscv64gc-unknown-zkvm-elf` probe target
# (scripts/vm-target/riscv64gc-unknown-zkvm-elf.json). Only `blst` has C
# sources in the verification-only ledger graph.
#
# `cc-rs` derives the clang triple from the target JSON's file name, which
# clang rejects. Rewrite it to a bare-metal RV64GC triple, freestanding, and
# reuse the tiny freestanding sysroot (string.h/stdlib.h stubs) that
# `secp256k1-sys` ships for its wasm build.
#
# Use via:  CC_riscv64gc_unknown_zkvm_elf=$PWD/scripts/vm-target/cc-zkvm.sh
#           AR_riscv64gc_unknown_zkvm_elf=$(brew --prefix llvm)/bin/llvm-ar
# Apple's clang has no RISC-V backend; point CLANG at an LLVM clang.
set -euo pipefail

CLANG="${CLANG:-/opt/homebrew/opt/llvm/bin/clang}"
SYSROOT="${ZKVM_SYSROOT:-$(ls -d "$HOME"/.cargo/registry/src/*/secp256k1-sys-0.10.*/wasm/wasm-sysroot 2>/dev/null | head -1)}"

args=()
for a in "$@"; do
  case "$a" in
    --target=*) ;;
    -march=*|-mabi=*) ;;
    *) args+=("$a") ;;
  esac
done

extra=(--target=riscv64-unknown-none-elf -march=rv64gc -mabi=lp64d -ffreestanding -fPIE -fno-exceptions -fno-stack-protector -nostdlib)
[[ -n "$SYSROOT" ]] && extra+=(-isystem "$SYSROOT")

exec "$CLANG" "${extra[@]}" "${args[@]}"
