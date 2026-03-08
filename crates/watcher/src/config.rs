use serde::{Deserialize, Serialize};
use zkclear_types::ChainId;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    pub chain_id: ChainId,
    pub rpc_url: String,
    pub deposit_contract_address: String,
    pub required_confirmations: u64,
    pub poll_interval_seconds: u64,
    pub rpc_timeout_seconds: u64,
    pub max_retries: u32,
    pub retry_delay_seconds: u64,
    pub reorg_safety_blocks: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherConfig {
    pub chains: Vec<ChainConfig>,
}

impl Default for ChainConfig {
    fn default() -> Self {
        Self {
            chain_id: zkclear_types::chain_ids::ETHEREUM,
            rpc_url: std::env::var("RPC_URL")
                .unwrap_or_else(|_| "https://eth.llamarpc.com".to_string()),
            deposit_contract_address: std::env::var("DEPOSIT_CONTRACT_ADDRESS")
                .unwrap_or_else(|_| "0x0000000000000000000000000000000000000000".to_string()),
            required_confirmations: std::env::var("REQUIRED_CONFIRMATIONS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(12),
            poll_interval_seconds: std::env::var("POLL_INTERVAL_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
            rpc_timeout_seconds: std::env::var("RPC_TIMEOUT_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30),
            max_retries: std::env::var("MAX_RETRIES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
            retry_delay_seconds: std::env::var("RETRY_DELAY_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1),
            reorg_safety_blocks: std::env::var("REORG_SAFETY_BLOCKS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(10),
        }
    }
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            chains: vec![
                ChainConfig {
                    chain_id: zkclear_types::chain_ids::ETHEREUM,
                    rpc_url: std::env::var("ETHEREUM_RPC_URL")
                        .unwrap_or_else(|_| "https://eth.llamarpc.com".to_string()),
                    deposit_contract_address: std::env::var("ETHEREUM_DEPOSIT_CONTRACT")
                        .unwrap_or_else(|_| {
                            "0x0000000000000000000000000000000000000000".to_string()
                        }),
                    required_confirmations: 12,
                    poll_interval_seconds: 3,
                    rpc_timeout_seconds: 30,
                    max_retries: 3,
                    retry_delay_seconds: 1,
                    reorg_safety_blocks: 10,
                },
                ChainConfig {
                    chain_id: zkclear_types::chain_ids::BASE,
                    rpc_url: std::env::var("BASE_RPC_URL")
                        .unwrap_or_else(|_| "https://mainnet.base.org".to_string()),
                    deposit_contract_address: std::env::var("BASE_DEPOSIT_CONTRACT")
                        .unwrap_or_else(|_| {
                            "0x0000000000000000000000000000000000000000".to_string()
                        }),
                    required_confirmations: 12,
                    poll_interval_seconds: 3,
                    rpc_timeout_seconds: 30,
                    max_retries: 3,
                    retry_delay_seconds: 1,
                    reorg_safety_blocks: 10,
                },
            ],
        }
    }
}
