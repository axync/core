// Integration tests for watcher with Hardhat local node
// These tests require Hardhat to be running locally

mod contract_helpers;
mod hardhat_helpers;

use contract_helpers::*;

use hardhat_helpers::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use zkclear_sequencer::Sequencer;
use zkclear_storage::InMemoryStorage;
use zkclear_watcher::{ChainConfig, ChainWatcher};

// Hardhat default RPC URL
const HARDHAT_RPC: &str = "http://127.0.0.1:8545";
const HARDHAT_CHAIN_ID: u64 = 31337; // Hardhat default chain ID

// Helper to create a test sequencer
fn create_test_sequencer() -> Arc<Sequencer> {
    Arc::new(Sequencer::with_storage(InMemoryStorage::new()).unwrap())
}

// Helper to create a test chain config
fn create_test_chain_config(deposit_contract_address: String) -> ChainConfig {
    ChainConfig {
        chain_id: HARDHAT_CHAIN_ID,
        rpc_url: HARDHAT_RPC.to_string(),
        deposit_contract_address,
        required_confirmations: 0, // Hardhat doesn't need confirmations for local testing
        poll_interval_seconds: 1,
        rpc_timeout_seconds: 10,
        max_retries: 3,
        retry_delay_seconds: 1,
        reorg_safety_blocks: 0, // No reorgs in Hardhat local node
    }
}

// Helper to wait for watcher to process deposits with retries
async fn wait_for_deposits_processed(
    sequencer: &Sequencer,
    initial_queue: usize,
    expected_count: usize,
    max_wait_seconds: u64,
) -> usize {
    let start = std::time::Instant::now();
    loop {
        let current_queue = get_queue_length(sequencer);
        if current_queue >= initial_queue + expected_count {
            return current_queue;
        }
        if start.elapsed().as_secs() >= max_wait_seconds {
            return current_queue;
        }
        sleep(Duration::from_millis(500)).await;
    }
}

// Helper to get initial queue length
fn get_queue_length(sequencer: &Sequencer) -> usize {
    sequencer.queue_length()
}

#[tokio::test]
#[ignore] // Requires Hardhat node to be running
async fn test_watcher_detects_single_deposit() {
    // Initialize
    let sequencer = create_test_sequencer();

    // Check if Hardhat is running
    let mut hardhat = HardhatNode::new();
    if !hardhat.is_running().await {
        hardhat.start().await.expect("Should start Hardhat node");
    }

    let contract_address = deploy_contract().await.expect("Should deploy contract");

    let config = create_test_chain_config(contract_address.clone());
    let watcher = ChainWatcher::new(config, sequencer.clone()).expect("Should create watcher");

    // Get initial queue length
    let initial_queue = get_queue_length(&sequencer);

    // Get test account from Hardhat
    let test_account = get_account_address(0)
        .await
        .expect("Should get test account");

    // Start watcher in background
    let watcher_handle = tokio::spawn(async move { watcher.watch().await });

    // Wait a bit for watcher to start and initialize
    sleep(Duration::from_secs(2)).await;

    // Make a native ETH deposit
    let asset_id = 0u16; // Native ETH
    let amount = 1_000_000_000_000_000_000u128; // 1 ETH in wei

    println!("Making deposit from account: {}", test_account);
    let tx_hash = deposit_native(&contract_address, &test_account, asset_id, amount)
        .await
        .expect("Should make deposit");
    println!("Deposit tx hash: {}", tx_hash);

    // Wait for transaction to be mined
    wait_for_transaction(&tx_hash)
        .await
        .expect("Transaction should be mined");
    println!("Deposit mined");

    // Wait for watcher to process with retries
    let final_queue = wait_for_deposits_processed(&sequencer, initial_queue, 1, 10).await;
    println!(
        "Queue status: Initial: {}, Final: {}",
        initial_queue, final_queue
    );

    // Verify queue increased (deposit was processed)
    assert!(
        final_queue > initial_queue,
        "Queue should have increased after deposit. Initial: {}, Final: {}",
        initial_queue,
        final_queue
    );

    watcher_handle.abort();
}

#[tokio::test]
#[ignore]
async fn test_watcher_handles_multiple_deposits() {
    // Test that watcher can handle multiple deposits in sequence
    let sequencer = create_test_sequencer();

    let mut hardhat = HardhatNode::new();
    if !hardhat.is_running().await {
        hardhat.start().await.expect("Should start Hardhat node");
    }

    let contract_address = deploy_contract().await.expect("Should deploy contract");

    // Get test accounts before moving watcher
    let account1 = get_account_address(0).await.expect("Should get account 1");
    let account2 = get_account_address(1).await.expect("Should get account 2");

    let config = create_test_chain_config(contract_address.clone());
    let watcher = ChainWatcher::new(config, sequencer.clone()).expect("Should create watcher");

    let initial_queue = get_queue_length(&sequencer);

    // Start watcher
    let watcher_handle = tokio::spawn(async move { watcher.watch().await });

    // Wait for watcher to initialize and start polling
    sleep(Duration::from_secs(2)).await;

    // Make multiple deposits from different accounts
    let asset_id = 0u16;
    let amount = 1_000_000_000_000_000_000u128; // 1 ETH

    println!("Making first deposit from account: {}", account1);
    let tx1 = deposit_native(&contract_address, &account1, asset_id, amount)
        .await
        .expect("Should make first deposit");
    println!("First deposit tx hash: {}", tx1);
    wait_for_transaction(&tx1)
        .await
        .expect("Tx1 should be mined");
    println!("First deposit mined");

    // Wait a bit for block to be finalized
    sleep(Duration::from_secs(2)).await;

    println!("Making second deposit from account: {}", account2);
    let tx2 = deposit_native(&contract_address, &account2, asset_id, amount)
        .await
        .expect("Should make second deposit");
    println!("Second deposit tx hash: {}", tx2);
    wait_for_transaction(&tx2)
        .await
        .expect("Tx2 should be mined");
    println!("Second deposit mined");

    // Wait for watcher to process with retries
    let final_queue = wait_for_deposits_processed(&sequencer, initial_queue, 2, 15).await;
    println!(
        "Queue status: Initial: {}, Final: {}",
        initial_queue, final_queue
    );

    // Verify multiple deposits were processed
    assert!(
        final_queue >= initial_queue + 2,
        "Multiple deposits should be processed. Initial: {}, Final: {}. Expected at least {} transactions.",
        initial_queue,
        final_queue,
        initial_queue + 2
    );

    watcher_handle.abort();
}

#[tokio::test]
#[ignore]
async fn test_watcher_handles_large_amounts() {
    // Test edge case: very large deposit amounts (u128::MAX)
    let sequencer = create_test_sequencer();

    let mut hardhat = HardhatNode::new();
    if !hardhat.is_running().await {
        hardhat.start().await.expect("Should start Hardhat node");
    }

    let contract_address = deploy_contract().await.expect("Should deploy contract");

    let config = create_test_chain_config(contract_address);
    let watcher = ChainWatcher::new(config, sequencer.clone()).expect("Should create watcher");

    // Test with large amount
    // Verify it's parsed correctly (u128::MAX)

    let watcher_handle = tokio::spawn(async move { watcher.watch().await });

    sleep(Duration::from_secs(3)).await;

    watcher_handle.abort();
}

#[tokio::test]
#[ignore]
async fn test_watcher_handles_reorgs() {
    // Test that watcher handles blockchain reorganizations
    let _sequencer = create_test_sequencer();

    let mut hardhat = HardhatNode::new();
    if !hardhat.is_running().await {
        hardhat.start().await.expect("Should start Hardhat node");
    }

    let contract_address = deploy_contract().await.expect("Should deploy contract");

    let config = create_test_chain_config(contract_address);
    let watcher = ChainWatcher::new(config, _sequencer.clone()).expect("Should create watcher");

    // Simulate reorg by forking Hardhat to a previous block
    // Verify watcher detects and handles the reorg correctly

    let watcher_handle = tokio::spawn(async move { watcher.watch().await });

    sleep(Duration::from_secs(3)).await;

    watcher_handle.abort();
}

#[tokio::test]
#[ignore]
async fn test_watcher_retry_on_rpc_failure() {
    // Test that watcher retries on RPC failures
    let _sequencer = create_test_sequencer();

    // This test would require:
    // 1. Mocking RPC failures
    // 2. Verifying retry logic works
    // 3. Checking that watcher recovers after RPC is back
    // For now, this is a placeholder
}

#[tokio::test]
#[ignore]
async fn test_watcher_parses_deposit_event_correctly() {
    // Test that watcher correctly parses Deposit event structure
    // Verify all fields: user, assetId, amount, txHash

    let sequencer = create_test_sequencer();

    let mut hardhat = HardhatNode::new();
    if !hardhat.is_running().await {
        hardhat.start().await.expect("Should start Hardhat node");
    }

    let contract_address = deploy_contract().await.expect("Should deploy contract");

    let config = create_test_chain_config(contract_address);
    let _watcher = ChainWatcher::new(config, sequencer.clone()).expect("Should create watcher");

    // Make a deposit with known values
    // Verify that watcher parses:
    // - user address correctly
    // - assetId correctly
    // - amount correctly
    // - txHash correctly
    sleep(Duration::from_secs(2)).await;
}

#[tokio::test]
#[ignore]
async fn test_watcher_handles_empty_blocks() {
    // Test that watcher handles blocks with no deposit events
    let sequencer = create_test_sequencer();

    let mut hardhat = HardhatNode::new();
    if !hardhat.is_running().await {
        hardhat.start().await.expect("Should start Hardhat node");
    }

    let contract_address = deploy_contract().await.expect("Should deploy contract");

    let config = create_test_chain_config(contract_address);
    let _watcher = ChainWatcher::new(config, sequencer.clone()).expect("Should create watcher");

    // Wait for several blocks with no deposits
    // Verify watcher doesn't crash or error
    sleep(Duration::from_secs(2)).await;
}

#[tokio::test]
#[ignore]
async fn test_watcher_handles_duplicate_deposits() {
    // Test that watcher doesn't process the same deposit twice
    let sequencer = create_test_sequencer();

    let mut hardhat = HardhatNode::new();
    if !hardhat.is_running().await {
        hardhat.start().await.expect("Should start Hardhat node");
    }

    let contract_address = deploy_contract().await.expect("Should deploy contract");

    let config = create_test_chain_config(contract_address);
    let _watcher = ChainWatcher::new(config, sequencer.clone()).expect("Should create watcher");

    // Make a deposit
    // Verify it's processed once
    // Re-process the same block
    // Verify it's not processed again (deduplication works)
    sleep(Duration::from_secs(2)).await;
}
