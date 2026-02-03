// This file is part of midnight-ledger.
// Copyright (C) 2025 Midnight Foundation
// SPDX-License-Identifier: Apache-2.0

//! Address types for iOS bindings.

use crate::error::LedgerError;
use crate::util::{from_hex, from_hex_ser, to_hex_ser};
use base_crypto::hash::HashOutput;
use coin_structure::coin::{
    PublicAddress as LedgerPublicAddress, UserAddress as LedgerUserAddress,
};
use coin_structure::contract::ContractAddress as LedgerContractAddress;
use serialize::{tagged_deserialize, tagged_serialize};
use std::sync::Arc;

/// A contract address (32-byte hash).
pub struct ContractAddress {
    pub(crate) inner: LedgerContractAddress,
}

impl ContractAddress {
    /// Creates a contract address from a hex-encoded string (32 bytes = 64 hex chars).
    pub fn from_hex(hex: String) -> Result<Self, LedgerError> {
        let bytes = from_hex(&hex)?;
        if bytes.len() != 32 {
            return Err(LedgerError::InvalidData);
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(ContractAddress {
            inner: LedgerContractAddress(HashOutput(arr)),
        })
    }

    /// Returns the address as a hex-encoded string.
    pub fn to_hex(&self) -> String {
        to_hex_ser(&self.inner)
    }

    /// Creates a custom shielded token type for this contract.
    /// The domain_sep is a 32-byte domain separator (hex-encoded).
    pub fn custom_shielded_token(&self, domain_sep: String) -> Result<String, LedgerError> {
        let bytes = from_hex(&domain_sep)?;
        if bytes.len() != 32 {
            return Err(LedgerError::InvalidData);
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        let token = self.inner.custom_shielded_token_type(HashOutput(arr));
        Ok(to_hex_ser(&token))
    }

    /// Creates a custom unshielded token type for this contract.
    /// The domain_sep is a 32-byte domain separator (hex-encoded).
    pub fn custom_unshielded_token(&self, domain_sep: String) -> Result<String, LedgerError> {
        let bytes = from_hex(&domain_sep)?;
        if bytes.len() != 32 {
            return Err(LedgerError::InvalidData);
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        let token = self.inner.custom_unshielded_token_type(HashOutput(arr));
        Ok(to_hex_ser(&token))
    }

    /// Serializes the address to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut buf = Vec::new();
        tagged_serialize(&self.inner, &mut buf)?;
        Ok(buf)
    }

    /// Deserializes a contract address from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerContractAddress = tagged_deserialize(&mut &raw[..])?;
        Ok(ContractAddress { inner })
    }
}

/// A public address that can be either a contract or a user.
pub struct PublicAddress {
    pub(crate) inner: LedgerPublicAddress,
}

impl PublicAddress {
    /// Creates a public address for a contract.
    pub fn from_contract(contract: Arc<ContractAddress>) -> Self {
        PublicAddress {
            inner: LedgerPublicAddress::Contract(contract.inner),
        }
    }

    /// Creates a public address for a user from a hex-encoded user address.
    pub fn from_user(user_address: String) -> Result<Self, LedgerError> {
        let addr: LedgerUserAddress = from_hex_ser(&user_address)?;
        Ok(PublicAddress {
            inner: LedgerPublicAddress::User(addr),
        })
    }

    /// Returns true if this is a contract address.
    pub fn is_contract(&self) -> bool {
        matches!(self.inner, LedgerPublicAddress::Contract(_))
    }

    /// Returns true if this is a user address.
    pub fn is_user(&self) -> bool {
        matches!(self.inner, LedgerPublicAddress::User(_))
    }

    /// Returns the contract address if this is a contract address.
    pub fn contract_address(&self) -> Option<Arc<ContractAddress>> {
        match &self.inner {
            LedgerPublicAddress::Contract(addr) => Some(Arc::new(ContractAddress { inner: *addr })),
            LedgerPublicAddress::User(_) => None,
        }
    }

    /// Returns the user address as a hex string if this is a user address.
    pub fn user_address(&self) -> Option<String> {
        match &self.inner {
            LedgerPublicAddress::User(addr) => Some(to_hex_ser(addr)),
            LedgerPublicAddress::Contract(_) => None,
        }
    }

    /// Returns the address as a hex-encoded string.
    pub fn to_hex(&self) -> String {
        to_hex_ser(&self.inner)
    }

    /// Serializes the address to bytes.
    pub fn serialize(&self) -> Result<Vec<u8>, LedgerError> {
        let mut buf = Vec::new();
        tagged_serialize(&self.inner, &mut buf)?;
        Ok(buf)
    }

    /// Deserializes a public address from bytes.
    pub fn deserialize(raw: Vec<u8>) -> Result<Self, LedgerError> {
        let inner: LedgerPublicAddress = tagged_deserialize(&mut &raw[..])?;
        Ok(PublicAddress { inner })
    }
}
