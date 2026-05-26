//! Placeholder for the CRUD API.
//!
//! Real implementation lands in Tasks 6-8. This stub exists so the
//! `mod.rs` re-export resolves and the crate compiles after the
//! Task 5 scaffold.

use redb::Database;

#[allow(dead_code)] // populated in Task 7
pub struct VcStore {
    db: std::sync::Arc<Database>,
}
