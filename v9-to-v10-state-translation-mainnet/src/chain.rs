// This file is part of midnight-ledger.
// Copyright (C) 2026 Midnight Foundation
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

//! Reads a storage value at the finalized block straight from a Substrate
//! node's chain database, so no node needs to run.

use anyhow::{Context, Result, anyhow, bail, ensure};
use std::path::Path;

// `sc-client-db`'s columns.
const COL_META: u8 = 0;
const COL_STATE: u8 = 1;
const COL_HEADER: u8 = 4;

/// `twox128("Midnight") ++ twox128("StateKey")`.
pub const STATE_KEY_STORAGE_KEY: [u8; 32] = [
    0x2a, 0x76, 0x0f, 0x9a, 0x17, 0x3a, 0x6d, 0xf5, 0xcd, 0x43, 0x73, 0xff, 0x49, 0xfa, 0x99, 0x9b,
    0xd3, 0xb3, 0x81, 0x2d, 0x84, 0xb9, 0x65, 0xbe, 0x98, 0xec, 0x47, 0x7e, 0x35, 0x75, 0x37, 0x9b,
];

pub struct Finalized {
    pub number: u32,
    pub hash: [u8; 32],
    /// The storage value, SCALE-encoded as the runtime stored it.
    pub value: Vec<u8>,
}

/// Looks up `key` in the state of the finalized block. The database is opened
/// read-only, which still needs the node stopped.
pub fn read_finalized(chain_db: &Path, key: &[u8]) -> Result<Finalized> {
    let metadata = parity_db::Options::load_metadata(chain_db)
        .map_err(|e| anyhow!("reading {} metadata: {e}", chain_db.display()))?
        .with_context(|| format!("{} is not a parity-db database", chain_db.display()))?;
    let mut options = parity_db::Options::with_columns(chain_db, metadata.columns.len() as u8);
    options.columns = metadata.columns;
    options.salt = Some(metadata.salt);
    let db = parity_db::Db::open_read_only(&options)
        .map_err(|e| anyhow!("opening {} read-only: {e}", chain_db.display()))?;
    let get = |col, key: &[u8]| {
        db.get(col, key)
            .map_err(|e| anyhow!("reading column {col}: {e}"))
    };

    // The lookup key: the block number, big-endian, then the hash.
    let lookup = get(COL_META, b"final")?.context("no finalized block")?;
    ensure!(
        lookup.len() == 36,
        "finalized lookup key of {} bytes",
        lookup.len()
    );
    let number = u32::from_be_bytes(lookup[..4].try_into()?);
    let hash: [u8; 32] = lookup[4..].try_into()?;
    let header = get(COL_HEADER, &lookup)?.context("finalized header missing")?;
    let mut rest = header.get(32..).context("truncated header")?;
    let header_number = compact(&mut rest)?;
    ensure!(
        header_number == u64::from(number),
        "header number {header_number}, lookup {number}"
    );
    let state_root: [u8; 32] = rest.get(..32).context("truncated header")?.try_into()?;

    let fetch = |hash: &[u8]| {
        get(COL_STATE, hash)?.with_context(|| format!("trie node {} missing", hex::encode(hash)))
    };
    let value = trie_lookup(&fetch, fetch(&state_root)?, &nibbles(key))?
        .with_context(|| format!("{} not set at block {number}", hex::encode(key)))?;
    Ok(Finalized {
        number,
        hash,
        value,
    })
}

/// Reads a SCALE `Compact` integer.
pub fn compact(input: &mut &[u8]) -> Result<u64> {
    let first = *input.first().context("truncated compact")?;
    let (len, value) = match first & 3 {
        0 => (1, u64::from(first >> 2)),
        1 => (
            2,
            u64::from(
                u16::from_le_bytes(input.get(..2).context("truncated compact")?.try_into()?) >> 2,
            ),
        ),
        2 => (
            4,
            u64::from(
                u32::from_le_bytes(input.get(..4).context("truncated compact")?.try_into()?) >> 2,
            ),
        ),
        _ => {
            let n = usize::from(first >> 2) + 4;
            ensure!(n <= 8, "compact wider than u64");
            let mut bytes = [0; 8];
            bytes[..n].copy_from_slice(input.get(1..=n).context("truncated compact")?);
            (n + 1, u64::from_le_bytes(bytes))
        }
    };
    *input = &input[len..];
    Ok(value)
}

fn take<'a>(input: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
    ensure!(input.len() >= n, "truncated trie node");
    let (head, tail) = input.split_at(n);
    *input = tail;
    Ok(head)
}

fn nibbles(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().flat_map(|b| [b >> 4, b & 0xf]).collect()
}

enum Value<'a> {
    Inline(&'a [u8]),
    Hashed(&'a [u8]),
}

/// Follows `key` down from `node`, in `sp-trie`'s V1 node format: no
/// extension nodes, children and large values referenced by 32-byte hash.
fn trie_lookup(
    fetch: &impl Fn(&[u8]) -> Result<Vec<u8>>,
    node: Vec<u8>,
    key: &[u8],
) -> Result<Option<Vec<u8>>> {
    let mut input = &node[..];
    let first = *take(&mut input, 1)?.first().expect("took one byte");
    // (has a value, is a branch, value hashed, size bits)
    let (has_value, branch, hashed, size_bits) = match first >> 6 {
        0b01 => (true, false, false, 6),
        0b10 => (false, true, false, 6),
        0b11 => (true, true, false, 6),
        _ if first == 0 => return Ok(None),
        _ if first >> 5 == 0b001 => (true, false, true, 5),
        _ if first >> 4 == 0b0001 => (true, true, true, 4),
        _ => bail!("unknown trie node header {first:#04x}"),
    };
    let max = (1u16 << size_bits) as usize - 1;
    let mut count = usize::from(first) & max;
    if count == max {
        loop {
            let n = usize::from(take(&mut input, 1)?[0]);
            count += n;
            if n < 255 {
                break;
            }
        }
    }
    let packed = take(&mut input, count.div_ceil(2))?;
    let mut partial = nibbles(packed);
    if count % 2 == 1 {
        partial.remove(0);
    }
    let Some(rest) = key.strip_prefix(&partial[..]) else {
        return Ok(None);
    };

    let bitmap = if branch {
        u16::from_le_bytes(take(&mut input, 2)?.try_into()?)
    } else {
        0
    };
    let value = if !has_value {
        None
    } else if hashed {
        Some(Value::Hashed(take(&mut input, 32)?))
    } else {
        let len = compact(&mut input)? as usize;
        Some(Value::Inline(take(&mut input, len)?))
    };
    if rest.is_empty() {
        return match value {
            None => Ok(None),
            Some(Value::Inline(v)) => Ok(Some(v.to_vec())),
            Some(Value::Hashed(h)) => fetch(h).map(Some),
        };
    }
    if !branch {
        return Ok(None);
    }
    let wanted = usize::from(rest[0]);
    for i in 0..16 {
        if bitmap & (1 << i) == 0 {
            continue;
        }
        let len = compact(&mut input)? as usize;
        let child = take(&mut input, len)?;
        if i == wanted {
            let child = if len == 32 {
                fetch(child)?
            } else {
                child.to_vec()
            };
            return trie_lookup(fetch, child, &rest[1..]);
        }
    }
    Ok(None)
}
