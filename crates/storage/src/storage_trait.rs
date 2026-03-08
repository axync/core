use zkclear_state::State;
use zkclear_types::{Block, BlockId, Deal, DealId, Tx};

#[derive(Debug)]
pub enum StorageError {
    NotFound,
    SerializationFailed,
    DeserializationFailed,
    DatabaseError(String),
    IOError(String),
}

pub trait Storage: Send + Sync {
    fn save_block(&self, block: &Block) -> Result<(), StorageError>;
    fn get_block(&self, block_id: BlockId) -> Result<Option<Block>, StorageError>;
    fn get_latest_block_id(&self) -> Result<Option<BlockId>, StorageError>;

    fn save_transaction(
        &self,
        tx: &Tx,
        block_id: BlockId,
        index: usize,
    ) -> Result<(), StorageError>;
    fn get_transaction(&self, block_id: BlockId, index: usize) -> Result<Option<Tx>, StorageError>;
    fn get_transactions_by_block(&self, block_id: BlockId) -> Result<Vec<Tx>, StorageError>;

    fn save_deal(&self, deal: &Deal) -> Result<(), StorageError>;
    fn get_deal(&self, deal_id: DealId) -> Result<Option<Deal>, StorageError>;
    fn get_all_deals(&self) -> Result<Vec<Deal>, StorageError>;

    fn save_state_snapshot(&self, state: &State, block_id: BlockId) -> Result<(), StorageError>;
    fn get_latest_state_snapshot(&self) -> Result<Option<(State, BlockId)>, StorageError>;

    fn flush(&self) -> Result<(), StorageError>;
}

pub type TxId = (BlockId, usize);
