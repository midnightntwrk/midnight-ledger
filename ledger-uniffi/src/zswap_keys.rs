use transient_crypto::encryption::SecretKey;
use ledger::dust::DustSecretKey;

#[derive(Clone)]
pub struct CoinSecretKey(pub SecretKey);

impl CoinSecretKey {
    pub fn new(secret_key: SecretKey) -> Self {
        CoinSecretKey(secret_key)
    }

    pub fn inner(&self) -> &SecretKey {
        &self.0
    }
}

#[derive(Clone)]
pub struct DustSecretKeyWrapper(pub DustSecretKey);

impl DustSecretKeyWrapper {
    pub fn new(secret_key: DustSecretKey) -> Self {
        DustSecretKeyWrapper(secret_key)
    }

    pub fn inner(&self) -> &DustSecretKey {
        &self.0
    }
}
