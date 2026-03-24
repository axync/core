use std::path::PathBuf;
use axync_prover::ProverConfig;
use axync_watcher::{WatcherConfig, ChainConfig};

/// Parse an env var with a default value
fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Parse an optional env var
fn env_opt(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|s| !s.is_empty())
}

/// Parse a comma-separated env var into a Vec<String>
fn env_list(key: &str, default: &str) -> Vec<String> {
    std::env::var(key)
        .unwrap_or_else(|_| default.to_string())
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

// ── Server ──────────────────────────────────────────────

pub fn port() -> u16 {
    env_or("PORT", 8080)
}

pub fn block_interval_sec() -> u64 {
    env_or("BLOCK_INTERVAL_SEC", 5)
}

pub fn storage_path() -> PathBuf {
    std::env::var("STORAGE_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("./data"))
}

// ── Rate Limiting ───────────────────────────────────────

pub fn rate_limit_max_requests() -> u32 {
    env_or("RATE_LIMIT_MAX_REQUESTS", 100)
}

pub fn rate_limit_window_seconds() -> u64 {
    env_or("RATE_LIMIT_WINDOW_SECONDS", 60)
}

// ── Prover ──────────────────────────────────────────────

pub fn prover_config() -> ProverConfig {
    ProverConfig {
        use_placeholders: env_or("USE_PLACEHOLDER_PROVER", false),
        groth16_keys_dir: env_opt("GROTH16_KEYS_DIR").map(PathBuf::from),
        force_regenerate_keys: env_or("FORCE_REGENERATE_KEYS", false),
        ..Default::default()
    }
}

// ── Marketplace / Vesting ───────────────────────────────

pub fn marketplace_rpc() -> String {
    env_opt("MARKETPLACE_RPC_URL")
        .or_else(|| env_opt("ETHEREUM_RPC_URL"))
        .unwrap_or_else(|| "https://ethereum-sepolia-rpc.publicnode.com".to_string())
}

pub fn escrow_contract() -> Option<String> {
    env_opt("ESCROW_CONTRACT")
}

pub fn sablier_contracts() -> Vec<String> {
    env_list("SABLIER_CONTRACTS", "")
}

pub fn hedgey_contracts() -> Vec<String> {
    env_list("HEDGEY_CONTRACTS", "")
}

// ── Watcher / Chains ────────────────────────────────────

/// Common watcher settings shared across chains
struct CommonWatcher {
    poll_interval: u64,
    rpc_timeout: u64,
    max_retries: u32,
    retry_delay: u64,
    reorg_safety: u64,
}

fn common_watcher() -> CommonWatcher {
    CommonWatcher {
        poll_interval: env_or("POLL_INTERVAL_SECONDS", 15),
        rpc_timeout: env_or("RPC_TIMEOUT_SECONDS", 30),
        max_retries: env_or("MAX_RETRIES", 3),
        retry_delay: env_or("RETRY_DELAY_SECONDS", 2),
        reorg_safety: env_or("REORG_SAFETY_BLOCKS", 5),
    }
}

fn chain_config(
    chain_id: u64,
    rpc_url: String,
    vault_key: &str,
    escrow_key: &str,
    start_key: &str,
    confirmations_key: &str,
    common: &CommonWatcher,
) -> ChainConfig {
    ChainConfig {
        chain_id,
        rpc_url,
        vault_contract_address: env_opt(vault_key)
            .unwrap_or_else(|| "0x0000000000000000000000000000000000000000".to_string()),
        escrow_contract_address: env_opt(escrow_key),
        required_confirmations: env_or(confirmations_key, 2),
        poll_interval_seconds: common.poll_interval,
        rpc_timeout_seconds: common.rpc_timeout,
        max_retries: common.max_retries,
        retry_delay_seconds: common.retry_delay,
        reorg_safety_blocks: common.reorg_safety,
        start_block: env_or(start_key, 0),
    }
}

pub fn watcher_config() -> WatcherConfig {
    let common = common_watcher();

    // Multi-chain mode (testnet/mainnet)
    if env_opt("ETHEREUM_RPC_URL").is_some() || env_opt("BASE_RPC_URL").is_some() {
        let mut chains = Vec::new();

        if let Some(rpc) = env_opt("ETHEREUM_RPC_URL") {
            let chain_id = env_or("ETHEREUM_CHAIN_ID", axync_types::chain_ids::ETHEREUM);
            chains.push(chain_config(
                chain_id, rpc,
                "ETHEREUM_DEPOSIT_CONTRACT",
                "ETHEREUM_ESCROW_CONTRACT",
                "ETHEREUM_START_BLOCK",
                "ETHEREUM_REQUIRED_CONFIRMATIONS",
                &common,
            ));
        }

        if let Some(rpc) = env_opt("BASE_RPC_URL") {
            let chain_id = env_or("BASE_CHAIN_ID", axync_types::chain_ids::BASE);
            chains.push(chain_config(
                chain_id, rpc,
                "BASE_DEPOSIT_CONTRACT",
                "BASE_ESCROW_CONTRACT",
                "BASE_START_BLOCK",
                "BASE_REQUIRED_CONFIRMATIONS",
                &common,
            ));
        }

        WatcherConfig { chains }
    }
    // Local dev mode (single chain)
    else if let Some(rpc) = env_opt("RPC_URL") {
        WatcherConfig {
            chains: vec![ChainConfig {
                chain_id: env_or("CHAIN_ID", 31337),
                rpc_url: rpc,
                vault_contract_address: env_opt("VAULT_CONTRACT_ADDRESS")
                    .unwrap_or_else(|| "0x0000000000000000000000000000000000000000".to_string()),
                escrow_contract_address: env_opt("ESCROW_CONTRACT_ADDRESS"),
                required_confirmations: env_or("REQUIRED_CONFIRMATIONS", 0),
                poll_interval_seconds: env_or("POLL_INTERVAL_SECONDS", 3),
                rpc_timeout_seconds: common.rpc_timeout,
                max_retries: common.max_retries,
                retry_delay_seconds: 1,
                reorg_safety_blocks: 0,
                start_block: env_or("START_BLOCK", 0),
            }],
        }
    }
    // Default (mainnet)
    else {
        WatcherConfig::default()
    }
}
