use crate::storage_trait::{Storage, StorageError, TxId};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use zkclear_state::State;
use zkclear_types::{Block, BlockId, Deal, DealId, Tx};

pub struct InMemoryStorage {
    blocks: Arc<RwLock<HashMap<BlockId, Block>>>,
    transactions: Arc<RwLock<HashMap<TxId, Tx>>>,
    deals: Arc<RwLock<HashMap<DealId, Deal>>>,
    state_snapshots: Arc<RwLock<HashMap<BlockId, State>>>,
    latest_block_id: Arc<RwLock<Option<BlockId>>>,
}

impl InMemoryStorage {
    pub fn new() -> Self {
        Self {
            blocks: Arc::new(RwLock::new(HashMap::new())),
            transactions: Arc::new(RwLock::new(HashMap::new())),
            deals: Arc::new(RwLock::new(HashMap::new())),
            state_snapshots: Arc::new(RwLock::new(HashMap::new())),
            latest_block_id: Arc::new(RwLock::new(None)),
        }
    }
}

impl Storage for InMemoryStorage {
    fn save_block(&self, block: &Block) -> Result<(), StorageError> {
        let mut blocks = self.blocks.write().unwrap();
        blocks.insert(block.id, block.clone());

        let mut latest = self.latest_block_id.write().unwrap();
        *latest = Some(block.id);

        for (index, tx) in block.transactions.iter().enumerate() {
            self.save_transaction(tx, block.id, index)?;
        }

        Ok(())
    }

    fn get_block(&self, block_id: BlockId) -> Result<Option<Block>, StorageError> {
        let blocks = self.blocks.read().unwrap();
        Ok(blocks.get(&block_id).cloned())
    }

    fn get_latest_block_id(&self) -> Result<Option<BlockId>, StorageError> {
        let latest = self.latest_block_id.read().unwrap();
        Ok(*latest)
    }

    fn save_transaction(
        &self,
        tx: &Tx,
        block_id: BlockId,
        index: usize,
    ) -> Result<(), StorageError> {
        let mut transactions = self.transactions.write().unwrap();
        transactions.insert((block_id, index), tx.clone());
        Ok(())
    }

    fn get_transaction(&self, block_id: BlockId, index: usize) -> Result<Option<Tx>, StorageError> {
        let transactions = self.transactions.read().unwrap();
        Ok(transactions.get(&(block_id, index)).cloned())
    }

    fn get_transactions_by_block(&self, block_id: BlockId) -> Result<Vec<Tx>, StorageError> {
        let transactions = self.transactions.read().unwrap();
        let mut txs: Vec<(usize, Tx)> = transactions
            .iter()
            .filter(|((bid, _), _)| *bid == block_id)
            .map(|((_, idx), tx)| (*idx, tx.clone()))
            .collect();
        txs.sort_by_key(|(idx, _)| *idx);
        Ok(txs.into_iter().map(|(_, tx)| tx).collect())
    }

    fn save_deal(&self, deal: &Deal) -> Result<(), StorageError> {
        let mut deals = self.deals.write().unwrap();
        deals.insert(deal.id, deal.clone());
        Ok(())
    }

    fn get_deal(&self, deal_id: DealId) -> Result<Option<Deal>, StorageError> {
        let deals = self.deals.read().unwrap();
        Ok(deals.get(&deal_id).cloned())
    }

    fn get_all_deals(&self) -> Result<Vec<Deal>, StorageError> {
        let deals = self.deals.read().unwrap();
        Ok(deals.values().cloned().collect())
    }

    fn save_state_snapshot(&self, state: &State, block_id: BlockId) -> Result<(), StorageError> {
        let mut snapshots = self.state_snapshots.write().unwrap();
        snapshots.insert(block_id, state.clone());
        Ok(())
    }

    fn get_latest_state_snapshot(&self) -> Result<Option<(State, BlockId)>, StorageError> {
        let snapshots = self.state_snapshots.read().unwrap();
        let mut latest_block_id = None;
        let mut latest_state = None;

        for (block_id, state) in snapshots.iter() {
            if latest_block_id.is_none() || *block_id > latest_block_id.unwrap() {
                latest_block_id = Some(*block_id);
                latest_state = Some(state.clone());
            }
        }

        Ok(latest_block_id.and_then(|id| latest_state.map(|s| (s, id))))
    }

    fn flush(&self) -> Result<(), StorageError> {
        Ok(())
    }
}

impl Default for InMemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zkclear_types::{
        Address, Deal, DealStatus, DealVisibility, Deposit, Tx, TxKind, TxPayload,
    };

    fn dummy_address(byte: u8) -> Address {
        [byte; 20]
    }

    fn dummy_tx(id: u64, from: Address, nonce: u64) -> Tx {
        Tx {
            id,
            from,
            nonce,
            kind: TxKind::Deposit,
            payload: TxPayload::Deposit(Deposit {
                tx_hash: [0u8; 32],
                account: from,
                asset_id: 0,
                amount: 100,
                chain_id: zkclear_types::chain_ids::ETHEREUM,
            }),
            signature: [0u8; 65],
        }
    }

    fn dummy_block(id: BlockId, tx_count: usize) -> Block {
        let mut transactions = Vec::new();
        let addr = dummy_address(1);
        for i in 0..tx_count {
            transactions.push(dummy_tx(i as u64, addr, i as u64));
        }
        Block {
            id,
            transactions,
            timestamp: 1000,
            state_root: [0u8; 32],
            withdrawals_root: [0u8; 32],
            block_proof: Vec::new(),
        }
    }

    #[test]
    fn test_save_and_get_block() {
        let storage = InMemoryStorage::new();
        let block = dummy_block(0, 3);

        storage.save_block(&block).unwrap();
        let retrieved = storage.get_block(0).unwrap().unwrap();

        assert_eq!(retrieved.id, 0);
        assert_eq!(retrieved.transactions.len(), 3);
        assert_eq!(retrieved.timestamp, 1000);
    }

    #[test]
    fn test_get_nonexistent_block() {
        let storage = InMemoryStorage::new();
        assert!(storage.get_block(999).unwrap().is_none());
    }

    #[test]
    fn test_save_and_get_transaction() {
        let storage = InMemoryStorage::new();
        let tx = dummy_tx(0, dummy_address(1), 0);

        storage.save_transaction(&tx, 0, 0).unwrap();
        let retrieved = storage.get_transaction(0, 0).unwrap().unwrap();

        assert_eq!(retrieved.id, 0);
        assert_eq!(retrieved.from, dummy_address(1));
    }

    #[test]
    fn test_get_transactions_by_block() {
        let storage = InMemoryStorage::new();
        let block = dummy_block(0, 5);

        storage.save_block(&block).unwrap();
        let txs = storage.get_transactions_by_block(0).unwrap();

        assert_eq!(txs.len(), 5);
    }

    #[test]
    fn test_save_and_get_deal() {
        let storage = InMemoryStorage::new();
        let maker = dummy_address(1);
        let deal = Deal {
            id: 42,
            maker,
            taker: None,
            asset_base: 0,
            asset_quote: 1,
            chain_id_base: zkclear_types::chain_ids::ETHEREUM,
            chain_id_quote: zkclear_types::chain_ids::ETHEREUM,
            amount_base: 1000,
            amount_remaining: 1000,
            price_quote_per_base: 100,
            status: DealStatus::Pending,
            visibility: DealVisibility::Public,
            created_at: 1000,
            expires_at: None,
            external_ref: None,
            is_cross_chain: false,
        };

        storage.save_deal(&deal).unwrap();
        let retrieved = storage.get_deal(42).unwrap().unwrap();

        assert_eq!(retrieved.id, 42);
        assert_eq!(retrieved.maker, maker);
        assert_eq!(retrieved.amount_base, 1000);
    }

    #[test]
    fn test_save_and_get_state_snapshot() {
        let storage = InMemoryStorage::new();
        let mut state = State::new();
        let addr = dummy_address(1);
        state.get_or_create_account_by_owner(addr);

        storage.save_state_snapshot(&state, 100).unwrap();
        let (retrieved_state, block_id) = storage.get_latest_state_snapshot().unwrap().unwrap();

        assert_eq!(block_id, 100);
        assert_eq!(retrieved_state.accounts.len(), 1);
    }

    #[test]
    fn test_get_latest_block_id() {
        let storage = InMemoryStorage::new();
        assert!(storage.get_latest_block_id().unwrap().is_none());

        storage.save_block(&dummy_block(0, 1)).unwrap();
        assert_eq!(storage.get_latest_block_id().unwrap(), Some(0));

        storage.save_block(&dummy_block(1, 1)).unwrap();
        assert_eq!(storage.get_latest_block_id().unwrap(), Some(1));
    }

    #[test]
    fn test_get_all_deals() {
        let storage = InMemoryStorage::new();
        let maker = dummy_address(1);

        for i in 0..5 {
            let deal = Deal {
                id: i,
                maker,
                taker: None,
                asset_base: 0,
                asset_quote: 1,
                chain_id_base: zkclear_types::chain_ids::ETHEREUM,
                chain_id_quote: zkclear_types::chain_ids::ETHEREUM,
                amount_base: 1000,
                amount_remaining: 1000,
                price_quote_per_base: 100,
                status: DealStatus::Pending,
                visibility: DealVisibility::Public,
                created_at: 1000,
                expires_at: None,
                external_ref: None,
                is_cross_chain: false,
            };
            storage.save_deal(&deal).unwrap();
        }

        let deals = storage.get_all_deals().unwrap();
        assert_eq!(deals.len(), 5);
    }
}
