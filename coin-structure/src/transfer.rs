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

use crate::coin::{PublicKey, SecretKey};
use crate::contract::ContractAddress;
use base_crypto::hash::{BLANK_HASH, HashOutput};
use base_crypto::repr::MemWrite;
use fake::Dummy;
#[cfg(feature = "proptest")]
use proptest_derive::Arbitrary;
#[cfg(feature = "proptest")]
use serialize::randomised_serialization_test;
use serialize::{self, Deserializable, Serializable, Tagged, tag_enforcement_test};
use transient_crypto::curve::Fr;
use transient_crypto::repr::{FieldRepr, FromFieldRepr};

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Dummy, Serializable)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
#[tag = "recipient[v1]"]
pub enum Recipient {
    User(PublicKey),
    Contract(ContractAddress),
}
tag_enforcement_test!(Recipient);

#[cfg(feature = "proptest")]
randomised_serialization_test!(Recipient);

impl FieldRepr for Recipient {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        match self {
            Recipient::User(pk) => {
                true.field_repr(writer);
                pk.0.field_repr(writer);
                BLANK_HASH.field_repr(writer);
            }
            Recipient::Contract(addr) => {
                false.field_repr(writer);
                BLANK_HASH.field_repr(writer);
                addr.0.field_repr(writer);
            }
        }
    }
    fn field_size(&self) -> usize {
        <HashOutput as FromFieldRepr>::FIELD_SIZE * 2 + <bool as FromFieldRepr>::FIELD_SIZE
    }
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, PartialOrd, Ord, Serializable, Dummy)]
#[cfg_attr(feature = "proptest", derive(Arbitrary))]
pub enum SenderEvidence {
    User(SecretKey),
    Contract(ContractAddress),
}

impl FieldRepr for SenderEvidence {
    fn field_repr<W: MemWrite<Fr>>(&self, writer: &mut W) {
        match self {
            SenderEvidence::User(sk) => {
                true.field_repr(writer);
                sk.0.field_repr(writer);
                BLANK_HASH.field_repr(writer);
            }
            SenderEvidence::Contract(addr) => {
                false.field_repr(writer);
                BLANK_HASH.field_repr(writer);
                addr.0.field_repr(writer);
            }
        }
    }
    fn field_size(&self) -> usize {
        <HashOutput as FromFieldRepr>::FIELD_SIZE * 2 + <bool as FromFieldRepr>::FIELD_SIZE
    }
}

impl From<&SenderEvidence> for Recipient {
    fn from(se: &SenderEvidence) -> Recipient {
        use SenderEvidence::*;
        match se {
            User(sk) => Recipient::User(sk.public_key()),
            Contract(addr) => Recipient::Contract(*addr),
        }
    }
}
