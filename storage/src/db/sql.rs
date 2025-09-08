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

//! An implementation of `super::DB` backed by an SQLite database.
//!
//! Because our needs are very simple, we use the bare-bones `rusqlite` crate,
//! instead of something fancier like `diesel` (a full-on ORM, which seems like
//! overkill) or `sqlx` (a db agnostic, async-first SQL toolkit, but claimed to
//! be "7-70x slower" than `rusqlite` and `diesel`).
//!
//! We need to implement a mapping from `ArenaKey` hashes to `OnDiskObject {
//! data: Vec<u8>, ref_count: u32, children: Vec<ArenaKey> }`. For simplicity,
//! we use a single table `node`, with the hash keys as primary ids, and store the
//! vector of children hashes as a serialized binary blob. Alternatively, since
//! the hashes are expected to be 32 bytes, we could probably improve disk usage
//! a little at the expense of slower lookups and more implementation
//! complexity, by introducing a standard integer primary key, and storing the
//! children hashes in a separate 3-column join table of the form `parent id x
//! child id x child index`.
//!
//! We also need to keep track of which hashes are GC roots, and most hashes are
//! not roots, so we have a separate table `root` for that.
//!
//! We serialize hash keys using versioned serialization via
//! [`crate::serialize::Serializable`], but behind the scenes no version information is
//! included for the hash keys, i.e. there is no version overhead from
//! this. However, the serialization format for `ArenaKey` includes the length
//! of the vector, even tho these are always the same size for a given hash
//! function, so we could probably reduce from 36 to 32 bytes per hash using a
//! custom serializer.
//!
//! SQLite write transactions are very expensive, so intensive write operations
//! should always use the `SqlDB::batch_update` when possible. See discussion
//! here: <https://www.sqlite.org/draft/faq.html#q19>. See discussion inside the
//! `all_ops_memory` test for ways to speed up transactions that we didn't
//! enable. A more drastic alternative would be to expose transactions in the
//! API, and let the user create a single transaction for a batch of operations.
//!
//! This module handles concurrency without corruption, but may crash under high
//! concurrent loads, when the busy timeout is exceeded. If someone is
//! experiencing crashes due to high concurrency at some point in the future,
//! first make sure the high load is justified (e.g. should `bulk_insert` be
//! used instead?), and then increase the busy timeout in `SqlDB::new`.
use super::{DB, Update};
#[cfg(feature = "proptest")]
use crate::db::DummyDBStrategy;
use crate::{
    DefaultHasher, WellBehavedHasher, arena::ArenaKey, backend::OnDiskObject, db::DummyArbitrary,
};
use crypto::digest::generic_array::GenericArray;
#[cfg(feature = "proptest")]
use proptest::prelude::*;
use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{
    Connection, OptionalExtension, Result, ToSql, Transaction,
    TransactionBehavior::{self, Deferred, Immediate},
    config::DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY,
    params,
    types::FromSql,
};
use serialize::{Deserializable, Serializable};
use std::{
    collections::{HashMap, HashSet},
    fs::{File, OpenOptions},
    marker::PhantomData,
    path::Path,
};

/// A `DB` backed by an SQLite database.
#[derive(Debug)]
pub struct SqlDB<H: WellBehavedHasher = DefaultHasher> {
    pool: Pool<SqliteConnectionManager>,
    _phantom: std::marker::PhantomData<H>,
    lock_file: Option<File>,
}

impl<H: WellBehavedHasher> Default for SqlDB<H> {
    /// Create a new `SqlDB` with a random temporary file.
    ///
    /// Note: some stress tests assume that the default constructor here creates
    /// a file-backed `SqlDB`, since that's what we want to stress test.
    fn default() -> Self {
        let path = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        Self::exclusive_file(path)
    }
}

impl<H: WellBehavedHasher> SqlDB<H> {
    /// Create in-memory DB.
    ///
    /// This doesn't handle concurrency well.
    pub fn memory() -> Self {
        Self::new(SqliteConnectionManager::memory(), None)
    }

    /// Open file-based DB, creating it if it doesn't already exist.
    ///
    /// The database is opened exclusively, meaning attempting to create a
    /// second `SqlDB` pointing at the same file will fail, as long as the first
    /// `SqlDB` hasn't been `drop`ed yet.
    ///
    /// # Note
    ///
    /// Nothing is stopping someone else from opening the same DB via some other
    /// means, e.g. the command line SQLite client. Here we just prevent
    /// creating another `SqlDB` instance.
    pub fn exclusive_file<P: AsRef<Path>>(path: P) -> Self {
        Self::file(path, true)
    }

    /// Open file-based DB, creating it if it doesn't already exist.
    ///
    /// The database is opened non-exclusively, meaning creating multiple
    /// non-exclusive `SqlDB` instances pointing at the same file is
    /// allowed. However, you can't open exclusive and non-exclusive `SqlDB`
    /// instances at the same time.
    ///
    /// # Note
    ///
    /// We only expose this function for testing, because the point of exclusive
    /// DB access is to avoid accidental non-exclusive DB access in ledger code.
    #[cfg(test)]
    pub(crate) fn non_exclusive_file<P: AsRef<Path>>(path: P) -> Self {
        Self::file(path, false)
    }

    /// Open file-based DB, creating it if it doesn't already exist.
    ///
    /// If `exclusive` is true, then no other `SqlDB` instance can be created
    /// for the same `path` while this `SqlDB` is alive.
    ///
    /// # Note
    ///
    /// The database file itself will be created if it doesn't exist, but the
    /// parent directory of the database file must already exist.
    fn file<P: AsRef<Path>>(path: P, exclusive: bool) -> Self {
        // Compute the mutex file path as the canonicalized `path` with the
        // added extension `.mutex`.

        let normalized_path = path
            .as_ref()
            .canonicalize()
            .unwrap_or_else(|e| panic!("can't canonicalize path {:?}: {e}", path.as_ref()));
        let mut mutex_file_path = normalized_path.clone();
        mutex_file_path.set_extension("mutex");

        // Lock the mutex file, after creating it if necessary.

        let lock_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&mutex_file_path)
            .unwrap_or_else(|e| panic!("can't open .mutex file {:?}: {e}", &mutex_file_path));
        if exclusive {
            fs2::FileExt::try_lock_exclusive(&lock_file)
                .expect("can't get exclusive lock with existing locks active");
        } else {
            fs2::FileExt::try_lock_shared(&lock_file)
                .expect("can't get shared lock with exclusive lock active");
        }

        Self::new(SqliteConnectionManager::file(path), Some(lock_file))
    }

    /// Create a new db using the provided connection manager.
    ///
    /// The optional `lock_file` will be unlocked on [`Self::drop`] if
    /// `Some`. See [`Self::file`].
    fn new(cm: SqliteConnectionManager, lock_file: Option<File>) -> Self {
        let init = |conn: &mut Connection| {
            // Enable foreign-key support.
            assert!(
                conn.set_db_config(SQLITE_DBCONFIG_ENABLE_FKEY, true)?,
                "foreign keys aren't supported"
            );

            // Disable "synchronous" transaction commits.
            //
            // This greatly speeds up transactions, but the tradeoff is that
            // power losses and OS crashes may now corrupt the database. The
            // database can't be corrupted by application crashes, regardless of
            // the `synchronous` mode.
            //
            // https://www.sqlite.org/pragma.html#pragma_synchronous
            //
            // The default setting is 2=FULL, here we set 0=OFF, allowing
            // override via undocumented env var.
            let synchronous: u32 =
                std::env::var("MIDNIGHT_STORAGE_DB_SQL_SYNCHRONOUS").map_or(0, |v| {
                    v.parse().expect(
                        "MIDNIGHT_STORAGE_DB_SQL_SYNCHRONOUS invalid as u32:
            {v}",
                    )
                });
            conn.pragma_update(None, "synchronous", synchronous)?;
            // If you want to see a current setting, use e.g.
            //
            // conn.pragma_query(None, "synchronous", |s| {
            //     println!("synchronous={:?}", s);
            //     Ok(())
            // })?;

            // Enable the write-ahead log journaling mode, which is faster for
            // both concurrent and non-concurrent work loads.
            //
            // https://www.sqlite.org/wal.html
            // https://www.sqlite.org/pragma.html#pragma_journal_mode
            //
            // Default is DELETE, here we set WAL, allowing override via
            // undocumented env var.
            let journal_mode =
                std::env::var("MIDNIGHT_STORAGE_DB_SQL_JOURNAL_MODE").unwrap_or("WAL".to_string());
            conn.pragma_update(None, "journal_mode", journal_mode)?;

            // Explicitly set the busy timeout (the default is 5 seconds). Increase
            // this as needed if we're ever doing high concurrency. See discussion
            // at `SqlDB::with_tx`. The default value of 5 seconds causes the
            // `concurrent_access_file` test to fail sometimes.
            conn.busy_timeout(std::time::Duration::from_millis(10_000))
        };
        let db = SqlDB {
            pool: Pool::new(cm.with_init(init)).unwrap(),
            _phantom: PhantomData,
            lock_file,
        };
        db.create_tables();
        db
    }

    /// Create database tables and indices if they don't already exist.
    fn create_tables(&self) {
        self.with_tx(Immediate, |tx| {
            let sql = "CREATE TABLE IF NOT EXISTS node (
                     key BLOB NOT NULL PRIMARY KEY,
                     data BLOB NOT NULL,
                     ref_count INT NOT NULL,
                     children BLOB NOT NULL
                   )";
            tx.execute(sql, ()).unwrap();
            let sql = "CREATE INDEX IF NOT EXISTS ix_node_ref_count ON node (ref_count)";
            tx.execute(sql, ()).unwrap();
            // Altho the `root.key` is logically a foreign key referencing
            // `node.key`, we don't enforce that here, because we need to allow
            // out of order updates -- e.g. updating the root count for a key
            // before inserting the node for that key -- in order for the
            // backend write-cache-overflow flushing to work: we don't control
            // the order of writes produced by that process, and don't even
            // know if the node creation will happen in the same `batch_update`
            // as any root-count updates.
            let sql = "CREATE TABLE IF NOT EXISTS root (
                     key BLOB NOT NULL PRIMARY KEY,
                     count INT NOT NULL
                   )";
            tx.execute(sql, ()).unwrap();
            let sql = "CREATE INDEX IF NOT EXISTS ix_root_count ON root (count)";
            tx.execute(sql, ()).unwrap();
        })
    }

    /// Convenience function that wraps a closure in a transaction.
    ///
    /// Note: If `closure` does any DB modification, then it must use `behavior
    /// = Immediate` to start a write transaction right away, otherwise we may
    /// get a non-timeout `SQL_BUSY` error with concurrency: with `behavior =
    /// Deferred`, a read transaction is started, and then SQLite attempts to
    /// upgrade it to a write transaction on the first mutating SQL
    /// statement. However, if upgrading to a write transaction is not possible,
    /// the mutating SQL statement will fail immediately with `SQL_BUSY`. On the other hand, if
    /// we start with a write transaction immediately, and there is a conflict,
    /// we'll block for the SQLite busy timeout before failing.
    ///
    /// A closure that only reads should always start a read transaction with
    /// `behavior = Implicit`, since those are non-exclusive.
    ///
    /// SQLite transaction documentation:
    /// - <https://www.sqlite.org/atomiccommit.html>
    /// - <https://www.sqlite.org/lang_transaction.html>
    fn with_tx<F, R>(&self, behavior: TransactionBehavior, closure: F) -> R
    where
        F: FnOnce(&Transaction) -> R,
        R: Send,
    {
        // In theory, getting a connection here can fail if there are too
        // many concurrent connections, so if we have this level of
        // concurrency later then we need to implement some retry logic
        // here. However, testing 100 concurrent tasks accessing the DB
        // doesn't trigger this: we run into problems with SQL_BUSY timeouts
        // long before we run out of connections.
        let mut conn = self
            .pool
            .get()
            .expect("UNIMPLEMENTED: should retry when connection is not available");
        let tx = conn.transaction_with_behavior(behavior).unwrap();
        let result = closure(&tx);
        tx.commit().unwrap();
        result
    }

    /// Remove all unreachable nodes from the DB.
    ///
    /// The `additional_roots` are used as additional roots, in addition to
    /// roots already marked in the DB.
    ///
    /// # Note
    ///
    /// This version of GC assumes that the back-end has no pending writes, and
    /// so would require a flush before calling it. A backend-aware GC
    /// implementation is provided by [`crate::backend::StorageBackend::gc`].
    ///
    /// # Note
    ///
    /// This GC implementation assumes the correctness of the reference counts
    /// stored in the db, and doesn't actually do a reachability search from the
    /// roots. This is much faster than searching the entire db from the roots,
    /// but means this function is not sufficient to clean up the db after a
    /// crash which left the db in an inconsistent state, in terms of db-stored
    /// reference counts.
    fn _gc(&mut self, additional_roots: HashSet<ArenaKey<H>>) {
        self.with_tx(Immediate, |tx| {
            // Select keys that are not roots and have a `ref_count` of 0.
            let sql =
                "SELECT key FROM node WHERE key NOT IN (SELECT key FROM root) AND ref_count = 0";
            let mut get_unreachable_keys = tx.prepare(sql).unwrap();
            // Select `children` of `key`.
            let sql = "SELECT children FROM node WHERE key = (?1)";
            let mut get_children = tx.prepare(sql).unwrap();
            // Decrement `ref_count` of `key` by 1.
            let sql = "UPDATE node SET ref_count = ref_count - 1 WHERE key = (?1)";
            let mut dec_ref_count = tx.prepare(sql).unwrap();
            // Delete `key`.
            let sql = "DELETE FROM node WHERE key = (?1)";
            let mut delete_node = tx.prepare(sql).unwrap();

            // Keep decrementing ref counts and deleting nodes until there are no
            // more unreachable nodes.
            //
            // Possible optimization: instead of selecting all unreachable keys
            // from db, and then filtering out the additional roots in Rust
            // land, we could instead store the additional roots in the db -- in
            // a temp table that we clear at the beginning of gc -- and then do
            // the filtering in the SQL query itself.
            loop {
                let unreachable_keys: Vec<_> = get_unreachable_keys
                    .query_map([], |row| {
                        let key: ArenaKey<H> = row.get(0)?;
                        Ok(key)
                    })
                    .unwrap()
                    .map(|r| r.unwrap())
                    .filter(|k: &ArenaKey<H>| !additional_roots.contains(k))
                    .collect();
                if unreachable_keys.is_empty() {
                    break;
                }
                for key in unreachable_keys {
                    let children: Vec<ArenaKey<H>> = get_children
                        .query_row(params![key.clone()], |row| {
                            let children: Children<H> = row.get(0)?;
                            Ok(children.0)
                        })
                        .unwrap();
                    for child in children {
                        dec_ref_count.execute(params![child]).unwrap();
                    }
                    delete_node.execute(params![key]).unwrap();
                }
            }

            get_unreachable_keys.finalize().unwrap();
            get_children.finalize().unwrap();
            dec_ref_count.finalize().unwrap();
            delete_node.finalize().unwrap();
        })
    }

    /// Implementation of `Clone::clone` for testing `SqlDB::memory` `DB`s concurrently.
    #[cfg(test)]
    pub(crate) fn clone_memory_db(&self) -> Self {
        match self.lock_file {
            Some(_) => panic!("Can't clone file db: found lock file!"),
            None => SqlDB {
                pool: self.pool.clone(),
                _phantom: self._phantom,
                lock_file: None,
            },
        }
    }
}

impl<H: WellBehavedHasher> Drop for SqlDB<H> {
    fn drop(&mut self) {
        if let Some(lock_file) = &self.lock_file {
            if let Err(e) = fs2::FileExt::unlock(lock_file) {
                eprintln!("Failed to unlock mutex file: {:?}", e);
            }
        }
    }
}

impl<H: WellBehavedHasher> ToSql for ArenaKey<H> {
    fn to_sql(&self) -> Result<rusqlite::types::ToSqlOutput<'_>> {
        // We could use `serialize` here, but then we'd get the length in the
        // front. We probably don't care, but having the pure, unprefixed key
        // here should be slightly more convenient if we're manually poking
        // around in the db for some reason.
        Ok(self.0.to_vec().into())
    }
}

impl<H: WellBehavedHasher> FromSql for ArenaKey<H> {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        Ok(ArenaKey(
            GenericArray::from_slice(value.as_bytes()?).clone(),
        ))
    }
}

// Newtype wrapper for `OnDiskObject.children`, so we can implement conversion
// traits.
struct Children<H: WellBehavedHasher>(Vec<ArenaKey<H>>);

impl<H: WellBehavedHasher> ToSql for Children<H> {
    fn to_sql(&self) -> Result<rusqlite::types::ToSqlOutput<'_>> {
        let mut buf = vec![];
        self.0.serialize(&mut buf).unwrap();
        Ok(buf.into())
    }
}

impl<H: WellBehavedHasher> FromSql for Children<H> {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        Ok(Children(
            Deserializable::deserialize(&mut value.as_bytes()?, 0).unwrap(),
        ))
    }
}

impl<H: WellBehavedHasher> DB for SqlDB<H> {
    type Hasher = H;

    fn get_node(&self, key: &ArenaKey<H>) -> Option<OnDiskObject<H>> {
        let key = key.clone();
        self.with_tx(Deferred, |tx| {
            let sql = "SELECT data, ref_count, children FROM node WHERE key = (?1)";
            let mut stmt = tx.prepare(sql).unwrap();
            let result = stmt
                .query_row(params![key], |row| {
                    let data = row.get(0)?;
                    let ref_count = row.get(1)?;
                    let children: Children<H> = row.get(2)?;
                    let children = children.0;
                    Ok(OnDiskObject {
                        data,
                        ref_count,
                        children,
                    })
                })
                .optional()
                .unwrap();
            stmt.finalize().unwrap();
            result
        })
    }

    fn get_unreachable_keys(&self) -> Vec<ArenaKey<H>> {
        self.with_tx(Deferred, |tx| {
            // Select keys that are not roots and have a `ref_count` of 0.
            let sql =
                "SELECT key FROM node WHERE key NOT IN (SELECT key FROM root) AND ref_count = 0";
            let mut get_unreachable_keys = tx.prepare(sql).unwrap();
            let unreachable_keys: Vec<ArenaKey<H>> = get_unreachable_keys
                .query_map([], |row| {
                    let key: ArenaKey<H> = row.get(0)?;
                    Ok(key)
                })
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            get_unreachable_keys.finalize().unwrap();
            unreachable_keys
        })
    }

    /// Batch get nodes for all keys in `keys`.
    fn batch_get_nodes<I>(&self, keys: I) -> Vec<(ArenaKey<H>, Option<OnDiskObject<H>>)>
    where
        I: Iterator<Item = ArenaKey<H>>,
    {
        let keys = keys.collect::<Vec<_>>();
        self.with_tx(Deferred, |tx| {
            let sql = "SELECT data, ref_count, children FROM node WHERE key = (?1)";
            let mut stmt = tx.prepare(sql).unwrap();
            let result = keys
                .into_iter()
                .filter_map(|key| {
                    stmt.query_row(params![key.clone()], |row| {
                        let data = row.get(0)?;
                        let ref_count = row.get(1)?;
                        let children: Children<H> = row.get(2)?;
                        let children = children.0;
                        let obj = OnDiskObject {
                            data,
                            ref_count,
                            children,
                        };
                        Ok((key, Some(obj)))
                    })
                    .optional()
                    .unwrap()
                })
                .collect();
            stmt.finalize().unwrap();
            result
        })
    }

    /// Always use `batch_update` instead if you have a lot of keys to insert!
    fn insert_node(&mut self, key: ArenaKey<H>, object: OnDiskObject<H>) {
        self.with_tx(Immediate, |tx| {
            let sql = "INSERT OR REPLACE INTO node (key, data, ref_count, children) \
                       VALUES (?1, ?2, ?3, ?4)";
            let mut stmt = tx.prepare(sql).unwrap();
            stmt.execute(params![
                key,
                object.data,
                object.ref_count,
                Children(object.children)
            ])
            .unwrap();
            stmt.finalize().unwrap();
        })
    }

    /// Always use `batch_update` instead if you have a lot of keys to delete!
    fn delete_node(&mut self, key: &ArenaKey<H>) {
        let key = key.clone();
        self.with_tx(Immediate, |tx| {
            let sql = "DELETE FROM node WHERE key = (?1)";
            let mut stmt = tx.prepare(sql).unwrap();
            stmt.execute(params![key]).unwrap();
            stmt.finalize().unwrap();
        })
    }

    /// This is significantly faster than the default implementation provided by
    /// the trait!
    fn batch_update<I>(&mut self, iter: I)
    where
        I: Iterator<Item = (ArenaKey<H>, Update<H>)>,
    {
        use Update::*;
        // For batching at the SQL level, this approach is supposed to be faster
        // (and easier!) than building up large INSERTs:
        // https://stackoverflow.com/a/5209093/470844
        self.with_tx(Immediate, |tx| {
            let sql = "INSERT OR REPLACE INTO node (key, data, ref_count, children) \
                       VALUES (?1, ?2, ?3, ?4)";
            let mut insert_node = tx.prepare(sql).unwrap();
            let sql = "DELETE FROM node WHERE key = (?1)";
            let mut delete_node = tx.prepare(sql).unwrap();
            let sql = "INSERT OR REPLACE INTO root (key, count) \
                       VALUES (?1, ?2)";
            let mut set_root_count = tx.prepare(sql).unwrap();
            let sql = "DELETE FROM root WHERE key = (?1)";
            let mut delete_root_count = tx.prepare(sql).unwrap();
            for (key, update) in iter {
                match update {
                    DeleteNode => delete_node.execute(params![key]).unwrap(),
                    InsertNode(object) => insert_node
                        .execute(params![
                            key,
                            object.data,
                            object.ref_count,
                            Children(object.children.clone())
                        ])
                        .unwrap(),
                    SetRootCount(count) => {
                        if count > 0 {
                            set_root_count.execute(params![key, count]).unwrap()
                        } else {
                            delete_root_count.execute(params![key]).unwrap()
                        }
                    }
                };
            }
            insert_node.finalize().unwrap();
            delete_node.finalize().unwrap();
            set_root_count.finalize().unwrap();
            delete_root_count.finalize().unwrap();
        })
    }

    fn size(&self) -> usize {
        self.with_tx(Deferred, |tx| {
            let sql = "SELECT COUNT(*) FROM node";
            let mut stmt = tx.prepare(sql).unwrap();
            let result = stmt.query_row([], |row| row.get(0)).unwrap();
            stmt.finalize().unwrap();
            result
        })
    }

    fn get_root_count(&self, key: &ArenaKey<Self::Hasher>) -> u32 {
        let key = key.clone();
        self.with_tx(Deferred, |tx| {
            let sql = "SELECT count FROM root WHERE key = (?1)";
            let mut stmt = tx.prepare(sql).unwrap();
            let result = stmt
                .query_row(params![key], |row| row.get(0))
                .optional()
                .unwrap()
                .unwrap_or(0);
            stmt.finalize().unwrap();
            result
        })
    }

    fn set_root_count(&mut self, key: ArenaKey<Self::Hasher>, count: u32) {
        self.with_tx(Immediate, |tx| {
            if count > 0 {
                let sql = "INSERT OR REPLACE INTO root (key, count) \
                       VALUES (?1, ?2)";
                let mut stmt = tx.prepare(sql).unwrap();
                stmt.execute(params![key, count]).unwrap();
                stmt.finalize().unwrap();
            } else {
                let sql = "DELETE FROM root WHERE key = (?1)";
                let mut stmt = tx.prepare(sql).unwrap();
                stmt.execute(params![key]).unwrap();
                stmt.finalize().unwrap();
            }
        })
    }

    fn get_roots(&self) -> HashMap<ArenaKey<Self::Hasher>, u32> {
        self.with_tx(Deferred, |tx| {
            let sql = "SELECT key, count FROM root";
            let mut stmt = tx.prepare(sql).unwrap();
            let result = stmt
                .query_map([], |row| {
                    let key: ArenaKey<H> = row.get(0)?;
                    let count: u32 = row.get(1)?;
                    Ok((key, count))
                })
                .unwrap()
                .map(|r| r.unwrap())
                .collect();
            stmt.finalize().unwrap();
            result
        })
    }
}

impl<H: WellBehavedHasher> DummyArbitrary for SqlDB<H> {}

#[cfg(feature = "proptest")]
/// A dummy Arbitrary impl for `SqlDB` to allow for deriving Arbitrary on Sp<T, D>
impl<H: WellBehavedHasher> Arbitrary for SqlDB<H> {
    type Parameters = ();
    type Strategy = DummyDBStrategy<Self>;

    fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
        DummyDBStrategy::<Self>(PhantomData)
    }
}

#[cfg(test)]
mod tests {
    use super::{SqlDB, Update::*};
    use crate::{DefaultHasher, WellBehavedHasher, arena::ArenaKey, backend::OnDiskObject, db::DB};
    use rand::Rng;
    use rusqlite::TransactionBehavior::Deferred;
    use rusqlite::types::FromSql;
    use std::collections::HashSet;

    /// This test always fails due to db locking errors. Since we don't intend
    /// to use the memory back-end anyway, not going to fix this.
    #[test]
    #[ignore = "always fails, indep of busy timeout"]
    fn concurrent_access_memory() {
        let db = SqlDB::memory();
        let mk_db = || db.clone_memory_db();
        test_concurrent_access(mk_db);
    }

    #[test]
    fn concurrent_access_file() {
        let path: tempfile::TempPath = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let mk_db = || SqlDB::non_exclusive_file(&path);
        test_concurrent_access(mk_db);
    }

    // Increasing `NUM_WRITE_JOBS`, `NUM_BULK_JOBS`, or `ITERS_PER_JOB` much
    // leads to SQL_BUSY errors for the default timeout of 5 seconds.
    const NUM_WRITE_JOBS: usize = 5;
    const NUM_BULK_JOBS: usize = 10;
    const NUM_READ_JOBS: usize = 100;
    const ITERS_PER_JOB: usize = 10;

    /// Test concurrent reading and writing.
    fn test_concurrent_access(mk_db: impl Fn() -> SqlDB) {
        let mut rng = rand::thread_rng();
        let k: ArenaKey<_> = rng.r#gen();
        let v: OnDiskObject<_> = rng.r#gen();
        let mut jobs = vec![];
        for _ in 0..NUM_WRITE_JOBS {
            let (k, v, db) = (k.clone(), v.clone(), mk_db());
            jobs.push(std::thread::spawn(move || {
                insert_read_delete_loop(k, v, db)
            }));
        }
        for _ in 0..NUM_BULK_JOBS {
            let (k, v, db) = (k.clone(), v.clone(), mk_db());
            jobs.push(std::thread::spawn(move || bulk_insert_loop(k, v, db)));
        }
        for _ in 0..NUM_READ_JOBS {
            let (k, db) = (k.clone(), mk_db());
            jobs.push(std::thread::spawn(move || read_loop(k, db)));
        }
        for job in jobs {
            job.join().unwrap();
        }
    }

    /// Helper for testing concurrent DB access.
    ///
    /// Note that this is not a realistic workload, since inserting or deleting
    /// in a loop should always be done with `bulk_insert` instead.
    fn insert_read_delete_loop<H: WellBehavedHasher>(
        k: ArenaKey<H>,
        v: OnDiskObject<H>,
        mut db: SqlDB<H>,
    ) {
        for _ in 0..ITERS_PER_JOB {
            db.insert_node(k.clone(), v.clone());
            db.get_node(&k);
            db.delete_node(&k);
        }
    }

    /// Helper for testing concurrent DB access.
    fn bulk_insert_loop<H: WellBehavedHasher>(
        k: ArenaKey<H>,
        v: OnDiskObject<H>,
        mut db: SqlDB<H>,
    ) {
        let u = InsertNode(v);
        let iter = std::iter::repeat((k.clone(), u.clone())).take(ITERS_PER_JOB);
        db.batch_update(iter);
        db.delete_node(&k);
    }

    /// Helper for testing concurrent DB access.
    fn read_loop<H: WellBehavedHasher>(k: ArenaKey<H>, db: SqlDB<H>) {
        for _ in 0..ITERS_PER_JOB {
            db.get_node(&k);
        }
    }

    /// Test the db-level garbage collection. Note this is not the same as the
    /// backend-level GC, which is what we actually use in practice. This
    /// db-level GC can only be run when the db is in a logically consistent
    /// state, i.e. when there are no pending writes in the back-end.
    #[test]
    fn db_level_gc() {
        use crate::backend::raw_node::RawNode;

        let n5 = RawNode::new(&[5], 2, vec![]);
        let n4 = RawNode::new(&[4], 1, vec![&n5]);
        let n3 = RawNode::new(&[3], 1, vec![&n5]);
        let n2 = RawNode::new(&[2], 1, vec![&n4, &n3]);
        let n1 = RawNode::new(&[1], 0, vec![&n2]);
        let nodes: [&RawNode; 5] = [&n5, &n4, &n3, &n2, &n1];

        let init_db = || {
            let mut db = SqlDB::default();
            for n in nodes.iter() {
                n.insert_into_db(&mut db);
            }
            for n in nodes.iter() {
                assert!(db.get_node(&n.key).is_some());
            }
            db
        };

        ////////////////////////////////////////////////////////////////

        let mut db = init_db();
        db.set_root_count(n1.key.clone(), 1);
        db._gc(HashSet::new());
        for n in nodes.iter() {
            assert!(db.get_node(&n.key).is_some());
        }
        db.set_root_count(n1.key.clone(), 0);
        db._gc(HashSet::new());
        assert_eq!(db.size(), 0);

        ////////////////////////////////////////////////////////////////

        let mut db = init_db();

        db.set_root_count(n2.key.clone(), 1);
        db._gc(HashSet::new());
        assert!(db.get_node(&n1.key).is_none());
        assert!(db.get_node(&n2.key).is_some());
        assert!(db.get_node(&n3.key).is_some());
        assert!(db.get_node(&n4.key).is_some());
        assert!(db.get_node(&n5.key).is_some());

        db.set_root_count(n2.key.clone(), 0);
        db.set_root_count(n3.key.clone(), 1);
        db._gc(HashSet::new());
        assert!(db.get_node(&n1.key).is_none());
        assert!(db.get_node(&n2.key).is_none());
        assert!(db.get_node(&n3.key).is_some());
        assert!(db.get_node(&n4.key).is_none());
        assert!(db.get_node(&n5.key).is_some());

        db.set_root_count(n3.key.clone(), 0);
        db._gc(HashSet::new());
        assert_eq!(db.size(), 0);

        ////////////////////////////////////////////////////////////////

        let mut db = init_db();
        let additional_roots = [n3.key.clone(), n4.key.clone()].into_iter().collect();
        db._gc(additional_roots);
        assert!(db.get_node(&n1.key).is_none());
        assert!(db.get_node(&n2.key).is_none());
        assert!(db.get_node(&n3.key).is_some());
        assert!(db.get_node(&n4.key).is_some());
        assert!(db.get_node(&n5.key).is_some());
    }

    ////////////////////////////////////////////////////////////////
    // Tests for exclusive and shared locking.

    /// Helper trait for reusing the same test code for both local and multi-thread
    /// testing. In Haskell we'd just make `test_exclusivity` take an
    /// `(() -> IO ()) -> IO ()` argument, but in Rust I couldn't figure out how to
    /// make an analog of that type-check without introducing indirection via
    /// this `Runner` trait 🤷 Yes, this is probably overkill compared to
    /// copy-pasting the test once ...
    trait Runner {
        fn run(&self, action: impl FnOnce() + Send);
    }

    /// Run in the current thread.
    struct LocalRunner;
    impl Runner for LocalRunner {
        fn run(&self, action: impl FnOnce()) {
            action();
        }
    }

    /// Run in a separate thread.
    struct ThreadRunner;
    impl Runner for ThreadRunner {
        fn run(&self, action: impl FnOnce() + Send) {
            std::thread::scope(|s| s.spawn(action).join().unwrap());
        }
    }

    #[test]
    fn exclusivity_local() {
        test_exclusivity(LocalRunner);
    }

    #[test]
    fn exclusivity_threaded() {
        test_exclusivity(ThreadRunner);
    }

    /// Test that exclusive and non-exclusive db creations interact correctly,
    /// both locally and across threads.
    fn test_exclusivity(runner: impl Runner) {
        let path: tempfile::TempPath = tempfile::NamedTempFile::new().unwrap().into_temp_path();

        // Multiple non-exclusive dbs are ok, but we can't create exclusive with
        // non-exclusive open.

        let db = SqlDB::<DefaultHasher>::non_exclusive_file(&path);
        runner.run(|| {
            SqlDB::<DefaultHasher>::non_exclusive_file(&path);
        });
        runner.run(|| {
            let result = std::panic::catch_unwind(|| {
                SqlDB::<DefaultHasher>::exclusive_file(&path);
            });
            assert!(result.is_err());
        });

        // Dropping releases the lock, so we can create an exclusive db. But
        // then we can't create any other exclusive or non-exclusive dbs.

        drop(db);
        let db = SqlDB::<DefaultHasher>::exclusive_file(&path);
        runner.run(|| {
            let result = std::panic::catch_unwind(|| {
                SqlDB::<DefaultHasher>::non_exclusive_file(&path);
            });
            assert!(result.is_err());
        });
        runner.run(|| {
            let result = std::panic::catch_unwind(|| {
                SqlDB::<DefaultHasher>::exclusive_file(&path);
            });
            assert!(result.is_err());
        });
        drop(db);
    }

    // Query the value of a pragma.
    fn query_pragma<T: FromSql + Sync + Send>(db: &SqlDB, pragma: &str) -> T {
        db.with_tx(Deferred, |tx| {
            tx.pragma_query_value(None, pragma, |row| row.get::<_, T>(0))
                .unwrap()
        })
    }

    // Test the default sqlite parameter values
    #[test]
    fn default_sqlite_params() {
        let path: tempfile::TempPath = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let db = SqlDB::exclusive_file(&path);
        let journal_mode: String = query_pragma(&db, "journal_mode");
        assert_eq!(journal_mode.to_uppercase(), "WAL");
        let synchronous: i32 = query_pragma(&db, "synchronous");
        assert_eq!(synchronous, 0);
    }

    // Test that we can override sqlite parameters with the environment
    // variables MIDNIGHT_STORAGE_DB_SQL_JOURNAL_MODE and
    // MIDNIGHT_STORAGE_DB_SQL_SYNCHRONOUS.
    //
    // WARNING: this test is unsafe to run along with performance sensitive
    // tests, because it overrides process-global env vars that control sqlite
    // config, making the sqlite db much slower. To properly isolate this, it
    // needs to be in a separate process, which could be accomplished by using
    // `crate:stress_test`. However, this test is not important, so just
    // `ignore`ing.
    #[test]
    #[ignore = "unsafe because it overrides the shared env"]
    fn env_override_sqlite_params() {
        use std::env;

        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { env::set_var("MIDNIGHT_STORAGE_DB_SQL_JOURNAL_MODE", "DELETE") };
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { env::set_var("MIDNIGHT_STORAGE_DB_SQL_SYNCHRONOUS", "2") };
        let path: tempfile::TempPath = tempfile::NamedTempFile::new().unwrap().into_temp_path();
        let db = SqlDB::exclusive_file(&path);
        let journal_mode: String = query_pragma(&db, "journal_mode");
        assert_eq!(journal_mode.to_uppercase(), "DELETE");
        let synchronous: i32 = query_pragma(&db, "synchronous");
        assert_eq!(synchronous, 2);
    }
}
