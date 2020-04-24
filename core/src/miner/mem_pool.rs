// Copyright 2018-2020 Kodebox, Inc.
// This file is part of CodeChain.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as
// published by the Free Software Foundation, either version 3 of the
// License, or (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use super::backup;
use super::mem_pool_types::{
    AccountDetails, CurrentQueue, FutureQueue, MemPoolInput, MemPoolItem, MemPoolStatus, PoolingInstant, QueueTag,
    TransactionOrder, TransactionOrderWithTag, TxOrigin,
};
use super::TransactionImportResult;
use crate::client::{AccountData, BlockChainTrait};
use crate::miner::fetch_account_creator;
use crate::transaction::{PendingVerifiedTransactions, VerifiedTransaction};
use crate::Error as CoreError;
use ckey::Ed25519Public as Public;
use ctypes::errors::{HistoryError, RuntimeError, SyntaxError};
use ctypes::{BlockNumber, TxHash};
use kvdb::{DBTransaction, KeyValueDB};
use std::cmp::max;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;
use table::Table;

const DEFAULT_POOLING_PERIOD: BlockNumber = 128;

#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    History(HistoryError),
    Runtime(RuntimeError),
    Syntax(SyntaxError),
}

impl Error {
    pub fn into_core_error(self) -> CoreError {
        match self {
            Error::History(err) => CoreError::History(err),
            Error::Runtime(err) => CoreError::Runtime(err),
            Error::Syntax(err) => CoreError::Syntax(err),
        }
    }
}

impl From<HistoryError> for Error {
    fn from(err: HistoryError) -> Error {
        Error::History(err)
    }
}

impl From<RuntimeError> for Error {
    fn from(err: RuntimeError) -> Error {
        Error::Runtime(err)
    }
}

impl From<SyntaxError> for Error {
    fn from(err: SyntaxError) -> Error {
        Error::Syntax(err)
    }
}

pub struct MemPool {
    /// A value which is used to check whether a new transaciton can replace a transaction in the memory pool with the same signer and seq.
    /// If the fee of the new transaction is `new_fee` and the fee of the transaction in the memory pool is `old_fee`,
    /// then `new_fee > old_fee + old_fee >> mem_pool_fee_bump_shift` should be satisfied to replace.
    /// Local transactions ignore this option.
    fee_bump_shift: usize,
    /// Maximal time transaction may occupy the pool.
    /// When we reach `max_time_in_pool / 2^3` we re-validate
    /// account balance.
    max_block_number_period_in_pool: PoolingInstant,
    /// Priority queue and fee counter for transactions that can go to block
    current: CurrentQueue,
    /// Priority queue for transactions that has been received but are not yet valid to go to block
    future: FutureQueue,
    /// All transactions managed by pool indexed by public and seq
    by_signer_public: Table<Public, u64, TransactionOrderWithTag>,
    /// The count(number) limit of each queue
    queue_count_limit: usize,
    /// The memory limit of each queue
    queue_memory_limit: usize,
    /// All transactions managed by pool indexed by hash
    by_hash: HashMap<TxHash, MemPoolItem>,
    /// Current seq of each public key (fee payer)
    first_seqs: HashMap<Public, u64>,
    /// Next seq of transaction in current (to quickly check next expected transaction)
    next_seqs: HashMap<Public, u64>,
    /// Check if there's any local transaction from specific account
    is_local_account: HashSet<Public>,
    /// The time when the pool is finally used
    last_block_number: PoolingInstant,
    /// The timestamp when the pool is finally used
    last_timestamp: u64,
    /// Next id that should be assigned to a transaction imported to the pool
    next_transaction_id: u64,
    /// Arc of KeyValueDB in which the backup information is stored.
    db: Arc<dyn KeyValueDB>,
}

impl MemPool {
    /// Create new instance of this Queue with specified limits
    pub fn with_limits(limit: usize, memory_limit: usize, fee_bump_shift: usize, db: Arc<dyn KeyValueDB>) -> Self {
        MemPool {
            fee_bump_shift,
            max_block_number_period_in_pool: DEFAULT_POOLING_PERIOD,
            current: CurrentQueue::new(),
            future: FutureQueue::new(),
            by_signer_public: Table::new(),
            queue_count_limit: limit,
            queue_memory_limit: memory_limit,
            by_hash: HashMap::new(),
            first_seqs: HashMap::new(),
            next_seqs: HashMap::new(),
            is_local_account: HashSet::new(),
            last_block_number: 0,
            last_timestamp: 0,
            next_transaction_id: 0,
            db,
        }
    }

    /// Set the new limit for `current` and `future` queue.
    pub fn set_limit(&mut self, limit: usize) {
        self.queue_count_limit = limit;
    }

    /// Enforce the limit to the current/future queue
    fn enforce_limit(&mut self, batch: &mut DBTransaction) {
        // Get transaction orders to drop from each queue (current/future)
        fn get_orders_to_drop(
            set: &BTreeSet<TransactionOrder>,
            limit: usize,
            memory_limit: usize,
        ) -> Vec<TransactionOrder> {
            let mut count = 0;
            let mut mem_usage = 0;
            set.iter()
                .filter(|order| {
                    count += 1;
                    mem_usage += order.mem_usage;
                    !order.origin.is_local() && (mem_usage > memory_limit || count > limit)
                })
                .cloned()
                .collect()
        }

        let to_drop_current =
            if self.current.mem_usage > self.queue_memory_limit || self.current.count > self.queue_count_limit {
                get_orders_to_drop(&self.current.queue, self.queue_count_limit, self.queue_memory_limit)
            } else {
                vec![]
            };

        let to_drop_future =
            if self.future.mem_usage > self.queue_memory_limit || self.future.count > self.queue_count_limit {
                get_orders_to_drop(&self.future.queue, self.queue_count_limit, self.queue_memory_limit)
            } else {
                vec![]
            };

        for (order, is_current) in
            to_drop_current.iter().map(|order| (order, true)).chain(to_drop_future.iter().map(|order| (order, false)))
        {
            let hash = order.hash;
            let item = self.by_hash.remove(&hash).expect("`by_hash` and `current/future` should be synced");
            backup::remove_item(batch, &hash);
            let signer_public = item.signer_public();
            let seq = item.seq();
            self.by_signer_public
                .remove(&signer_public, &seq)
                .expect("`by_hash` and `by_signer_public` should be synced");
            if self.by_signer_public.clear_if_empty(&signer_public) {
                self.is_local_account.remove(&signer_public);
            }
            if is_current {
                self.current.remove(order);
            } else {
                self.future.remove(order);
            }
        }
    }

    /// Returns current limit of transactions in the pool.
    pub fn limit(&self) -> usize {
        self.queue_count_limit
    }

    /// Get one more than the lowest fee in the pool iff the pool is
    /// full, otherwise 0.
    pub fn effective_minimum_fee(&self) -> u64 {
        if self.current.len() >= self.queue_count_limit {
            self.current.minimum_fee()
        } else {
            0
        }
    }

    /// Returns current status for this pool
    pub fn status(&self) -> MemPoolStatus {
        MemPoolStatus {
            pending: self.current.len(),
            future: self.future.len(),
        }
    }

    /// Add signed transaction to pool to be verified and imported.
    ///
    /// NOTE details_provider methods should be cheap to compute
    /// otherwise it might open up an attack vector.
    pub fn add<F>(
        &mut self,
        inputs: Vec<MemPoolInput>,
        inserted_block_number: PoolingInstant,
        inserted_timestamp: u64,
        fetch_account: &F,
    ) -> Vec<Result<TransactionImportResult, Error>>
    where
        F: Fn(&Public) -> AccountDetails, {
        ctrace!(MEM_POOL, "add() called, time: {}, timestamp: {}", inserted_block_number, inserted_timestamp);
        let mut insert_results = Vec::new();
        let mut to_insert: HashMap<Public, Vec<u64>> = HashMap::new();
        let mut new_local_accounts = HashSet::new();
        let mut batch = backup::backup_batch_with_capacity(inputs.len());

        for input in inputs {
            let tx = input.transaction;
            let signer_public = tx.signer_public();
            let seq = tx.transaction().seq;
            let hash = tx.hash();

            let origin = if input.origin.is_local() && !self.is_local_account.contains(&signer_public) {
                self.is_local_account.insert(signer_public);
                new_local_accounts.insert(signer_public);
                TxOrigin::Local
            } else if input.origin.is_external() && self.is_local_account.contains(&signer_public) {
                TxOrigin::Local
            } else {
                input.origin
            };

            let client_account = fetch_account(&signer_public);
            if let Err(e) = self.verify_transaction(&tx, origin, &client_account) {
                insert_results.push(Err(e));
                continue
            }

            let id = self.next_transaction_id;
            self.next_transaction_id += 1;
            let item = MemPoolItem::new(tx, origin, inserted_block_number, inserted_timestamp, id);
            let order = TransactionOrder::for_transaction(&item, client_account.seq);
            let order_with_tag = TransactionOrderWithTag::new(order, QueueTag::New);

            backup::backup_item(&mut batch, *hash, &item);
            self.by_hash.insert(hash, item);

            if let Some(old_order_with_tag) = self.by_signer_public.insert(signer_public, seq, order_with_tag) {
                let old_order = old_order_with_tag.order;
                let tag = old_order_with_tag.tag;

                self.by_hash.remove(&old_order.hash);
                backup::remove_item(&mut batch, &old_order.hash);

                match tag {
                    QueueTag::Current => {
                        self.current.remove(&old_order);
                    }
                    QueueTag::Future => {
                        self.future.remove(&old_order);
                    }
                    QueueTag::New => unreachable!(),
                }
            }

            to_insert.entry(signer_public).or_default().push(seq);
            insert_results.push(Ok((signer_public, seq)));
        }

        let keys = self.by_signer_public.keys().map(Clone::clone).collect::<Vec<_>>();

        for public in keys {
            let current_seq = fetch_account(&public).seq;
            let mut first_seq = *self.first_seqs.get(&public).unwrap_or(&0);
            let next_seq = self.next_seqs.get(&public).cloned().unwrap_or(current_seq);

            let target_seq = if current_seq < first_seq
                || inserted_block_number < self.last_block_number
                || inserted_timestamp < self.last_timestamp
                || next_seq < current_seq
            {
                current_seq
            } else {
                next_seq
            };
            let new_next_seq = self.next_seq_of_queued(public, target_seq);

            let is_this_account_local = new_local_accounts.contains(&public);
            // Need to update transactions because of height/origin change
            if current_seq != first_seq || is_this_account_local {
                self.update_orders(public, current_seq, new_next_seq, is_this_account_local, &mut batch);
                self.first_seqs.insert(public, current_seq);
                first_seq = current_seq;
            }
            // We don't need to update the height, just move transactions
            else if new_next_seq < next_seq {
                self.move_queue(public, new_next_seq, next_seq, QueueTag::Future);
            } else if new_next_seq > next_seq {
                self.move_queue(public, next_seq, new_next_seq, QueueTag::Current);
            }

            if new_next_seq <= first_seq {
                self.next_seqs.remove(&public);
            } else {
                self.next_seqs.insert(public, new_next_seq);
            }

            if let Some(seq_list) = to_insert.get(&public) {
                self.add_new_orders_to_queue(public, seq_list, new_next_seq);
            }

            if self.by_signer_public.clear_if_empty(&public) {
                self.is_local_account.remove(&public);
            }
        }

        self.enforce_limit(&mut batch);

        self.last_block_number = inserted_block_number;
        self.last_timestamp = inserted_timestamp;

        assert_eq!(self.current.len() + self.future.len(), self.by_hash.len());
        assert_eq!(self.current.fee_counter.values().sum::<usize>(), self.current.len());
        assert_eq!(self.by_signer_public.len(), self.by_hash.len());

        self.db.write(batch).expect("Low level database error. Some issue with disk?");
        insert_results
            .into_iter()
            .map(|v| match v {
                Ok((signer_public, seq)) => match self.by_signer_public.get(&signer_public, &seq) {
                    Some(order_with_tag) => match order_with_tag.tag {
                        QueueTag::Current => Ok(TransactionImportResult::Current),
                        QueueTag::Future => Ok(TransactionImportResult::Future),
                        QueueTag::New => unreachable!(),
                    },
                    None => Err(HistoryError::LimitReached.into()),
                },
                Err(e) => Err(e),
            })
            .collect()
    }

    /// Clear both current and future.
    pub fn remove_all(&mut self) {
        self.current.clear();
        self.future.clear();
    }

    /// Checks the current seq for all transactions' senders in the pool and removes the old transactions.
    pub fn remove_old<F>(&mut self, fetch_account: &F, current_block_number: PoolingInstant, current_timestamp: u64)
    where
        F: Fn(&Public) -> AccountDetails, {
        ctrace!(MEM_POOL, "remove_old() called, time: {}, timestamp: {}", current_block_number, current_timestamp);
        let signers =
            self.by_signer_public.keys().map(|sender| (*sender, fetch_account(sender))).collect::<HashMap<_, _>>();
        let max_block_number = self.max_block_number_period_in_pool;
        let balance_check = max_block_number >> 3;

        // Clear transactions occupying the pool too long
        let invalid = self
            .by_hash
            .iter()
            .filter(|&(_, ref item)| !item.origin.is_local())
            .map(|(hash, item)| (hash, item, current_block_number.saturating_sub(item.inserted_block_number)))
            .filter_map(|(hash, item, time_diff)| {
                if time_diff > max_block_number {
                    return Some(*hash)
                }

                if time_diff > balance_check {
                    return match signers.get(&item.signer_public()) {
                        Some(details) if item.cost() > details.balance => Some(*hash),
                        _ => None,
                    }
                }

                None
            })
            .collect::<Vec<_>>();
        let fetch_seq =
            |a: &Public| signers.get(a).expect("We fetch details for all signers from both current and future").seq;
        self.remove(&invalid, &fetch_seq, current_block_number, current_timestamp);
    }

    // Recover MemPool state from db stored data
    pub fn recover_from_db<C: AccountData + BlockChainTrait>(&mut self, client: &C) {
        let fetch_account = fetch_account_creator(client);
        let by_hash = backup::recover_to_data(self.db.as_ref());

        let recover_block_number = client.chain_info().best_block_number;
        let recover_timestamp = client.chain_info().best_block_timestamp;

        let mut max_insertion_id = 0u64;
        let mut to_insert: HashMap<_, Vec<_>> = HashMap::new();

        for (hash, item) in by_hash.iter() {
            let signer_public = item.signer_public();
            let seq = item.seq();
            let client_account = fetch_account(&signer_public);

            if item.insertion_id > max_insertion_id {
                max_insertion_id = item.insertion_id;
            }

            let order = TransactionOrder::for_transaction(&item, client_account.seq);
            let order_with_tag = TransactionOrderWithTag::new(order, QueueTag::New);

            self.by_hash.insert((*hash).into(), item.clone());

            self.by_signer_public.insert(signer_public, seq, order_with_tag);
            if item.origin == TxOrigin::Local {
                self.is_local_account.insert(signer_public);
            }
            to_insert.entry(signer_public).or_default().push(seq);
        }

        let keys = self.by_signer_public.keys().map(Clone::clone).collect::<Vec<_>>();

        for public in keys {
            let current_seq = fetch_account(&public).seq;
            let next_seq = self.next_seq_of_queued(public, current_seq);

            self.first_seqs.insert(public, current_seq);
            if next_seq > current_seq {
                self.next_seqs.insert(public, next_seq);
            }

            if let Some(seq_list) = to_insert.get(&public) {
                self.add_new_orders_to_queue(public, seq_list, next_seq);
            }

            if self.by_signer_public.clear_if_empty(&public) {
                self.is_local_account.remove(&public);
            }
        }
        // last_block_number and last_timestamp don't have to be the same as previous mem_pool state.
        // These values are used only to optimize the renewal behavior of next seq and first seq.
        self.last_block_number = recover_block_number;
        self.last_timestamp = recover_timestamp;
        self.next_transaction_id = max_insertion_id + 1;
    }

    /// Removes invalid transaction identified by hash from pool.
    /// Assumption is that this transaction seq is not related to client seq,
    /// so transactions left in pool are processed according to client seq.
    ///
    /// If gap is introduced marks subsequent transactions as future
    pub fn remove<F>(
        &mut self,
        transaction_hashes: &[TxHash],
        fetch_seq: &F,
        current_block_number: PoolingInstant,
        current_timestamp: u64,
    ) where
        F: Fn(&Public) -> u64, {
        ctrace!(MEM_POOL, "remove() called, time: {}, timestamp: {}", current_block_number, current_timestamp);
        let mut removed: HashMap<_, _> = HashMap::new();
        let mut batch = backup::backup_batch_with_capacity(transaction_hashes.len());

        for hash in transaction_hashes {
            if let Some(item) = self.by_hash.get(hash).map(Clone::clone) {
                let signer_public = item.signer_public();
                let seq = item.seq();
                let current_seq = fetch_seq(&signer_public);

                let order_with_tag = *self
                    .by_signer_public
                    .get(&signer_public, &seq)
                    .expect("`by_hash` and `by_signer_public` must be synced");
                let order = order_with_tag.order;
                match order_with_tag.tag {
                    QueueTag::Current => self.current.remove(&order),
                    QueueTag::Future => self.future.remove(&order),
                    QueueTag::New => unreachable!(),
                }

                self.by_hash.remove(hash);
                backup::remove_item(&mut batch, hash);
                self.by_signer_public.remove(&signer_public, &seq);
                if current_seq <= seq {
                    let old = removed.get(&signer_public).map(Clone::clone);
                    match old {
                        Some(old_seq) if old_seq <= seq => {}
                        _ => {
                            removed.insert(signer_public, seq);
                        }
                    }
                }
            }
        }

        let keys = self.by_signer_public.keys().map(Clone::clone).collect::<Vec<_>>();

        for public in keys {
            let current_seq = fetch_seq(&public);
            let mut first_seq = *self.first_seqs.get(&public).unwrap_or(&0);
            let next_seq = self.next_seqs.get(&public).cloned().unwrap_or(current_seq);

            let new_next_seq = if current_seq < first_seq
                || current_block_number < self.last_block_number
                || current_timestamp < self.last_timestamp
                || next_seq < current_seq
            {
                self.next_seq_of_queued(public, current_seq)
            } else if let Some(seq) = removed.get(&public) {
                *seq
            } else {
                self.next_seq_of_queued(public, next_seq)
            };

            // Need to update the height
            if current_seq != first_seq {
                self.update_orders(public, current_seq, new_next_seq, false, &mut batch);
                self.first_seqs.insert(public, current_seq);
                first_seq = current_seq;
            }
            // We don't need to update the height, just move transactions
            else if new_next_seq < next_seq {
                self.move_queue(public, new_next_seq, next_seq, QueueTag::Future);
            } else if new_next_seq > next_seq {
                self.move_queue(public, next_seq, new_next_seq, QueueTag::Current);
            }

            if new_next_seq <= first_seq {
                self.next_seqs.remove(&public);
            } else {
                self.next_seqs.insert(public, new_next_seq);
            }

            if self.by_signer_public.clear_if_empty(&public) {
                self.is_local_account.remove(&public);
            }
        }

        self.last_block_number = current_block_number;
        self.last_timestamp = current_timestamp;

        assert_eq!(self.current.len() + self.future.len(), self.by_hash.len());
        assert_eq!(self.current.fee_counter.values().sum::<usize>(), self.current.len());
        assert_eq!(self.by_signer_public.len(), self.by_hash.len());

        self.db.write(batch).expect("Low level database error. Some issue with disk?");
    }

    /// Returns the next seq of the last transaction which can be in the current queue
    fn next_seq_of_queued(&self, public: Public, start_seq: u64) -> u64 {
        let row = self
            .by_signer_public
            .row(&public)
            .expect("This function should be called after checking from `self.by_signer_public.keys()`");

        (start_seq..).find(|s| row.get(s).is_none()).expect("Open ended range does not end")
    }

    /// Moves the transactions which of seq is in [start_seq, end_seq -1],
    /// to the given queue `to`.
    fn move_queue(&mut self, public: Public, mut start_seq: u64, end_seq: u64, to: QueueTag) {
        let row = self
            .by_signer_public
            .row_mut(&public)
            .expect("This function should be called after checking from `self.by_signer_public.keys()`");

        while start_seq < end_seq {
            if let Some(order_with_tag) = row.get_mut(&start_seq) {
                let tag = order_with_tag.tag;
                match tag {
                    QueueTag::Current if to == QueueTag::Future => {
                        let order = order_with_tag.order;
                        order_with_tag.tag = QueueTag::Future;
                        self.current.remove(&order);
                        self.future.insert(order);
                    }
                    QueueTag::Future if to == QueueTag::Current => {
                        let order = order_with_tag.order;
                        order_with_tag.tag = QueueTag::Current;
                        self.future.remove(&order);
                        self.current.insert(order);
                    }
                    _ => {}
                }
            }
            start_seq += 1;
        }
    }

    /// Add the given transactions to the corresponding queue.
    /// It should be tagged as QueueTag::New in self.by_signer_public.
    fn add_new_orders_to_queue(&mut self, public: Public, seq_list: &[u64], new_next_seq: u64) {
        let row = self
            .by_signer_public
            .row_mut(&public)
            .expect("This function should be called after checking from `self.by_signer_public.keys()`");

        for seq in seq_list {
            let order_with_tag = row.get_mut(seq).expect("Must exist");
            let tag = order_with_tag.tag;
            match tag {
                QueueTag::New => {
                    let order = order_with_tag.order;
                    if *seq < new_next_seq {
                        order_with_tag.tag = QueueTag::Current;
                        self.current.insert(order);
                    } else {
                        order_with_tag.tag = QueueTag::Future;
                        self.future.insert(order);
                    }
                }
                _ => unreachable!(),
            }
        }
    }

    /// Updates the seq height of the orders in the queues and self.by_signer_public.
    /// Also, drops old transactions.
    fn update_orders(
        &mut self,
        public: Public,
        current_seq: u64,
        new_next_seq: u64,
        to_local: bool,
        batch: &mut DBTransaction,
    ) {
        let row = self
            .by_signer_public
            .row_mut(&public)
            .expect("This function should be called after checking from `self.by_signer_public.keys()`");

        let seqs = row.keys().map(Clone::clone).collect::<Vec<_>>();

        for seq in seqs {
            let order_with_tag = *row.get(&seq).expect("Must exist");
            let old_order = order_with_tag.order;

            // Remove old order
            match order_with_tag.tag {
                QueueTag::Current => self.current.remove(&old_order),
                QueueTag::Future => self.future.remove(&old_order),
                QueueTag::New => continue,
            }
            row.remove(&seq);

            if seq < current_seq {
                self.by_hash.remove(&old_order.hash);
                backup::remove_item(batch, &old_order.hash);
            } else {
                let new_order = old_order.update_height(seq, current_seq);
                let new_order = if to_local {
                    new_order.change_origin(TxOrigin::Local)
                } else {
                    new_order
                };
                if seq < new_next_seq {
                    let new_order_with_tag = TransactionOrderWithTag::new(new_order, QueueTag::Current);
                    self.current.insert(new_order);
                    row.insert(seq, new_order_with_tag);
                } else {
                    let new_order_with_tag = TransactionOrderWithTag::new(new_order, QueueTag::Future);
                    self.future.insert(new_order);
                    row.insert(seq, new_order_with_tag);
                }
            }
        }
    }

    /// Verify signed transaction with its content.
    /// This function can return errors: InsufficientFee, InsufficientBalance,
    /// TransactionAlreadyImported, Old, TooCheapToReplace
    fn verify_transaction(
        &self,
        tx: &VerifiedTransaction,
        origin: TxOrigin,
        client_account: &AccountDetails,
    ) -> Result<(), Error> {
        let full_pools_lowest = self.effective_minimum_fee();
        if origin != TxOrigin::Local && tx.transaction().fee < full_pools_lowest {
            ctrace!(
                MEM_POOL,
                "Dropping transaction below lowest fee in a full pool: {:?} (gp: {} < {})",
                tx.hash(),
                tx.transaction().fee,
                full_pools_lowest
            );

            return Err(SyntaxError::InsufficientFee {
                minimal: full_pools_lowest,
                got: tx.transaction().fee,
            }
            .into())
        }

        if client_account.balance < tx.transaction().fee {
            ctrace!(
                MEM_POOL,
                "Dropping transaction without sufficient balance: {:?} ({} < {})",
                tx.hash(),
                client_account.balance,
                tx.transaction().fee
            );

            return Err(RuntimeError::InsufficientBalance {
                pubkey: tx.signer_public(),
                cost: tx.transaction().fee,
                balance: client_account.balance,
            }
            .into())
        }

        if self.by_hash.get(&tx.hash()).is_some() {
            ctrace!(MEM_POOL, "Dropping already imported transaction: {:?}", tx.hash());
            return Err(HistoryError::TransactionAlreadyImported.into())
        }

        if tx.transaction().seq < client_account.seq {
            ctrace!(
                MEM_POOL,
                "Dropping old transaction: {:?} (seq: {} < {})",
                tx.hash(),
                tx.transaction().seq,
                client_account.seq
            );
            return Err(HistoryError::Old.into())
        }

        if origin != TxOrigin::Local {
            if let Some(TransactionOrderWithTag {
                order,
                ..
            }) = self.by_signer_public.get(&tx.signer_public(), &tx.transaction().seq)
            {
                let old_fee = order.fee;
                let new_fee = tx.transaction().fee;
                let min_required_fee = old_fee + (old_fee >> self.fee_bump_shift);

                if new_fee < min_required_fee {
                    ctrace!(
                        MEM_POOL,
                        "Dropping transaction because fee is not enough to replace: {:?} (gp: {} < {}) (old_fee: {})",
                        tx.hash(),
                        new_fee,
                        min_required_fee,
                        old_fee,
                    );
                    return Err(HistoryError::TooCheapToReplace.into())
                }
            }
        }

        Ok(())
    }

    /// Returns top transactions whose timestamp are in the given range from the pool ordered by priority.
    // FIXME: current_timestamp should be `u64`, not `Option<u64>`.
    // FIXME: if range_contains becomes stable, use range.contains instead of inequality.
    pub fn top_transactions(&self, size_limit: usize, range: Range<u64>) -> PendingVerifiedTransactions {
        let mut current_size: usize = 0;
        let pending_items: Vec<_> = self
            .current
            .queue
            .iter()
            .map(|t| {
                self.by_hash
                    .get(&t.hash)
                    .expect("All transactions in `current` and `future` are always included in `by_hash`")
            })
            .filter(|t| range.contains(&t.inserted_timestamp))
            .take_while(|t| {
                let encoded_byte_array = rlp::encode(&t.tx);
                let size_in_byte = encoded_byte_array.len();
                current_size += size_in_byte;
                current_size < size_limit
            })
            .collect();

        let transactions = pending_items.iter().map(|t| t.tx.clone()).collect();
        let last_timestamp = pending_items.into_iter().map(|t| t.inserted_timestamp).max();

        PendingVerifiedTransactions {
            transactions,
            last_timestamp,
        }
    }

    /// Return all transactions whose timestamp are in the given range in the memory pool.
    pub fn count_pending_transactions(&self, range: Range<u64>) -> usize {
        self.current
            .queue
            .iter()
            .map(|t| {
                self.by_hash
                    .get(&t.hash)
                    .expect("All transactions in `current` and `future` are always included in `by_hash`")
            })
            .filter(|t| range.contains(&t.inserted_timestamp))
            .count()
    }

    pub fn future_included_count_pending_transactions(&self, range: Range<u64>) -> usize {
        self.future
            .queue
            .iter()
            .map(|t| {
                self.by_hash
                    .get(&t.hash)
                    .expect("All transactions in `current` and `future` are always included in `by_hash`")
            })
            .filter(|t| range.contains(&t.inserted_timestamp))
            .count()
            + self
                .current
                .queue
                .iter()
                .map(|t| {
                    self.by_hash
                        .get(&t.hash)
                        .expect("All transactions in `current` and `future` are always included in `by_hash`")
                })
                .filter(|t| range.contains(&t.inserted_timestamp))
                .count()
    }

    /// Return all future transactions along with current transactions.
    pub fn get_future_pending_transactions(&self, size_limit: usize, range: Range<u64>) -> PendingVerifiedTransactions {
        let mut current_size: usize = 0;
        let pending_items: Vec<_> = self
            .current
            .queue
            .iter()
            .map(|t| {
                self.by_hash
                    .get(&t.hash)
                    .expect("All transactions in `current` and `future` are always included in `by_hash`")
            })
            .filter(|t| range.contains(&t.inserted_timestamp))
            .take_while(|t| {
                let encoded_byte_array = rlp::encode(&t.tx);
                let size_in_byte = encoded_byte_array.len();
                current_size += size_in_byte;
                current_size < size_limit
            })
            .collect();
        let future_pending_items: Vec<_> = self
            .future
            .queue
            .iter()
            .map(|t| {
                self.by_hash
                    .get(&t.hash)
                    .expect("All transactions in `current` and `future` are always included in `by_hash`")
            })
            .filter(|t| range.contains(&t.inserted_timestamp))
            .take_while(|t| {
                let encoded_byte_array = rlp::encode(&t.tx);
                let size_in_byte = encoded_byte_array.len();
                current_size += size_in_byte;
                current_size < size_limit
            })
            .collect();
        let mut current_signed_tx: Vec<VerifiedTransaction> = pending_items.iter().map(|t| t.tx.clone()).collect();
        let current_last_timestamp = pending_items.into_iter().map(|t| t.inserted_timestamp).max();
        let mut future_signed_tx: Vec<VerifiedTransaction> =
            future_pending_items.iter().map(|t| t.tx.clone()).collect();
        current_signed_tx.append(&mut future_signed_tx);
        let transactions: Vec<VerifiedTransaction> = current_signed_tx;
        let future_last_timestamp = future_pending_items.into_iter().map(|t| t.inserted_timestamp).max();
        let last_timestamp = max(current_last_timestamp, future_last_timestamp);
        PendingVerifiedTransactions {
            transactions,
            last_timestamp,
        }
    }

    pub fn future_transactions(&self) -> Vec<VerifiedTransaction> {
        self.future
            .queue
            .iter()
            .map(|t| {
                self.by_hash
                    .get(&t.hash)
                    .expect("All transactions in `current` and `future` are always included in `by_hash`")
            })
            .map(|t| t.tx.clone())
            .collect()
    }
}

#[cfg(test)]
pub mod test {
    use crate::transaction::UnverifiedTransaction;
    use std::cmp::Ordering;

    use crate::client::{AccountData, TestBlockChainClient};
    use ckey::{Ed25519KeyPair as KeyPair, Generator, KeyPairTrait, Random};
    use ctypes::transaction::{Action, Transaction};

    use super::backup::MemPoolItemProjection;
    use super::*;
    use rlp::{rlp_encode_and_decode_test, Rlp};
    use std::convert::TryInto;

    #[test]
    fn origin_ordering() {
        assert_eq!(TxOrigin::Local.cmp(&TxOrigin::External), Ordering::Less);
        assert_eq!(TxOrigin::External.cmp(&TxOrigin::Local), Ordering::Greater);
    }

    #[test]
    fn pay_transaction_increases_cost() {
        let fee = 100;
        let quantity = 100_000;
        let receiver = 1u64.into();
        let keypair: KeyPair = Random.generate().unwrap();
        let tx = Transaction {
            seq: 0,
            fee,
            network_id: "tc".into(),
            action: Action::Pay {
                receiver,
                quantity,
            },
        };
        let signed = VerifiedTransaction::new_with_sign(tx, keypair.private());
        let item = MemPoolItem::new(signed, TxOrigin::Local, 0, 0, 0);

        assert_eq!(fee + quantity, item.cost());
    }

    #[test]
    fn txorigin_encode_and_decode() {
        rlp_encode_and_decode_test!(TxOrigin::External);
    }

    #[test]
    fn signed_transaction_encode_and_decode() {
        let receiver = 0u64.into();
        let keypair: KeyPair = Random.generate().unwrap();
        let tx = Transaction {
            seq: 0,
            fee: 100,
            network_id: "tc".into(),
            action: Action::Pay {
                receiver,
                quantity: 100_000,
            },
        };
        let signed = VerifiedTransaction::new_with_sign(tx, keypair.private());

        let rlp = rlp::encode(&signed);
        let encoded = Rlp::new(&rlp);
        let decoded: UnverifiedTransaction = encoded.as_val().unwrap();
        let result = decoded.try_into().unwrap();

        assert_eq!(signed, result);
    }

    #[test]
    fn mempool_item_encode_and_decode() {
        let keypair: KeyPair = Random.generate().unwrap();
        let tx = Transaction {
            seq: 0,
            fee: 10,
            network_id: "tc".into(),
            action: Action::Pay {
                receiver: Default::default(),
                quantity: 0,
            },
        };
        let signed = VerifiedTransaction::new_with_sign(tx, keypair.private());
        let item = MemPoolItem::new(signed, TxOrigin::Local, 0, 0, 0);

        let rlp = rlp::encode(&item);
        let encoded = Rlp::new(&rlp);
        let decoded: MemPoolItemProjection = encoded.as_val().unwrap();
        let result = decoded.try_into().unwrap();

        assert_eq!(item, result);
    }

    #[test]
    fn db_backup_and_recover() {
        //setup test_client
        let test_client = TestBlockChainClient::new();
        let keypair: KeyPair = Random.generate().unwrap();
        let default_addr = keypair.public();
        test_client.set_seq(*default_addr, 4u64);
        test_client.set_balance(*default_addr, u64::max_value());

        let db = Arc::new(kvdb_memorydb::create(crate::db::NUM_COLUMNS.unwrap_or(0)));
        let mut mem_pool = MemPool::with_limits(8192, usize::max_value(), 3, db.clone());

        let fetch_account = fetch_account_creator(&test_client);

        let inserted_block_number = 1;
        let inserted_timestamp = 100;
        let mut inputs: Vec<MemPoolInput> = Vec::new();

        inputs.push(create_mempool_input_with_pay(1u64, &keypair));
        inputs.push(create_mempool_input_with_pay(3u64, &keypair));
        inputs.push(create_mempool_input_with_pay(5u64, &keypair));
        mem_pool.add(inputs, inserted_block_number, inserted_timestamp, &fetch_account);

        let inserted_block_number = 11;
        let inserted_timestamp = 200;
        let mut inputs: Vec<MemPoolInput> = Vec::new();
        inputs.push(create_mempool_input_with_pay(2u64, &keypair));
        inputs.push(create_mempool_input_with_pay(4u64, &keypair));
        mem_pool.add(inputs, inserted_block_number, inserted_timestamp, &fetch_account);

        let inserted_block_number = 20;
        let inserted_timestamp = 300;
        let mut inputs: Vec<MemPoolInput> = Vec::new();
        inputs.push(create_mempool_input_with_pay(6u64, &keypair));
        inputs.push(create_mempool_input_with_pay(8u64, &keypair));
        inputs.push(create_mempool_input_with_pay(10u64, &keypair));
        mem_pool.add(inputs, inserted_block_number, inserted_timestamp, &fetch_account);

        let inserted_block_number = 21;
        let inserted_timestamp = 400;
        let mut inputs: Vec<MemPoolInput> = Vec::new();
        inputs.push(create_mempool_input_with_pay(7u64, &keypair));
        mem_pool.add(inputs, inserted_block_number, inserted_timestamp, &fetch_account);

        let mut mem_pool_recovered = MemPool::with_limits(8192, usize::max_value(), 3, db);
        mem_pool_recovered.recover_from_db(&test_client);

        assert_eq!(mem_pool_recovered.first_seqs, mem_pool.first_seqs);
        assert_eq!(mem_pool_recovered.next_seqs, mem_pool.next_seqs);
        assert_eq!(mem_pool_recovered.by_signer_public, mem_pool.by_signer_public);
        assert_eq!(mem_pool_recovered.is_local_account, mem_pool.is_local_account);
        assert_eq!(mem_pool_recovered.next_transaction_id, mem_pool.next_transaction_id);
        assert_eq!(mem_pool_recovered.by_hash, mem_pool.by_hash);
        assert_eq!(mem_pool_recovered.queue_count_limit, mem_pool.queue_count_limit);
        assert_eq!(mem_pool_recovered.queue_memory_limit, mem_pool.queue_memory_limit);
        assert_eq!(mem_pool_recovered.current, mem_pool.current);
        assert_eq!(mem_pool_recovered.future, mem_pool.future);
    }

    fn create_signed_pay(seq: u64, keypair: &KeyPair) -> VerifiedTransaction {
        let receiver = 1u64.into();
        let tx = Transaction {
            seq,
            fee: 100,
            network_id: "tc".into(),
            action: Action::Pay {
                receiver,
                quantity: 100_000,
            },
        };
        VerifiedTransaction::new_with_sign(tx, keypair.private())
    }

    fn create_mempool_input_with_pay(seq: u64, keypair: &KeyPair) -> MemPoolInput {
        let signed = create_signed_pay(seq, &keypair);
        MemPoolInput::new(signed, TxOrigin::Local)
    }

    #[test]
    fn transactions_are_moved_to_future_queue_if_the_preceding_one_removed() {
        //setup test_client
        let test_client = TestBlockChainClient::new();

        let db = Arc::new(kvdb_memorydb::create(crate::db::NUM_COLUMNS.unwrap_or(0)));
        let mut mem_pool = MemPool::with_limits(8192, usize::max_value(), 3, db);

        let fetch_account = fetch_account_creator(&test_client);
        let keypair: KeyPair = Random.generate().unwrap();
        let pubkey = keypair.public();
        test_client.set_balance(*pubkey, 1_000_000_000_000);
        assert_eq!(1_000_000_000_000, test_client.latest_balance(&pubkey));

        let inserted_block_number = 1;
        let inserted_timestamp = 100;
        let inputs = vec![
            create_mempool_input_with_pay(0, &keypair),
            create_mempool_input_with_pay(1, &keypair),
            create_mempool_input_with_pay(2, &keypair),
        ];
        let result = mem_pool.add(inputs, inserted_block_number, inserted_timestamp, &fetch_account);
        assert_eq!(
            vec![
                Ok(TransactionImportResult::Current),
                Ok(TransactionImportResult::Current),
                Ok(TransactionImportResult::Current)
            ],
            result
        );

        assert_eq!(
            vec![create_signed_pay(0, &keypair), create_signed_pay(1, &keypair), create_signed_pay(2, &keypair),],
            mem_pool.top_transactions(std::usize::MAX, 0..std::u64::MAX).transactions
        );

        assert_eq!(Vec::<VerifiedTransaction>::default(), mem_pool.future_transactions());

        let best_block_number = test_client.chain_info().best_block_number;
        let best_block_timestamp = test_client.chain_info().best_block_timestamp;
        let fetch_seq = |p: &Public| -> u64 { test_client.latest_seq(&p) };
        mem_pool.remove(&[create_signed_pay(1, &keypair).hash()], &fetch_seq, best_block_number, best_block_timestamp);

        assert_eq!(
            vec![create_signed_pay(0, &keypair),],
            mem_pool.top_transactions(std::usize::MAX, 0..std::u64::MAX).transactions
        );

        assert_eq!(vec![create_signed_pay(2, &keypair),], mem_pool.future_transactions());
    }
}
