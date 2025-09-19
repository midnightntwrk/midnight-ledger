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
