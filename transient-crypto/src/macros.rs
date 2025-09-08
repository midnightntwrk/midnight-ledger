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

macro_rules! wrap_field_arith {
    ($wrapper_name:ident) => {
        impl std::ops::Add for $wrapper_name {
            type Output = $wrapper_name;

            fn add(self, other: $wrapper_name) -> $wrapper_name {
                $wrapper_name(self.0 + other.0)
            }
        }

        impl std::ops::Sub for $wrapper_name {
            type Output = $wrapper_name;

            fn sub(self, other: $wrapper_name) -> $wrapper_name {
                $wrapper_name(self.0 - other.0)
            }
        }

        impl std::ops::Mul for $wrapper_name {
            type Output = $wrapper_name;

            fn mul(self, other: $wrapper_name) -> $wrapper_name {
                $wrapper_name(self.0 * other.0)
            }
        }

        impl std::ops::Neg for $wrapper_name {
            type Output = $wrapper_name;

            fn neg(self) -> $wrapper_name {
                $wrapper_name(-self.0)
            }
        }

        impl std::ops::Div for $wrapper_name {
            type Output = $wrapper_name;

            fn div(self, rhs: $wrapper_name) -> $wrapper_name {
                self.mul($wrapper_name(rhs.0.invert().unwrap()))
            }
        }
    };
}

macro_rules! wrap_group_arith {
    ($wrapper_name:ident, $scalar_wrapper:ident) => {
        impl std::ops::Add for $wrapper_name {
            type Output = $wrapper_name;

            fn add(self, other: $wrapper_name) -> $wrapper_name {
                $wrapper_name((self.0 + other.0).into())
            }
        }

        impl std::ops::Sub for $wrapper_name {
            type Output = $wrapper_name;

            fn sub(self, other: $wrapper_name) -> $wrapper_name {
                $wrapper_name((self.0 - other.0).into())
            }
        }

        impl std::ops::Mul<$scalar_wrapper> for $wrapper_name {
            type Output = $wrapper_name;

            fn mul(self, other: $scalar_wrapper) -> $wrapper_name {
                $wrapper_name(self.0.mul(other.0).into())
            }
        }

        impl std::ops::Neg for $wrapper_name {
            type Output = $wrapper_name;

            fn neg(self) -> $wrapper_name {
                $wrapper_name(-self.0)
            }
        }
    };
}

macro_rules! fr_display {
    ($wrapper_name:ident) => {
        impl std::fmt::Display for $wrapper_name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                let mut repr = self.as_le_bytes();
                while let Some(0) = repr.last() {
                    repr.pop();
                }
                if repr.is_empty() {
                    write!(formatter, "-")?;
                } else {
                    for byte in repr {
                        write!(formatter, "{byte:02x}")?;
                    }
                }
                Ok(())
            }
        }

        impl std::fmt::Debug for $wrapper_name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                std::fmt::Display::fmt(&self, formatter)
            }
        }
    };
}

macro_rules! wrap_display {
    ($wrapper_name:ident) => {
        impl std::fmt::Display for $wrapper_name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                std::fmt::Debug::fmt(&self.0, formatter)
            }
        }
        impl std::fmt::Debug for $wrapper_name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                std::fmt::Debug::fmt(&self.0, formatter)
            }
        }
    };
}

pub(crate) use {fr_display, wrap_display, wrap_field_arith, wrap_group_arith};
