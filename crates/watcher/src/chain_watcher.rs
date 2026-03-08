use crate::config::ChainConfig;
use crate::event_processor::EventProcessor;
use crate::rpc_client::RpcClient;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};
use zkclear_sequencer::Sequencer;

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
        let logs = self
            .rpc_client
            .get_logs(
                block_number,
                block_number,
                &self.config.deposit_contract_address,
            )
            .await?;

        debug!(
            chain_id = self.config.chain_id,
            block = block_number,
            log_count = logs.len(),
            "Processing block"
        );

        for log in logs {
            let tx_hash = self.parse_tx_hash(&log)?;

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

            let (account, asset_id, amount) = self.parse_deposit_log(&log)?;

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

    fn parse_deposit_log(
        &self,
        log: &serde_json::Value,
    ) -> anyhow::Result<(zkclear_types::Address, zkclear_types::AssetId, u128)> {
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
