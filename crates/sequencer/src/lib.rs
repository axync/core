pub mod config;
pub mod security;
mod validation;

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use axync_prover::{Prover, ProverConfig, ProverError};
use axync_state::State;
use axync_stf::{apply_block, apply_tx, StfError};
use axync_storage::Storage;
use axync_types::{Address, Block, BlockId, Tx};

use config::{DEFAULT_MAX_QUEUE_SIZE, DEFAULT_MAX_TXS_PER_BLOCK, DEFAULT_SNAPSHOT_INTERVAL};
use security::{validate_address, validate_nonce_gap, validate_tx_size};
use validation::{validate_tx, ValidationError};

#[derive(Debug)]
pub enum SequencerError {
    QueueFull,
    ExecutionFailed(StfError),
    NoTransactions,
    InvalidBlockId,
    InvalidSignature,
    InvalidNonce,
    ValidationFailed,
    StorageError(String),
    ProverError(String),
}

pub struct Sequencer {
    state: Arc<Mutex<State>>,
    tx_queue: Arc<Mutex<VecDeque<Tx>>>,
    max_queue_size: usize,
    current_block_id: Arc<Mutex<BlockId>>,
    max_txs_per_block: usize,
    storage: Option<Arc<dyn Storage>>,
    snapshot_interval: BlockId,
    last_snapshot_block_id: Arc<Mutex<BlockId>>,
    prover: Option<Arc<Prover>>,
    /// Maps NFT listing_id → block_id where BuyNft was processed
    listing_sold_block: Arc<Mutex<HashMap<u64, BlockId>>>,
    /// Maps (address_hex, asset_id, amount, chain_id) → block_id where Withdraw was processed
    withdrawal_block: Arc<Mutex<HashMap<String, BlockId>>>,
}

impl Sequencer {
    pub fn new() -> Self {
        Self::with_config(DEFAULT_MAX_QUEUE_SIZE, DEFAULT_MAX_TXS_PER_BLOCK)
    }

    pub fn with_config(max_queue_size: usize, max_txs_per_block: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(State::new())),
            tx_queue: Arc::new(Mutex::new(VecDeque::new())),
            max_queue_size,
            current_block_id: Arc::new(Mutex::new(0)),
            max_txs_per_block,
            storage: None,
            snapshot_interval: DEFAULT_SNAPSHOT_INTERVAL,
            last_snapshot_block_id: Arc::new(Mutex::new(0)),
            prover: None,
            listing_sold_block: Arc::new(Mutex::new(HashMap::new())),
            withdrawal_block: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_snapshot_interval(mut self, interval: BlockId) -> Self {
        self.snapshot_interval = interval;
        self
    }

    /// Set prover for automatic proof generation
    pub fn with_prover(mut self, prover: Arc<Prover>) -> Self {
        self.prover = Some(prover);
        self
    }

    /// Set prover configuration (will create prover internally)
    pub fn with_prover_config(mut self, config: ProverConfig) -> Result<Self, SequencerError> {
        let prover = Prover::new(config).map_err(|e| {
            SequencerError::ProverError(format!("Failed to create prover: {:?}", e))
        })?;
        self.prover = Some(Arc::new(prover));
        Ok(self)
    }

    pub fn with_storage<S: Storage + 'static>(storage: S) -> Result<Self, SequencerError> {
        let mut sequencer = Self::with_config(DEFAULT_MAX_QUEUE_SIZE, DEFAULT_MAX_TXS_PER_BLOCK);
        sequencer.load_state_from_storage(Arc::new(storage))?;
        Ok(sequencer)
    }

    pub fn with_storage_arc(storage: Arc<dyn Storage>) -> Result<Self, SequencerError> {
        let mut sequencer = Self::with_config(DEFAULT_MAX_QUEUE_SIZE, DEFAULT_MAX_TXS_PER_BLOCK);
        sequencer.load_state_from_storage(storage)?;
        Ok(sequencer)
    }

    pub fn set_storage<S: Storage + 'static>(&mut self, storage: S) -> Result<(), SequencerError> {
        self.load_state_from_storage(Arc::new(storage))?;
        Ok(())
    }

    fn load_state_from_storage(&mut self, storage: Arc<dyn Storage>) -> Result<(), SequencerError> {
        let latest_block_id = storage
            .get_latest_block_id()
            .map_err(|e| {
                SequencerError::StorageError(format!("Failed to get latest block ID: {:?}", e))
            })?
            .unwrap_or(0);

        match storage.get_latest_state_snapshot() {
            Ok(Some((snapshot_state, snapshot_block_id))) => {
                *self.state.lock().unwrap() = snapshot_state;
                *self.last_snapshot_block_id.lock().unwrap() = snapshot_block_id;

                if latest_block_id > snapshot_block_id {
                    self.replay_blocks_from_storage(
                        &*storage,
                        snapshot_block_id + 1,
                        latest_block_id,
                    )?;
                }

                *self.current_block_id.lock().unwrap() = latest_block_id + 1;
            }
            Ok(None) => {
                // If storage is empty (no snapshot), check if we actually have blocks
                // Blocks are numbered starting from 1 (not 0), so we need to check from block 1
                if latest_block_id > 0 {
                    // Try to find the first existing block (could be 1, 2, etc.)
                    // Start from block 1 since blocks are numbered from 1
                    let mut first_block_found = None;
                    for block_id in 1..=latest_block_id {
                        match storage.get_block(block_id) {
                            Ok(Some(_)) => {
                                first_block_found = Some(block_id);
                                break;
                            }
                            Ok(None) => continue,
                            Err(e) => {
                                return Err(SequencerError::StorageError(format!(
                                    "Failed to check block {}: {:?}",
                                    block_id, e
                                )));
                            }
                        }
                    }
                    
                    if let Some(first_block) = first_block_found {
                        // Found first block, replay from there
                        self.replay_blocks_from_storage(&*storage, first_block, latest_block_id)?;
                    } else {
                        // No blocks found despite latest_block_id > 0
                        // This indicates data inconsistency - treat as empty storage
                        println!("Warning: latest_block_id is {} but no blocks found. Starting with fresh state.", latest_block_id);
                    }
                }
                // If latest_block_id is 0 or no blocks found, start fresh
                *self.current_block_id.lock().unwrap() = latest_block_id + 1;
                *self.last_snapshot_block_id.lock().unwrap() = 0;
            }
            Err(e) => {
                return Err(SequencerError::StorageError(format!(
                    "Failed to load state: {:?}",
                    e
                )))
            }
        }

        self.storage = Some(storage);
        Ok(())
    }

    fn replay_blocks_from_storage(
        &self,
        storage: &dyn Storage,
        from_block: BlockId,
        to_block: BlockId,
    ) -> Result<(), SequencerError> {
        // Skip replay if range is invalid (from > to) or empty
        if from_block > to_block {
            return Ok(());
        }

        let mut state = self.state.lock().unwrap();

        for block_id in from_block..=to_block {
            match storage.get_block(block_id) {
                Ok(Some(block)) => {
                    apply_block(&mut state, &block.transactions, block.timestamp)
                        .map_err(SequencerError::ExecutionFailed)?;
                }
                Ok(None) => {
                    return Err(SequencerError::StorageError(format!(
                        "Block {} not found",
                        block_id
                    )));
                }
                Err(e) => {
                    return Err(SequencerError::StorageError(format!(
                        "Failed to load block {}: {:?}",
                        block_id, e
                    )));
                }
            }
        }

        Ok(())
    }

    pub fn submit_tx(&self, tx: Tx) -> Result<(), SequencerError> {
        self.submit_tx_with_validation(tx, true)
    }

    pub fn submit_tx_with_validation(&self, tx: Tx, validate: bool) -> Result<(), SequencerError> {
        if validate {
            // Security checks: validate transaction size and address format
            if let Err(_) = validate_tx_size(&tx) {
                return Err(SequencerError::InvalidSignature); // Reuse error type
            }
            
            if !validate_address(&tx.from) {
                return Err(SequencerError::InvalidSignature);
            }
            
            let state = self.state.lock().unwrap();
            
            // Validate nonce gap
            let account = state.get_account_by_address(tx.from);
            let current_nonce = account.map(|a| a.nonce).unwrap_or(0);
            if let Err(_) = validate_nonce_gap(current_nonce, tx.nonce) {
                return Err(SequencerError::InvalidNonce);
            }

            match validate_tx(&state, &tx) {
                Ok(()) => {}
                Err(ValidationError::InvalidSignature) => {
                    return Err(SequencerError::InvalidSignature)
                }
                Err(ValidationError::InvalidNonce) => return Err(SequencerError::InvalidNonce),
                Err(ValidationError::SignatureRecoveryFailed) => {
                    return Err(SequencerError::InvalidSignature)
                }
            }

            drop(state);
        }

        let mut queue = self.tx_queue.lock().unwrap();

        if queue.len() >= self.max_queue_size {
            return Err(SequencerError::QueueFull);
        }

        queue.push_back(tx);
        Ok(())
    }

    /// Build a block with transactions from the queue
    /// This is a synchronous version that doesn't generate proofs
    pub fn build_block(&self) -> Result<Block, SequencerError> {
        self.build_block_with_proof(false)
    }

    /// Build a block with optional proof generation
    /// If generate_proof is true and prover is available, generates ZK proof
    pub fn build_block_with_proof(&self, generate_proof: bool) -> Result<Block, SequencerError> {
        let mut queue = self.tx_queue.lock().unwrap();
        let block_id = *self.current_block_id.lock().unwrap();

        if queue.is_empty() {
            return Err(SequencerError::NoTransactions);
        }

        let mut transactions = Vec::new();
        let count = queue.len().min(self.max_txs_per_block);

        for _ in 0..count {
            if let Some(tx) = queue.pop_front() {
                transactions.push(tx);
            } else {
                break;
            }
        }
        drop(queue);

        // Get current state (before applying transactions)
        let prev_state = self.state.lock().unwrap().clone();
        drop(self.state.lock().unwrap());

        // Calculate state roots and withdrawals root
        // Note: prev_state_root is computed but not used directly here (used in proof generation)
        let _prev_state_root = self.compute_state_root(&prev_state)?;

        // Apply transactions individually, skipping failures.
        // If a transaction from a sender fails, skip all subsequent transactions
        // from the same sender (since their nonces would be invalid).
        let mut new_state = prev_state.clone();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut successful_transactions = Vec::new();
        let mut failed_senders: std::collections::HashSet<Address> = std::collections::HashSet::new();

        for tx in transactions {
            if failed_senders.contains(&tx.from) {
                eprintln!(
                    "Skipping tx (id={}) from sender {:?}: previous tx from same sender failed",
                    tx.id, tx.from
                );
                continue;
            }

            let mut trial_state = new_state.clone();
            match apply_tx(&mut trial_state, &tx, timestamp) {
                Ok(()) => {
                    new_state = trial_state;
                    successful_transactions.push(tx);
                }
                Err(e) => {
                    eprintln!(
                        "Warning: tx (id={}) from sender {:?} failed: {:?}, skipping",
                        tx.id, tx.from, e
                    );
                    failed_senders.insert(tx.from);
                }
            }
        }

        let transactions = successful_transactions;

        if transactions.is_empty() {
            return Err(SequencerError::NoTransactions);
        }

        let new_state_root = self.compute_state_root(&new_state)?;
        let withdrawals_root = self.compute_withdrawals_root_with_state(&transactions, &new_state)?;

        // Generate proof if requested and prover is available
        let block_proof = if generate_proof {
            if let Some(ref prover) = self.prover {
                // Create temporary block for proof generation
                // Note: We use prev_state_root and new_state_root that we just computed
                let temp_block = Block {
                    id: block_id,
                    transactions: transactions.clone(),
                    timestamp,
                    state_root: new_state_root,
                    withdrawals_root,
                    block_proof: Vec::new(),
                };

                // Generate proof (blocking call using tokio::runtime)
                match self.generate_block_proof(prover, &temp_block, &prev_state, &new_state) {
                    Ok(proof) => proof,
                    Err(e) => {
                        eprintln!("Warning: Failed to generate proof: {:?}", e);
                        Vec::new() // Fallback to empty proof
                    }
                }
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

        let block = Block {
            id: block_id,
            transactions,
            timestamp,
            state_root: new_state_root,
            withdrawals_root,
            block_proof,
        };

        Ok(block)
    }

    /// Generate block proof using prover (blocking call)
    /// This is called from spawn_blocking, so we try to use Handle::current() if available
    /// Otherwise create a new runtime in a separate thread to avoid deadlocks
    fn generate_block_proof(
        &self,
        prover: &Arc<Prover>,
        block: &Block,
        prev_state: &State,
        new_state: &State,
    ) -> Result<Vec<u8>, SequencerError> {
        // We're in spawn_blocking, so we can't use Handle::current() directly
        // Create runtime in a separate thread to avoid deadlocks

        // Clone data needed for proof generation
        let prover_clone = Arc::clone(prover);
        let block_clone = block.clone();
        let prev_state_clone = prev_state.clone();
        let new_state_clone = new_state.clone();

        // Create runtime in a separate thread to avoid deadlocks
        // This is necessary because we're already in spawn_blocking
        let handle = std::thread::spawn(move || {
            // Use current_thread runtime to avoid spawning new threads
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            
            match rt {
                Ok(runtime) => {
                    runtime.block_on(
                        prover_clone.prove_block(&block_clone, &prev_state_clone, &new_state_clone)
                    )
                }
                Err(e) => {
                    Err(ProverError::StarkProof(format!("Failed to create runtime: {:?}", e)))
                }
            }
        });

        // Wait for result - join will block until thread completes
        // For placeholder proofs, this should be very fast (< 1ms)
        // For real proofs, this may take longer, but timeout is handled in demo
        match handle.join() {
            Ok(Ok(block_proof)) => {
                // Serialize the proof
                bincode::serialize(&block_proof.zk_proof)
                    .map_err(|e| SequencerError::ProverError(format!("Failed to serialize proof: {}", e)))
            }
            Ok(Err(e)) => {
                Err(SequencerError::ProverError(format!("Proof generation failed: {:?}", e)))
            }
            Err(_) => {
                Err(SequencerError::ProverError("Thread panicked during proof generation".to_string()))
            }
        }
    }

    /// Compute state root from state
    fn compute_state_root(&self, _state: &State) -> Result<[u8; 32], SequencerError> {
        // Use prover's compute_state_root if available, otherwise use simple hash
        // For now, use simple hash (same logic as Prover's placeholder)
        let state_bytes = bincode::serialize(_state).map_err(|e| {
            SequencerError::StorageError(format!("Failed to serialize state: {}", e))
        })?;

        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&state_bytes);
        Ok(hasher.finalize().into())
    }

    /// Compute withdrawals root from transactions using the post-execution state
    fn compute_withdrawals_root_with_state(&self, transactions: &[Tx], new_state: &State) -> Result<[u8; 32], SequencerError> {
        use axync_prover::merkle::{hash_withdrawal, MerkleTree};

        let mut tree = MerkleTree::new();

        let state = new_state;

        for tx in transactions {
            match &tx.payload {
                axync_types::TxPayload::Withdraw(w) => {
                    let leaf = hash_withdrawal(tx.from, w.asset_id, w.amount, w.chain_id);
                    tree.add_leaf(leaf);
                }
                axync_types::TxPayload::BuyNft(buy) => {
                    if let Some(listing) = state.get_nft_listing(buy.listing_id) {
                        let leaf = Self::compute_release_leaf(&listing);
                        tree.add_leaf(leaf);
                    }
                }
                _ => {}
            }
        }

        tree.root().map_err(|e| {
            SequencerError::ProverError(format!("Failed to compute withdrawals root: {:?}", e))
        })
    }

    /// Compute release leaf for a listing based on asset type
    fn compute_release_leaf(listing: &axync_types::NftListing) -> [u8; 32] {
        use axync_prover::merkle::{hash_nft_release, hash_token_release};
        match listing.asset_type {
            axync_types::AssetType::ERC721 => hash_nft_release(
                listing.nft_contract,
                listing.token_id,
                listing.buyer,
                listing.nft_chain_id,
                listing.on_chain_listing_id,
            ),
            axync_types::AssetType::ERC20 => hash_token_release(
                listing.nft_contract,
                listing.amount,
                listing.buyer,
                listing.nft_chain_id,
                listing.on_chain_listing_id,
            ),
        }
    }

    pub fn execute_block(&self, block: Block) -> Result<(), SequencerError> {
        let expected_id = *self.current_block_id.lock().unwrap();
        if block.id != expected_id {
            return Err(SequencerError::InvalidBlockId);
        }

        let mut state = self.state.lock().unwrap();

        match apply_block(&mut state, &block.transactions, block.timestamp) {
            Ok(()) => {
                // Record BuyNft/Withdraw → block_id mappings for proof generation
                {
                    let mut sold_map = self.listing_sold_block.lock().unwrap();
                    let mut wd_map = self.withdrawal_block.lock().unwrap();
                    for tx in &block.transactions {
                        match &tx.payload {
                            axync_types::TxPayload::BuyNft(buy) => {
                                sold_map.insert(buy.listing_id, block.id);
                            }
                            axync_types::TxPayload::Withdraw(w) => {
                                let key = format!("{}:{}:{}:{}", hex::encode(tx.from), w.asset_id, w.amount, w.chain_id);
                                wd_map.insert(key, block.id);
                            }
                            _ => {}
                        }
                    }
                }

                let mut block_id = self.current_block_id.lock().unwrap();
                *block_id += 1;
                drop(block_id);

                if let Some(ref storage) = self.storage {
                    storage.save_block(&block).map_err(|e| {
                        SequencerError::StorageError(format!("Failed to save block: {:?}", e))
                    })?;

                    for (index, tx) in block.transactions.iter().enumerate() {
                        storage.save_transaction(tx, block.id, index).map_err(|e| {
                            SequencerError::StorageError(format!(
                                "Failed to save transaction: {:?}",
                                e
                            ))
                        })?;
                    }

                    for deal in state.deals.values() {
                        storage.save_deal(deal).map_err(|e| {
                            SequencerError::StorageError(format!("Failed to save deal: {:?}", e))
                        })?;
                    }

                    let last_snapshot = *self.last_snapshot_block_id.lock().unwrap();
                    let blocks_since_snapshot = block.id.saturating_sub(last_snapshot);

                    if blocks_since_snapshot >= self.snapshot_interval {
                        let state_clone = state.clone();
                        drop(state);

                        storage
                            .save_state_snapshot(&state_clone, block.id)
                            .map_err(|e| {
                                SequencerError::StorageError(format!(
                                    "Failed to save state snapshot: {:?}",
                                    e
                                ))
                            })?;

                        *self.last_snapshot_block_id.lock().unwrap() = block.id;
                    }
                }

                Ok(())
            }
            Err(e) => Err(SequencerError::ExecutionFailed(e)),
        }
    }

    pub fn build_and_execute_block(&self) -> Result<Block, SequencerError> {
        self.build_and_execute_block_with_proof(false)
    }

    /// Build and execute block with optional proof generation
    pub fn build_and_execute_block_with_proof(
        &self,
        generate_proof: bool,
    ) -> Result<Block, SequencerError> {
        let block = self.build_block_with_proof(generate_proof)?;
        self.execute_block(block.clone())?;
        Ok(block)
    }

    /// Get block_id where a listing was sold (BuyNft processed)
    pub fn get_listing_sold_block(&self, listing_id: u64) -> Option<BlockId> {
        self.listing_sold_block.lock().unwrap().get(&listing_id).copied()
    }

    /// Get block_id where a withdrawal was processed
    pub fn get_withdrawal_block(&self, from: &[u8; 20], asset_id: u16, amount: u128, chain_id: u64) -> Option<BlockId> {
        let key = format!("{}:{}:{}:{}", hex::encode(from), asset_id, amount, chain_id);
        self.withdrawal_block.lock().unwrap().get(&key).copied()
    }

    /// Generate merkle proof for a sold NFT listing by loading the block and rebuilding the tree
    pub fn generate_nft_release_proof(&self, listing_id: u64) -> Result<(Vec<[u8; 32]>, [u8; 32]), SequencerError> {
        let block_id = self.get_listing_sold_block(listing_id)
            .ok_or_else(|| SequencerError::StorageError("Listing not found in any block".to_string()))?;

        let storage = self.storage.as_ref()
            .ok_or_else(|| SequencerError::StorageError("No storage configured".to_string()))?;

        let block = storage.get_block(block_id)
            .map_err(|e| SequencerError::StorageError(format!("Failed to load block: {:?}", e)))?
            .ok_or_else(|| SequencerError::StorageError(format!("Block {} not found", block_id)))?;

        // Rebuild the merkle tree from block transactions
        use axync_prover::merkle::{hash_withdrawal, MerkleTree};
        let mut tree = MerkleTree::new();
        let mut target_leaf_index: Option<usize> = None;

        let state = self.state.lock().unwrap();

        for tx in &block.transactions {
            match &tx.payload {
                axync_types::TxPayload::Withdraw(w) => {
                    let leaf = hash_withdrawal(tx.from, w.asset_id, w.amount, w.chain_id);
                    tree.add_leaf(leaf);
                }
                axync_types::TxPayload::BuyNft(buy) => {
                    if let Some(listing) = state.get_nft_listing(buy.listing_id) {
                        let leaf = Self::compute_release_leaf(&listing);
                        if buy.listing_id == listing_id {
                            target_leaf_index = Some(tree.len());
                        }
                        tree.add_leaf(leaf);
                    }
                }
                _ => {}
            }
        }

        drop(state);

        let leaf_index = target_leaf_index
            .ok_or_else(|| SequencerError::StorageError("BuyNft tx not found in block".to_string()))?;

        let root = tree.root().map_err(|e| {
            SequencerError::ProverError(format!("Failed to compute root: {:?}", e))
        })?;

        let proof = tree.proof(leaf_index).map_err(|e| {
            SequencerError::ProverError(format!("Failed to generate proof: {:?}", e))
        })?;

        Ok((proof, root))
    }

    /// Generate merkle proof for a withdrawal
    pub fn generate_withdrawal_proof(&self, from: &[u8; 20], asset_id: u16, amount: u128, chain_id: u64) -> Result<(Vec<[u8; 32]>, [u8; 32]), SequencerError> {
        let block_id = self.get_withdrawal_block(from, asset_id, amount, chain_id)
            .ok_or_else(|| SequencerError::StorageError("Withdrawal not found in any block".to_string()))?;

        let storage = self.storage.as_ref()
            .ok_or_else(|| SequencerError::StorageError("No storage configured".to_string()))?;

        let block = storage.get_block(block_id)
            .map_err(|e| SequencerError::StorageError(format!("Failed to load block: {:?}", e)))?
            .ok_or_else(|| SequencerError::StorageError(format!("Block {} not found", block_id)))?;

        use axync_prover::merkle::{hash_withdrawal, MerkleTree};
        let mut tree = MerkleTree::new();
        let mut target_leaf_index: Option<usize> = None;

        let state = self.state.lock().unwrap();

        for tx in &block.transactions {
            match &tx.payload {
                axync_types::TxPayload::Withdraw(w) => {
                    let leaf = hash_withdrawal(tx.from, w.asset_id, w.amount, w.chain_id);
                    if tx.from == *from && w.asset_id == asset_id && w.amount == amount && w.chain_id == chain_id {
                        target_leaf_index = Some(tree.len());
                    }
                    tree.add_leaf(leaf);
                }
                axync_types::TxPayload::BuyNft(buy) => {
                    if let Some(listing) = state.get_nft_listing(buy.listing_id) {
                        let leaf = Self::compute_release_leaf(&listing);
                        tree.add_leaf(leaf);
                    }
                }
                _ => {}
            }
        }

        drop(state);

        let leaf_index = target_leaf_index
            .ok_or_else(|| SequencerError::StorageError("Withdraw tx not found in block".to_string()))?;

        let root = tree.root().map_err(|e| {
            SequencerError::ProverError(format!("Failed to compute root: {:?}", e))
        })?;

        let proof = tree.proof(leaf_index).map_err(|e| {
            SequencerError::ProverError(format!("Failed to generate proof: {:?}", e))
        })?;

        Ok((proof, root))
    }

    pub fn get_state(&self) -> Arc<Mutex<State>> {
        Arc::clone(&self.state)
    }

    pub fn get_current_block_id(&self) -> BlockId {
        *self.current_block_id.lock().unwrap()
    }

    pub fn queue_length(&self) -> usize {
        self.tx_queue.lock().unwrap().len()
    }

    pub fn has_pending_txs(&self) -> bool {
        !self.tx_queue.lock().unwrap().is_empty()
    }

    pub fn create_state_snapshot(&self) -> Result<(), SequencerError> {
        if let Some(ref storage) = self.storage {
            let state = self.state.lock().unwrap();
            let block_id = *self.current_block_id.lock().unwrap();

            let state_clone = state.clone();
            drop(state);

            storage
                .save_state_snapshot(&state_clone, block_id)
                .map_err(|e| {
                    SequencerError::StorageError(format!("Failed to save state snapshot: {:?}", e))
                })?;
        }
        Ok(())
    }
}

impl Default for Sequencer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axync_types::{Address, Deposit, Tx, TxKind, TxPayload};

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
                chain_id: axync_types::chain_ids::ETHEREUM,
            }),
            signature: [0u8; 65],
        }
    }

    #[test]
    fn test_submit_and_build_block() {
        let sequencer = Sequencer::with_config(100, 10);
        let addr = [1u8; 20];

        for i in 0..5 {
            sequencer
                .submit_tx_with_validation(dummy_tx(i, addr, i), false)
                .unwrap();
        }

        let block = sequencer.build_block().unwrap();
        assert_eq!(block.transactions.len(), 5);
        assert_eq!(sequencer.queue_length(), 0);
    }

    #[test]
    fn test_queue_full() {
        let sequencer = Sequencer::with_config(5, 10);
        let addr = [1u8; 20];

        for i in 0..5 {
            sequencer
                .submit_tx_with_validation(dummy_tx(i, addr, i), false)
                .unwrap();
        }

        match sequencer.submit_tx_with_validation(dummy_tx(5, addr, 5), false) {
            Err(SequencerError::QueueFull) => {}
            _ => panic!("Expected QueueFull error"),
        }
    }

    #[test]
    fn test_execute_block() {
        let sequencer = Sequencer::new();
        let addr = [1u8; 20];

        sequencer
            .submit_tx_with_validation(dummy_tx(0, addr, 0), false)
            .unwrap();
        let block = sequencer.build_block().unwrap();

        sequencer.execute_block(block).unwrap();
        assert_eq!(sequencer.get_current_block_id(), 1);
    }

    #[test]
    fn test_build_and_execute() {
        let sequencer = Sequencer::new();
        let addr = [1u8; 20];

        sequencer
            .submit_tx_with_validation(dummy_tx(0, addr, 0), false)
            .unwrap();
        let block = sequencer.build_and_execute_block().unwrap();

        assert_eq!(block.id, 0);
        assert_eq!(sequencer.get_current_block_id(), 1);
    }
}
