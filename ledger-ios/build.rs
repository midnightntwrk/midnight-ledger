// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

fn main() {
    uniffi::generate_scaffolding("src/ledger_ios.udl").unwrap();
}
