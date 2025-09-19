use base_crypto::hash::HashOutput;

use crate::FfiError;

// TokenType enum
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenType {
    Unshielded,
    Shielded,
    Dust,
}

impl From<coin_structure::coin::TokenType> for TokenType {
    fn from(tt: coin_structure::coin::TokenType) -> Self {
        match tt {
            coin_structure::coin::TokenType::Unshielded(_) => TokenType::Unshielded,
            coin_structure::coin::TokenType::Shielded(_) => TokenType::Shielded,
            coin_structure::coin::TokenType::Dust => TokenType::Dust,
        }
    }
}

impl From<TokenType> for coin_structure::coin::TokenType {
    fn from(tt: TokenType) -> Self {
        match tt {
            TokenType::Unshielded => coin_structure::coin::TokenType::Unshielded(
                coin_structure::coin::UnshieldedTokenType(HashOutput([0u8; 32]))
            ),
            TokenType::Shielded => coin_structure::coin::TokenType::Shielded(
                coin_structure::coin::ShieldedTokenType(HashOutput([0u8; 32]))
            ),
            TokenType::Dust => coin_structure::coin::TokenType::Dust,
        }
    }
}

// HashOutput wrapper for UniFFI
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct HashOutputWrapper {
    pub bytes: Vec<u8>,
}

impl From<HashOutput> for HashOutputWrapper {
    fn from(hash: HashOutput) -> Self {
        Self { bytes: hash.0.to_vec() }
    }
}

impl From<HashOutputWrapper> for HashOutput {
    fn from(wrapper: HashOutputWrapper) -> Self {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&wrapper.bytes[..32]);
        HashOutput(bytes)
    }
}

// ShieldedTokenType
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ShieldedTokenType {
    pub hash: HashOutputWrapper,
}

impl From<coin_structure::coin::ShieldedTokenType> for ShieldedTokenType {
    fn from(stt: coin_structure::coin::ShieldedTokenType) -> Self {
        Self { hash: stt.0.into() }
    }
}

impl From<ShieldedTokenType> for coin_structure::coin::ShieldedTokenType {
    fn from(stt: ShieldedTokenType) -> Self {
        Self(stt.hash.into())
    }
}

// UnshieldedTokenType
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct UnshieldedTokenType {
    pub hash: HashOutputWrapper,
}

impl From<coin_structure::coin::UnshieldedTokenType> for UnshieldedTokenType {
    fn from(utt: coin_structure::coin::UnshieldedTokenType) -> Self {
        Self { hash: utt.0.into() }
    }
}

impl From<UnshieldedTokenType> for coin_structure::coin::UnshieldedTokenType {
    fn from(utt: UnshieldedTokenType) -> Self {
        Self(utt.hash.into())
    }
}

// PublicKey
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct PublicKey {
    pub hash: HashOutputWrapper,
}

impl From<coin_structure::coin::PublicKey> for PublicKey {
    fn from(pk: coin_structure::coin::PublicKey) -> Self {
        Self { hash: pk.0.into() }
    }
}

impl From<PublicKey> for coin_structure::coin::PublicKey {
    fn from(pk: PublicKey) -> Self {
        Self(pk.hash.into())
    }
}

// UserAddress
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct UserAddress {
    pub hash: HashOutputWrapper,
}

impl From<coin_structure::coin::UserAddress> for UserAddress {
    fn from(ua: coin_structure::coin::UserAddress) -> Self {
        Self { hash: ua.0.into() }
    }
}

impl From<UserAddress> for coin_structure::coin::UserAddress {
    fn from(ua: UserAddress) -> Self {
        Self(ua.hash.into())
    }
}

// Commitment
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Commitment {
    pub hash: HashOutputWrapper,
}

impl From<coin_structure::coin::Commitment> for Commitment {
    fn from(c: coin_structure::coin::Commitment) -> Self {
        Self { hash: c.0.into() }
    }
}

impl From<Commitment> for coin_structure::coin::Commitment {
    fn from(c: Commitment) -> Self {
        Self(c.hash.into())
    }
}

// Nullifier
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Nullifier {
    pub hash: HashOutputWrapper,
}

impl From<coin_structure::coin::Nullifier> for Nullifier {
    fn from(n: coin_structure::coin::Nullifier) -> Self {
        Self { hash: n.0.into() }
    }
}

impl From<Nullifier> for coin_structure::coin::Nullifier {
    fn from(n: Nullifier) -> Self {
        Self(n.hash.into())
    }
}

// Nonce
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Nonce {
    pub hash: HashOutputWrapper,
}

impl From<coin_structure::coin::Nonce> for Nonce {
    fn from(n: coin_structure::coin::Nonce) -> Self {
        Self { hash: n.0.into() }
    }
}

impl From<Nonce> for coin_structure::coin::Nonce {
    fn from(n: Nonce) -> Self {
        Self(n.hash.into())
    }
}

// ShieldedCoinInfo
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ShieldedCoinInfo {
    pub nonce: Nonce,
    pub token_type: ShieldedTokenType,
    pub value: i64, // Using i64 instead of u128 for UniFFI compatibility
}

impl From<coin_structure::coin::Info> for ShieldedCoinInfo {
    fn from(info: coin_structure::coin::Info) -> Self {
        Self {
            nonce: info.nonce.into(),
            token_type: info.type_.into(),
            value: info.value as i64, // Convert u128 to i64
        }
    }
}

impl From<ShieldedCoinInfo> for coin_structure::coin::Info {
    fn from(info: ShieldedCoinInfo) -> Self {
        Self {
            nonce: info.nonce.into(),
            type_: info.token_type.into(),
            value: info.value as u128, // Convert i64 to u128
        }
    }
}

// Helper functions for serialization/deserialization
#[uniffi::export]
pub fn shielded_token_type_from_bytes(bytes: Vec<u8>) -> Result<ShieldedTokenType, FfiError> {
    if bytes.len() != 32 {
        return Err(FfiError::InvalidInput { 
            details: format!("Expected 32 bytes, got {}", bytes.len()) 
        });
    }
    let mut hash_bytes = [0u8; 32];
    hash_bytes.copy_from_slice(&bytes);
    Ok(ShieldedTokenType { 
        hash: HashOutputWrapper { bytes: hash_bytes.to_vec() } 
    })
}

#[uniffi::export]
pub fn unshielded_token_type_from_bytes(bytes: Vec<u8>) -> Result<UnshieldedTokenType, FfiError> {
    if bytes.len() != 32 {
        return Err(FfiError::InvalidInput { 
            details: format!("Expected 32 bytes, got {}", bytes.len()) 
        });
    }
    let mut hash_bytes = [0u8; 32];
    hash_bytes.copy_from_slice(&bytes);
    Ok(UnshieldedTokenType { 
        hash: HashOutputWrapper { bytes: hash_bytes.to_vec() } 
    })
}

#[uniffi::export]
pub fn public_key_from_bytes(bytes: Vec<u8>) -> Result<PublicKey, FfiError> {
    if bytes.len() != 32 {
        return Err(FfiError::InvalidInput { 
            details: format!("Expected 32 bytes, got {}", bytes.len()) 
        });
    }
    let mut hash_bytes = [0u8; 32];
    hash_bytes.copy_from_slice(&bytes);
    Ok(PublicKey { 
        hash: HashOutputWrapper { bytes: hash_bytes.to_vec() } 
    })
}

#[uniffi::export]
pub fn user_address_from_bytes(bytes: Vec<u8>) -> Result<UserAddress, FfiError> {
    if bytes.len() != 32 {
        return Err(FfiError::InvalidInput { 
            details: format!("Expected 32 bytes, got {}", bytes.len()) 
        });
    }
    let mut hash_bytes = [0u8; 32];
    hash_bytes.copy_from_slice(&bytes);
    Ok(UserAddress { 
        hash: HashOutputWrapper { bytes: hash_bytes.to_vec() } 
    })
}

#[uniffi::export]
pub fn commitment_from_bytes(bytes: Vec<u8>) -> Result<Commitment, FfiError> {
    if bytes.len() != 32 {
        return Err(FfiError::InvalidInput { 
            details: format!("Expected 32 bytes, got {}", bytes.len()) 
        });
    }
    let mut hash_bytes = [0u8; 32];
    hash_bytes.copy_from_slice(&bytes);
    Ok(Commitment { 
        hash: HashOutputWrapper { bytes: hash_bytes.to_vec() } 
    })
}

#[uniffi::export]
pub fn nullifier_from_bytes(bytes: Vec<u8>) -> Result<Nullifier, FfiError> {
    if bytes.len() != 32 {
        return Err(FfiError::InvalidInput { 
            details: format!("Expected 32 bytes, got {}", bytes.len()) 
        });
    }
    let mut hash_bytes = [0u8; 32];
    hash_bytes.copy_from_slice(&bytes);
    Ok(Nullifier { 
        hash: HashOutputWrapper { bytes: hash_bytes.to_vec() } 
    })
}

#[uniffi::export]
pub fn nonce_from_bytes(bytes: Vec<u8>) -> Result<Nonce, FfiError> {
    if bytes.len() != 32 {
        return Err(FfiError::InvalidInput { 
            details: format!("Expected 32 bytes, got {}", bytes.len()) 
        });
    }
    let mut hash_bytes = [0u8; 32];
    hash_bytes.copy_from_slice(&bytes);
    Ok(Nonce { 
        hash: HashOutputWrapper { bytes: hash_bytes.to_vec() } 
    })
}

// Conversion functions to get bytes
impl ShieldedTokenType {
    pub fn to_bytes(&self) -> Vec<u8> {
        self.hash.bytes.clone()
    }
}

impl UnshieldedTokenType {
    pub fn to_bytes(&self) -> Vec<u8> {
        self.hash.bytes.clone()
    }
}

impl PublicKey {
    pub fn to_bytes(&self) -> Vec<u8> {
        self.hash.bytes.clone()
    }
}

impl UserAddress {
    pub fn to_bytes(&self) -> Vec<u8> {
        self.hash.bytes.clone()
    }
}

impl Commitment {
    pub fn to_bytes(&self) -> Vec<u8> {
        self.hash.bytes.clone()
    }
}

impl Nullifier {
    pub fn to_bytes(&self) -> Vec<u8> {
        self.hash.bytes.clone()
    }
}

impl Nonce {
    pub fn to_bytes(&self) -> Vec<u8> {
        self.hash.bytes.clone()
    }
}
