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

use crate::coin::UserAddress;
use crate::coin::{
    Commitment, Info, Nonce, Nullifier, PublicKey, QualifiedInfo, ShieldedTokenType,
    UnshieldedTokenType,
};
use crate::contract::ContractAddress;
use crate::transfer::{Recipient, SenderEvidence};
use base_crypto::fab::{Aligned, Alignment, InvalidBuiltinDecode, Value, ValueAtom, ValueSlice};
use base_crypto::hash::{HashOutput, PERSISTENT_HASH_BYTES as PHB};

macro_rules! via_hash_output {
    ($($ty:ident),*) => {
        $(
            impl Aligned for $ty {
                fn alignment() -> Alignment {
                    HashOutput::alignment()
                }
            }

            impl From<$ty> for ValueAtom {
                fn from(hash: $ty) -> ValueAtom {
                    ValueAtom(hash.0.0.to_vec()).normalize()
                }
            }

            impl TryFrom<&ValueAtom> for $ty {
                type Error = InvalidBuiltinDecode;

                fn try_from(value: &ValueAtom) -> Result<$ty, InvalidBuiltinDecode> {
                    let mut buf = [0u8; PHB];
                    if value.0.len() <= PHB {
                        buf[..value.0.len()].copy_from_slice(&value.0[..]);
                        Ok($ty(HashOutput(buf)))
                    } else {
                        Err(InvalidBuiltinDecode(stringify!($ty)))
                    }
                }
            }

            impl From<$ty> for Value {
                fn from(val: $ty) -> Value {
                    Value(vec![val.into()])
                }
            }

            impl TryFrom<&ValueSlice> for $ty {
                type Error = InvalidBuiltinDecode;

                fn try_from(value: &ValueSlice) -> Result<$ty, InvalidBuiltinDecode> {
                    if value.0.len() == 1 {
                        Ok(<$ty>::try_from(&value.0[0])?)
                    } else {
                        Err(InvalidBuiltinDecode(stringify!($ty)))
                    }
                }
            }
        )*
    }
}

via_hash_output!(
    ContractAddress,
    UserAddress,
    Nonce,
    Nullifier,
    Commitment,
    PublicKey,
    UnshieldedTokenType,
    ShieldedTokenType
);

impl Aligned for Info {
    fn alignment() -> Alignment {
        Alignment::concat([
            &Nonce::alignment(),
            &ShieldedTokenType::alignment(),
            &u128::alignment(),
        ])
    }
}

impl Aligned for QualifiedInfo {
    fn alignment() -> Alignment {
        Alignment::concat([
            &Nonce::alignment(),
            &ShieldedTokenType::alignment(),
            &u128::alignment(),
            &u64::alignment(),
        ])
    }
}

impl Aligned for Recipient {
    fn alignment() -> Alignment {
        Alignment::concat([
            &bool::alignment(),
            &<[u8; 32]>::alignment(),
            &<[u8; 32]>::alignment(),
        ])
    }
}

impl From<Info> for Value {
    fn from(info: Info) -> Value {
        Value(vec![
            info.nonce.into(),
            info.type_.into(),
            info.value.into(),
        ])
    }
}

impl TryFrom<&ValueSlice> for Info {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueSlice) -> Result<Info, InvalidBuiltinDecode> {
        if value.0.len() == 3 {
            Ok(Info {
                nonce: (&value.0[0]).try_into()?,
                type_: (&value.0[1]).try_into()?,
                value: (&value.0[2]).try_into()?,
            })
        } else {
            Err(InvalidBuiltinDecode("CoinInfo"))
        }
    }
}

impl From<QualifiedInfo> for Value {
    fn from(info: QualifiedInfo) -> Value {
        Value(vec![
            info.nonce.into(),
            info.type_.into(),
            info.value.into(),
            info.mt_index.into(),
        ])
    }
}

impl TryFrom<&ValueSlice> for QualifiedInfo {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueSlice) -> Result<QualifiedInfo, InvalidBuiltinDecode> {
        if value.0.len() == 4 {
            Ok(QualifiedInfo {
                nonce: (&value.0[0]).try_into()?,
                type_: (&value.0[1]).try_into()?,
                value: (&value.0[2]).try_into()?,
                mt_index: (&value.0[3]).try_into()?,
            })
        } else {
            Err(InvalidBuiltinDecode("QualifiedCoinInfo"))
        }
    }
}

impl From<Recipient> for Value {
    fn from(recipient: Recipient) -> Value {
        Value(match recipient {
            Recipient::User(pk) => vec![true.into(), pk.into(), ().into()],
            Recipient::Contract(contract) => vec![false.into(), ().into(), contract.into()],
        })
    }
}

impl TryFrom<&ValueSlice> for Recipient {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueSlice) -> Result<Recipient, InvalidBuiltinDecode> {
        if value.0.len() == 3 {
            let is_left: bool = (&value.0[0]).try_into()?;
            if is_left {
                <()>::try_from(&value.0[2])?;
                Ok(Recipient::User((&value.0[1]).try_into()?))
            } else {
                <()>::try_from(&value.0[1])?;
                Ok(Recipient::Contract((&value.0[2]).try_into()?))
            }
        } else {
            Err(InvalidBuiltinDecode("Recipient"))
        }
    }
}

impl TryFrom<&ValueSlice> for SenderEvidence {
    type Error = InvalidBuiltinDecode;

    fn try_from(value: &ValueSlice) -> Result<SenderEvidence, InvalidBuiltinDecode> {
        if value.0.len() == 3 {
            let is_left: bool = (&value.0[0]).try_into()?;
            if is_left {
                <()>::try_from(&value.0[2])?;
                Ok(SenderEvidence::User((&value.0[1]).try_into()?))
            } else {
                <()>::try_from(&value.0[1])?;
                Ok(SenderEvidence::Contract((&value.0[2]).try_into()?))
            }
        } else {
            Err(InvalidBuiltinDecode("SenderEvidence"))
        }
    }
}
