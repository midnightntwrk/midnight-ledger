use redb::TableDefinition;

/// `vc_uri` (UTF-8) → CBOR-encoded `StoredVc`.
#[allow(dead_code)] // consumed by the CRUD API in Tasks 6-8
pub(super) const VCS: TableDefinition<&str, Vec<u8>> = TableDefinition::new("identity_vcs_v1");

/// Composite key `(vc_uri, claim_path)` (UTF-8 + 0x1f + UTF-8) → CBOR `VcOpening`.
#[allow(dead_code)] // consumed by the CRUD API in Tasks 6-8
pub(super) const VC_OPENINGS: TableDefinition<&str, Vec<u8>> =
    TableDefinition::new("identity_vc_openings_v1");

/// `vc_uri` (UTF-8) → CBOR-encoded `VcMetadata`.
#[allow(dead_code)] // consumed by the CRUD API in Tasks 6-8
pub(super) const VC_METADATA: TableDefinition<&str, Vec<u8>> =
    TableDefinition::new("identity_vc_metadata_v1");

/// Build the composite key for `VC_OPENINGS`. `0x1f` is the ASCII
/// "Unit Separator" — never appears in URIs or JSON pointers in
/// practice, so it's safe as a delimiter.
#[allow(dead_code)] // consumed by the CRUD API in Task 8
pub(super) fn opening_key(vc_uri: &str, claim_path: &str) -> String {
    format!("{vc_uri}\x1f{claim_path}")
}
