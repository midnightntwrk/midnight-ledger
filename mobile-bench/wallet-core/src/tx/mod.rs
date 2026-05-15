//! Transaction build + balance + prove + submit pipeline for
//! DID deploys.
//!
//!   `build`   → compose an unproven `Transaction::Standard`
//!   `balance` → cover DUST fees from a `DustLocalState`
//!   `prove`   → wrap `ledger::prove::tx_prove`
//!   `scale`   → `Transaction → Vec<u8>` for send_mn_transaction
//!
//! Public API is the `WizardStage` stream emitted by
//! `Wallet::create_did()` (Task 11).

pub(crate) mod build;
pub(crate) mod scale;

use crate::DidId;

#[allow(dead_code)] // Wired by Wallet::create_did in Task 11.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeployOutcome {
    pub did_id: DidId,
    pub tx_hash: [u8; 32],
    pub block_hash: [u8; 32],
}

#[allow(dead_code)] // Wired by Wallet::create_did in Task 11.
#[derive(Debug, Clone)]
pub enum WizardStage {
    SyncingDust,
    Composing,
    Balancing,
    Proving,
    Submitting,
    Confirming,
    Done(DeployOutcome),
    Failed(String),
}

#[allow(dead_code)] // Wired by Wallet::create_did in Task 11.
#[derive(Debug, thiserror::Error)]
pub enum TxError {
    #[error("dust sync: {0}")]
    Dust(#[from] crate::DustError),
    #[error("compose: {0}")]
    Compose(String),
    #[error("balance: {0}")]
    Balance(String),
    #[error("prove: {0}")]
    Prove(String),
    #[error("scale encode: {0}")]
    ScaleEncode(String),
    #[error("submit: {0}")]
    Submit(String),
}
