use crate::config::ChainConfig;
use crate::event_processor::EventProcessor;
use crate::rpc_client::RpcClient;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};
use axync_sequencer::Sequencer;

pub struct ChainWatcher {
    pub(crate) config: ChainConfig,
    processor: EventProcessor,
    pub(crate) rpc_client: RpcClient,
    processed_txs: Arc<tokio::sync::Mutex<HashSet<[u8; 32]>>>,
    last_processed_block: Arc<tokio::sync::Mutex<u64>>,
    last_confirmed_block_hash: Arc<tokio::sync::Mutex<Option<[u8; 32]>>>,
}

impl ChainWatcher {
    pub fn new(config: ChainConfig, sequencer: Arc<Sequencer>) -> anyhow::Result<Self> {
        let processor = EventProcessor::new(sequencer);
        let rpc_client = RpcClient::new(config.clone());
        Ok(Self {
            config,
            processor,
            rpc_client,
            processed_txs: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            last_processed_block: Arc::new(tokio::sync::Mutex::new(0)),
            last_confirmed_block_hash: Arc::new(tokio::sync::Mutex::new(None)),
        })
    }

    pub async fn watch(&self) -> anyhow::Result<()> {
        info!(
            chain_id = self.config.chain_id,
            rpc_url = %self.config.rpc_url,
            "Starting watcher for chain"
        );

        let mut interval_timer = interval(Duration::from_secs(self.config.poll_interval_seconds));

        loop {
            interval_timer.tick().await;

            if let Err(e) = self.poll_events().await {
                error!(
                    chain_id = self.config.chain_id,
                    error = %e,
                    "Error polling events"
                );
            }
        }
    }

    async fn poll_events(&self) -> anyhow::Result<()> {
        let latest_block = self.rpc_client.get_block_number().await?;
        let mut last_processed = *self.last_processed_block.lock().await;

        // On first poll, start from configured start_block
        if last_processed == 0 && self.config.start_block > 0 {
            last_processed = self.config.start_block;
            *self.last_processed_block.lock().await = last_processed;
        }

        // Check for reorgs by verifying block hash
        if last_processed > 0 {
            if let Err(e) = self.check_reorg(last_processed).await {
                warn!(
                    chain_id = self.config.chain_id,
                    block = last_processed,
                    error = %e,
                    "Possible reorg detected, resetting to safety block"
                );
                last_processed = last_processed.saturating_sub(self.config.reorg_safety_blocks);
                *self.last_processed_block.lock().await = last_processed;
            }
        }

        if latest_block < last_processed + self.config.required_confirmations {
            debug!(
                chain_id = self.config.chain_id,
                latest = latest_block,
                last_processed = last_processed,
                "Waiting for more confirmations"
            );
            return Ok(());
        }

        let from_block = last_processed.saturating_sub(self.config.reorg_safety_blocks);
        let to_block = latest_block - self.config.required_confirmations;

        if to_block <= from_block {
            return Ok(());
        }

        info!(
            chain_id = self.config.chain_id,
            from_block = from_block,
            to_block = to_block,
            "Polling blocks"
        );

        for block_num in from_block..=to_block {
            if let Err(e) = self.process_block(block_num).await {
                error!(
                    chain_id = self.config.chain_id,
                    block = block_num,
                    error = %e,
                    "Error processing block"
                );
            }
        }

        *self.last_processed_block.lock().await = to_block;

        Ok(())
    }

    async fn check_reorg(&self, block_number: u64) -> anyhow::Result<()> {
        // Get block hash for the last processed block
        let params = serde_json::json!([format!("0x{:x}", block_number), false]);
        let response = self.rpc_client.call("eth_getBlockByNumber", params).await?;

        let block_hash_hex = response
            .get("result")
            .and_then(|v| v.get("hash"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing block hash"))?;

        let block_hash_bytes = hex::decode(block_hash_hex.trim_start_matches("0x"))
            .map_err(|e| anyhow::anyhow!("Failed to decode block hash: {}", e))?;

        if block_hash_bytes.len() != 32 {
            return Err(anyhow::anyhow!("Invalid block hash length"));
        }

        let mut block_hash = [0u8; 32];
        block_hash.copy_from_slice(&block_hash_bytes);

        let mut last_hash = self.last_confirmed_block_hash.lock().await;
        if let Some(prev_hash) = *last_hash {
            if prev_hash != block_hash {
                return Err(anyhow::anyhow!("Block hash mismatch - reorg detected"));
            }
        } else {
            *last_hash = Some(block_hash);
        }

        Ok(())
    }

    async fn process_block(&self, block_number: u64) -> anyhow::Result<()> {
        // Process vault deposit events
        let logs = self
            .rpc_client
            .get_logs(
                block_number,
                block_number,
                &self.config.vault_contract_address,
            )
            .await?;

        debug!(
            chain_id = self.config.chain_id,
            block = block_number,
            log_count = logs.len(),
            "Processing block (vault)"
        );

        for log in &logs {
            let tx_hash = self.parse_tx_hash(log)?;

            let processed = self.processed_txs.lock().await;
            if processed.contains(&tx_hash) {
                debug!(
                    chain_id = self.config.chain_id,
                    tx_hash = ?tx_hash,
                    "Skipping already processed transaction"
                );
                continue;
            }
            drop(processed);

            let (account, asset_id, amount) = self.parse_deposit_log(log)?;

            match self.processor.process_deposit_event(
                self.config.chain_id,
                tx_hash,
                account,
                asset_id,
                amount,
            ) {
                Ok(_) => {
                    let mut processed = self.processed_txs.lock().await;
                    processed.insert(tx_hash);
                    info!(
                        chain_id = self.config.chain_id,
                        tx_hash = ?tx_hash,
                        account = ?account,
                        asset_id = asset_id,
                        amount = amount,
                        "Processed deposit"
                    );
                }
                Err(e) => {
                    error!(
                        chain_id = self.config.chain_id,
                        tx_hash = ?tx_hash,
                        error = %e,
                        "Failed to process deposit event"
                    );
                }
            }
        }

        // Process AxyncEscrow events
        if let Some(ref escrow_addr) = self.config.escrow_contract_address {
            let escrow_logs = self
                .rpc_client
                .get_logs(block_number, block_number, escrow_addr)
                .await?;

            if !escrow_logs.is_empty() {
                debug!(
                    chain_id = self.config.chain_id,
                    block = block_number,
                    log_count = escrow_logs.len(),
                    "Processing block (escrow)"
                );
            }

            let nft_listed_sig = "0xfebb39f58e20b82053b272222107ed5076573054a0becf582b5800513501d34b";
            let token_listed_sig = "0xe9f33fbfcd71bdbfdd2c2a95058cbb3f5378444a2676e6cfb173a65cfce389e6";
            let nft_cancelled_sig = "0xe8580d4b2abe8e4b73ec7f0ee6709642b78d94be0a89c3609cdddf6f119155e3";
            let listing_cancelled_sig = "0x411aee90354c51b1b04cd563fcab2617142a9d50da19232d888547c8a1b7fd8a";

            for log in &escrow_logs {
                let tx_hash = self.parse_tx_hash(log)?;

                let processed = self.processed_txs.lock().await;
                if processed.contains(&tx_hash) {
                    continue;
                }
                drop(processed);

                let topics = log["topics"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("Missing topics"))?;

                if topics.is_empty() {
                    continue;
                }

                let event_sig = topics[0].as_str().unwrap_or("");

                if event_sig == nft_listed_sig {
                    match self.parse_nft_listed_log(log) {
                        Ok((seller, nft_contract, token_id, price, payment_chain_id, listing_id)) => {
                            match self.processor.process_nft_listed_event(
                                self.config.chain_id,
                                seller,
                                nft_contract,
                                token_id,
                                price,
                                payment_chain_id,
                                listing_id,
                            ) {
                                Ok(_) => {
                                    let mut processed = self.processed_txs.lock().await;
                                    processed.insert(tx_hash);
                                    info!(
                                        chain_id = self.config.chain_id,
                                        listing_id = listing_id,
                                        "Processed NftListed"
                                    );
                                }
                                Err(e) => {
                                    error!(error = %e, "Failed to process NftListed");
                                }
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to parse NftListed log");
                        }
                    }
                } else if event_sig == token_listed_sig {
                    // TokenListed: topics=[sig, listingId, seller], data=[tokenContract, amount, price, paymentChainId]
                    match self.parse_token_listed_log(log) {
                        Ok((seller, token_contract, amount, price, payment_chain_id, listing_id)) => {
                            match self.processor.process_token_listed_event(
                                self.config.chain_id,
                                seller,
                                token_contract,
                                amount,
                                price,
                                payment_chain_id,
                                listing_id,
                            ) {
                                Ok(_) => {
                                    let mut processed = self.processed_txs.lock().await;
                                    processed.insert(tx_hash);
                                    info!(
                                        chain_id = self.config.chain_id,
                                        listing_id = listing_id,
                                        "Processed TokenListed"
                                    );
                                }
                                Err(e) => {
                                    error!(error = %e, "Failed to process TokenListed");
                                }
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to parse TokenListed log");
                        }
                    }
                } else if event_sig == nft_cancelled_sig || event_sig == listing_cancelled_sig {
                    match self.parse_nft_cancelled_log(log) {
                        Ok(listing_id) => {
                            match self.processor.process_nft_cancelled_event(listing_id) {
                                Ok(_) => {
                                    let mut processed = self.processed_txs.lock().await;
                                    processed.insert(tx_hash);
                                    info!(
                                        chain_id = self.config.chain_id,
                                        listing_id = listing_id,
                                        "Processed ListingCancelled"
                                    );
                                }
                                Err(e) => {
                                    error!(error = %e, "Failed to process ListingCancelled");
                                }
                            }
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to parse ListingCancelled log");
                        }
                    }
                }
            }
        }

        Ok(())
    }

    fn parse_tx_hash(&self, log: &serde_json::Value) -> anyhow::Result<[u8; 32]> {
        let tx_hash_hex = log["transactionHash"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing transactionHash in log"))?;

        let tx_hash_bytes = hex::decode(tx_hash_hex.trim_start_matches("0x"))
            .map_err(|e| anyhow::anyhow!("Failed to decode tx hash: {}", e))?;

        if tx_hash_bytes.len() != 32 {
            return Err(anyhow::anyhow!("Invalid tx hash length"));
        }

        let mut hash = [0u8; 32];
        hash.copy_from_slice(&tx_hash_bytes);
        Ok(hash)
    }

    /// Parse NftListed event: topics=[sig, listingId, seller], data=[nftContract, tokenId, price, paymentChainId]
    fn parse_nft_listed_log(
        &self,
        log: &serde_json::Value,
    ) -> anyhow::Result<(axync_types::Address, axync_types::Address, u64, u128, u64, u64)> {
        let topics = log["topics"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing topics"))?;

        if topics.len() < 3 {
            return Err(anyhow::anyhow!("NftListed: expected 3 topics"));
        }

        // topics[1] = listingId (indexed uint256)
        let listing_id_hex = topics[1].as_str().unwrap_or("0x0");
        let listing_id = u64::from_str_radix(listing_id_hex.trim_start_matches("0x"), 16)?;

        // topics[2] = seller (indexed address)
        let seller_hex = topics[2].as_str().unwrap_or("0x0");
        let seller_bytes = hex::decode(seller_hex.trim_start_matches("0x"))?;
        let mut seller = [0u8; 20];
        seller.copy_from_slice(&seller_bytes[12..32]);

        // data = abi.encode(nftContract, tokenId, price, paymentChainId)
        let data = log["data"].as_str().unwrap_or("0x");
        let data_bytes = hex::decode(data.trim_start_matches("0x"))?;

        if data_bytes.len() < 128 {
            return Err(anyhow::anyhow!("NftListed: data too short"));
        }

        // nftContract (address, padded to 32 bytes)
        let mut nft_contract = [0u8; 20];
        nft_contract.copy_from_slice(&data_bytes[12..32]);

        // tokenId (uint256)
        let mut token_id_bytes = [0u8; 8];
        token_id_bytes.copy_from_slice(&data_bytes[56..64]);
        let token_id = u64::from_be_bytes(token_id_bytes);

        // price (uint256 → u128)
        let mut price_bytes = [0u8; 16];
        price_bytes.copy_from_slice(&data_bytes[80..96]);
        let price = u128::from_be_bytes(price_bytes);

        // paymentChainId (uint256 → u64)
        let mut chain_id_bytes = [0u8; 8];
        chain_id_bytes.copy_from_slice(&data_bytes[120..128]);
        let payment_chain_id = u64::from_be_bytes(chain_id_bytes);

        Ok((seller, nft_contract, token_id, price, payment_chain_id, listing_id))
    }

    /// Parse TokenListed event: topics=[sig, listingId, seller], data=[tokenContract, amount, price, paymentChainId]
    fn parse_token_listed_log(
        &self,
        log: &serde_json::Value,
    ) -> anyhow::Result<(axync_types::Address, axync_types::Address, u128, u128, u64, u64)> {
        let topics = log["topics"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing topics"))?;

        if topics.len() < 3 {
            return Err(anyhow::anyhow!("TokenListed: expected 3 topics"));
        }

        let listing_id_hex = topics[1].as_str().unwrap_or("0x0");
        let listing_id = u64::from_str_radix(listing_id_hex.trim_start_matches("0x"), 16)?;

        let seller_hex = topics[2].as_str().unwrap_or("0x0");
        let seller_bytes = hex::decode(seller_hex.trim_start_matches("0x"))?;
        let mut seller = [0u8; 20];
        seller.copy_from_slice(&seller_bytes[12..32]);

        let data = log["data"].as_str().unwrap_or("0x");
        let data_bytes = hex::decode(data.trim_start_matches("0x"))?;

        if data_bytes.len() < 128 {
            return Err(anyhow::anyhow!("TokenListed: data too short"));
        }

        // tokenContract (address)
        let mut token_contract = [0u8; 20];
        token_contract.copy_from_slice(&data_bytes[12..32]);

        // amount (uint256 → u128)
        let mut amount_bytes = [0u8; 16];
        amount_bytes.copy_from_slice(&data_bytes[48..64]);
        let amount = u128::from_be_bytes(amount_bytes);

        // price (uint256 → u128)
        let mut price_bytes = [0u8; 16];
        price_bytes.copy_from_slice(&data_bytes[80..96]);
        let price = u128::from_be_bytes(price_bytes);

        // paymentChainId (uint256 → u64)
        let mut chain_id_bytes = [0u8; 8];
        chain_id_bytes.copy_from_slice(&data_bytes[120..128]);
        let payment_chain_id = u64::from_be_bytes(chain_id_bytes);

        Ok((seller, token_contract, amount, price, payment_chain_id, listing_id))
    }

    /// Parse NftCancelled event: topics=[sig, listingId]
    fn parse_nft_cancelled_log(
        &self,
        log: &serde_json::Value,
    ) -> anyhow::Result<u64> {
        let topics = log["topics"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing topics"))?;

        if topics.len() < 2 {
            return Err(anyhow::anyhow!("NftCancelled: expected 2 topics"));
        }

        let listing_id_hex = topics[1].as_str().unwrap_or("0x0");
        let listing_id = u64::from_str_radix(listing_id_hex.trim_start_matches("0x"), 16)?;

        Ok(listing_id)
    }

    fn parse_deposit_log(
        &self,
        log: &serde_json::Value,
    ) -> anyhow::Result<(axync_types::Address, axync_types::AssetId, u128)> {
        let topics = log["topics"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("Missing topics in log"))?;

        // Deposit event has 4 indexed parameters: event signature, user, assetId, txHash
        // topics[0] = event signature hash
        // topics[1] = user (address, padded to 32 bytes)
        // topics[2] = assetId (uint256, padded to 32 bytes)
        // topics[3] = txHash (bytes32)
        // data = amount (uint256, 32 bytes)
        if topics.len() < 4 {
            return Err(anyhow::anyhow!("Invalid topics length, expected at least 4 (event signature, user, assetId, txHash)"));
        }

        // Parse user address from topics[1]
        let account_hex = topics[1]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing account in topics"))?;

        let account_bytes = hex::decode(account_hex.trim_start_matches("0x"))
            .map_err(|e| anyhow::anyhow!("Failed to decode account: {}", e))?;

        if account_bytes.len() != 32 {
            return Err(anyhow::anyhow!(
                "Invalid account length in topic, expected 32 bytes"
            ));
        }

        let mut account = [0u8; 20];
        account.copy_from_slice(&account_bytes[12..32]);

        // Parse assetId from topics[2]
        let asset_id_hex = topics[2]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing asset_id in topics"))?;

        let asset_id_bytes = hex::decode(asset_id_hex.trim_start_matches("0x"))
            .map_err(|e| anyhow::anyhow!("Failed to decode asset_id: {}", e))?;

        if asset_id_bytes.len() != 32 {
            return Err(anyhow::anyhow!("Invalid asset_id length in topic"));
        }

        let asset_id = u16::from_be_bytes([asset_id_bytes[30], asset_id_bytes[31]]);

        // Parse amount from data field
        let data = log["data"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing data in log"))?;

        let data_bytes = hex::decode(data.trim_start_matches("0x"))
            .map_err(|e| anyhow::anyhow!("Failed to decode data: {}", e))?;

        if data_bytes.len() < 32 {
            return Err(anyhow::anyhow!(
                "Invalid data length, expected at least 32 bytes"
            ));
        }

        // Amount is uint256, stored as 32 bytes in data field
        let amount_bytes = &data_bytes[0..32];
        // Convert from big-endian bytes to u128 (we only use lower 16 bytes for u128)
        let mut amount_array = [0u8; 16];
        amount_array.copy_from_slice(&amount_bytes[16..32]);
        let amount = u128::from_be_bytes(amount_array);

        Ok((account, asset_id, amount))
    }
}
