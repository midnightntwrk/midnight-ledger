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
// limitations under the License

// Basic tx module structure for future implementation
// TODO: Implement transaction types when ledger API is finalized

#[derive(Clone)]
pub enum TransactionTypes {
    // Placeholder for future transaction types
    Placeholder,
}

#[derive(Clone)]
pub struct Transaction(pub TransactionTypes);

impl Transaction {
    pub fn new(transaction_type: TransactionTypes) -> Self {
        Transaction(transaction_type)
    }

    pub fn inner(&self) -> &TransactionTypes {
        &self.0
    }
}

// TODO: Implement From implementations when transaction types are available
