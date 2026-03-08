use crate::storage_trait::{Storage, StorageError, TxId};
use bincode;
#[cfg(feature = "rocksdb")]
use rocksdb::{ColumnFamilyDescriptor, Options, DB};
#[cfg(feature = "rocksdb")]
use std::path::Path;
#[cfg(feature = "rocksdb")]
use std::sync::Arc;
use zkclear_state::State;
use zkclear_types::{Block, BlockId, Deal, DealId, Tx};

#[cfg(feature = "rocksdb")]
const CF_BLOCKS: &str = "blocks";
#[cfg(feature = "rocksdb")]
const CF_TRANSACTIONS: &str = "transactions";
#[cfg(feature = "rocksdb")]
const CF_DEALS: &str = "deals";
#[cfg(feature = "rocksdb")]
const CF_STATE_SNAPSHOTS: &str = "state_snapshots";
#[cfg(feature = "rocksdb")]
const CF_METADATA: &str = "metadata";

#[cfg(feature = "rocksdb")]
pub struct RocksDBStorage {
    db: Arc<DB>,
}

#[cfg(feature = "rocksdb")]
impl RocksDBStorage {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.create_missing_column_families(true);

        let cfs = vec![
            ColumnFamilyDescriptor::new(CF_BLOCKS, Options::default()),
            ColumnFamilyDescriptor::new(CF_TRANSACTIONS, Options::default()),
            ColumnFamilyDescriptor::new(CF_DEALS, Options::default()),
            ColumnFamilyDescriptor::new(CF_STATE_SNAPSHOTS, Options::default()),
            ColumnFamilyDescriptor::new(CF_METADATA, Options::default()),
        ];

        let db = DB::open_cf_descriptors(&opts, path, cfs)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        Ok(Self { db: Arc::new(db) })
    }

    fn encode_block_id(block_id: BlockId) -> Vec<u8> {
        block_id.to_le_bytes().to_vec()
    }

    fn decode_block_id(bytes: &[u8]) -> Result<BlockId, StorageError> {
        if bytes.len() != 8 {
            return Err(StorageError::DeserializationFailed);
        }
        let mut arr = [0u8; 8];
        arr.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(arr))
    }

    fn encode_tx_id(tx_id: TxId) -> Vec<u8> {
        let mut key = Vec::with_capacity(16);
        key.extend_from_slice(&tx_id.0.to_le_bytes());
        key.extend_from_slice(&tx_id.1.to_le_bytes());
        key
    }
}

#[cfg(feature = "rocksdb")]
impl Storage for RocksDBStorage {
    fn save_block(&self, block: &Block) -> Result<(), StorageError> {
        let cf = self
            .db
            .cf_handle(CF_BLOCKS)
            .ok_or_else(|| StorageError::DatabaseError("CF_BLOCKS not found".to_string()))?;

        let key = Self::encode_block_id(block.id);
        let value = bincode::serialize(block).map_err(|_| StorageError::SerializationFailed)?;

        self.db
            .put_cf(cf, key, value)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        let metadata_cf = self
            .db
            .cf_handle(CF_METADATA)
            .ok_or_else(|| StorageError::DatabaseError("CF_METADATA not found".to_string()))?;

        self.db
            .put_cf(
                metadata_cf,
                b"latest_block_id",
                Self::encode_block_id(block.id),
            )
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        for (index, tx) in block.transactions.iter().enumerate() {
            self.save_transaction(tx, block.id, index)?;
        }

        Ok(())
    }

    fn get_block(&self, block_id: BlockId) -> Result<Option<Block>, StorageError> {
        let cf = self
            .db
            .cf_handle(CF_BLOCKS)
            .ok_or_else(|| StorageError::DatabaseError("CF_BLOCKS not found".to_string()))?;

        let key = Self::encode_block_id(block_id);
        match self
            .db
            .get_cf(cf, key)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?
        {
            Some(bytes) => {
                let block: Block = bincode::deserialize(&bytes[..][..])
                    .map_err(|_| StorageError::DeserializationFailed)?;
                Ok(Some(block))
            }
            None => Ok(None),
        }
    }

    fn get_latest_block_id(&self) -> Result<Option<BlockId>, StorageError> {
        let cf = self
            .db
            .cf_handle(CF_METADATA)
            .ok_or_else(|| StorageError::DatabaseError("CF_METADATA not found".to_string()))?;

        match self
            .db
            .get_cf(cf, b"latest_block_id")
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?
        {
            Some(bytes) => Ok(Some(Self::decode_block_id(&bytes)?)),
            None => Ok(None),
        }
    }

    fn save_transaction(
        &self,
        tx: &Tx,
        block_id: BlockId,
        index: usize,
    ) -> Result<(), StorageError> {
        let cf = self
            .db
            .cf_handle(CF_TRANSACTIONS)
            .ok_or_else(|| StorageError::DatabaseError("CF_TRANSACTIONS not found".to_string()))?;

        let key = Self::encode_tx_id((block_id, index));
        let value = bincode::serialize(tx).map_err(|_| StorageError::SerializationFailed)?;

        self.db
            .put_cf(cf, key, value)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    fn get_transaction(&self, block_id: BlockId, index: usize) -> Result<Option<Tx>, StorageError> {
        let cf = self
            .db
            .cf_handle(CF_TRANSACTIONS)
            .ok_or_else(|| StorageError::DatabaseError("CF_TRANSACTIONS not found".to_string()))?;

        let key = Self::encode_tx_id((block_id, index));
        match self
            .db
            .get_cf(cf, key)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?
        {
            Some(bytes) => {
                let tx: Tx = bincode::deserialize(&bytes[..])
                    .map_err(|_| StorageError::DeserializationFailed)?;
                Ok(Some(tx))
            }
            None => Ok(None),
        }
    }

    fn get_transactions_by_block(&self, block_id: BlockId) -> Result<Vec<Tx>, StorageError> {
        let cf = self
            .db
            .cf_handle(CF_TRANSACTIONS)
            .ok_or_else(|| StorageError::DatabaseError("CF_TRANSACTIONS not found".to_string()))?;

        let prefix = Self::encode_block_id(block_id);
        let mut txs = Vec::new();
        let iter = self.db.iterator_cf(
            cf,
            rocksdb::IteratorMode::From(&prefix, rocksdb::Direction::Forward),
        );

        for item in iter {
            let (key, value) = item.map_err(|e| StorageError::DatabaseError(e.to_string()))?;
            if key.len() < 8 || &key[0..8] != prefix {
                break;
            }
            let tx: Tx = bincode::deserialize(&value[..])
                .map_err(|_| StorageError::DeserializationFailed)?;
            txs.push(tx);
        }

        Ok(txs)
    }

    fn save_deal(&self, deal: &Deal) -> Result<(), StorageError> {
        let cf = self
            .db
            .cf_handle(CF_DEALS)
            .ok_or_else(|| StorageError::DatabaseError("CF_DEALS not found".to_string()))?;

        let key = deal.id.to_le_bytes().to_vec();
        let value = bincode::serialize(deal).map_err(|_| StorageError::SerializationFailed)?;

        self.db
            .put_cf(cf, key, value)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    fn get_deal(&self, deal_id: DealId) -> Result<Option<Deal>, StorageError> {
        let cf = self
            .db
            .cf_handle(CF_DEALS)
            .ok_or_else(|| StorageError::DatabaseError("CF_DEALS not found".to_string()))?;

        let key = deal_id.to_le_bytes().to_vec();
        match self
            .db
            .get_cf(cf, key)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?
        {
            Some(bytes) => {
                let deal: Deal = bincode::deserialize(&bytes[..])
                    .map_err(|_| StorageError::DeserializationFailed)?;
                Ok(Some(deal))
            }
            None => Ok(None),
        }
    }

    fn get_all_deals(&self) -> Result<Vec<Deal>, StorageError> {
        let cf = self
            .db
            .cf_handle(CF_DEALS)
            .ok_or_else(|| StorageError::DatabaseError("CF_DEALS not found".to_string()))?;

        let mut deals = Vec::new();
        let iter = self.db.iterator_cf(cf, rocksdb::IteratorMode::Start);

        for item in iter {
            let (_, value) = item.map_err(|e| StorageError::DatabaseError(e.to_string()))?;
            let deal: Deal = bincode::deserialize(&value[..])
                .map_err(|_| StorageError::DeserializationFailed)?;
            deals.push(deal);
        }

        Ok(deals)
    }

    fn save_state_snapshot(&self, state: &State, block_id: BlockId) -> Result<(), StorageError> {
        let cf = self.db.cf_handle(CF_STATE_SNAPSHOTS).ok_or_else(|| {
            StorageError::DatabaseError("CF_STATE_SNAPSHOTS not found".to_string())
        })?;

        let key = Self::encode_block_id(block_id);
        let value = bincode::serialize(state).map_err(|_| StorageError::SerializationFailed)?;

        self.db
            .put_cf(cf, key, value)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        let metadata_cf = self
            .db
            .cf_handle(CF_METADATA)
            .ok_or_else(|| StorageError::DatabaseError("CF_METADATA not found".to_string()))?;

        self.db
            .put_cf(
                metadata_cf,
                b"latest_state_snapshot_block_id",
                Self::encode_block_id(block_id),
            )
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    fn get_latest_state_snapshot(&self) -> Result<Option<(State, BlockId)>, StorageError> {
        let metadata_cf = self
            .db
            .cf_handle(CF_METADATA)
            .ok_or_else(|| StorageError::DatabaseError("CF_METADATA not found".to_string()))?;

        let snapshot_block_id = match self
            .db
            .get_cf(metadata_cf, b"latest_state_snapshot_block_id")
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?
        {
            Some(bytes) => Self::decode_block_id(&bytes)?,
            None => return Ok(None),
        };

        let cf = self.db.cf_handle(CF_STATE_SNAPSHOTS).ok_or_else(|| {
            StorageError::DatabaseError("CF_STATE_SNAPSHOTS not found".to_string())
        })?;

        let key = Self::encode_block_id(snapshot_block_id);
        match self
            .db
            .get_cf(cf, key)
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?
        {
            Some(bytes) => {
                let state: State = bincode::deserialize(&bytes[..])
                    .map_err(|_| StorageError::DeserializationFailed)?;
                Ok(Some((state, snapshot_block_id)))
            }
            None => Ok(None),
        }
    }

    fn flush(&self) -> Result<(), StorageError> {
        self.db
            .flush()
            .map_err(|e| StorageError::DatabaseError(e.to_string()))?;
        Ok(())
    }
}
