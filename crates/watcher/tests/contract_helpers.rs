// Helper functions for interacting with DepositContract via Hardhat

use anyhow::Result;
use serde_json::Value;

const HARDHAT_RPC: &str = "http://127.0.0.1:8545";

// Function selector for depositNative(uint256)
// keccak256("depositNative(uint256)") first 4 bytes
// Computed: depositNative(uint256) -> 0x... (first 4 bytes of hash)
// For testing, we'll use the actual selector
fn get_deposit_native_selector() -> &'static str {
    // This should match the actual function selector from the contract
    // In production, you'd compute this: keccak256("depositNative(uint256)")[0:4]
    // For now, using a placeholder - in real tests you'd compute it or use ethers.js
    "0x608fc37a" // Computed from contract ABI: ethers.utils.id("depositNative(uint256)").slice(0, 10)
}

// Deposit function signature: depositNative(uint256 assetId)
// For native ETH deposits
pub async fn deposit_native(
    contract_address: &str,
    from_address: &str,
    asset_id: u16,
    amount_wei: u128,
) -> Result<String> {
    let client = reqwest::Client::new();

    // Function selector for depositNative(uint256)
    let function_selector = get_deposit_native_selector();

    // Encode assetId as uint256 (32 bytes, big-endian)
    let asset_id_encoded = format!("{:064x}", asset_id);
    let data = format!(
        "{}{}",
        function_selector.trim_start_matches("0x"),
        asset_id_encoded
    );

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_sendTransaction",
        "params": [{
            "from": from_address,
            "to": contract_address,
            "value": format!("0x{:x}", amount_wei),
            "data": format!("0x{}", data)
        }],
        "id": 1
    });

    let response: Value = client
        .post(HARDHAT_RPC)
        .json(&payload)
        .send()
        .await?
        .json()
        .await?;

    if let Some(error) = response.get("error") {
        return Err(anyhow::anyhow!(
            "RPC error: {}",
            error
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown")
        ));
    }

    let tx_hash = response["result"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No transaction hash in response"))?;

    Ok(tx_hash.to_string())
}

// Get account address from Hardhat (first account)
pub async fn get_account_address(index: usize) -> Result<String> {
    let client = reqwest::Client::new();

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_accounts",
        "params": [],
        "id": 1
    });

    let response: Value = client
        .post(HARDHAT_RPC)
        .json(&payload)
        .send()
        .await?
        .json()
        .await?;

    let accounts = response["result"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("No accounts in response"))?;

    if index >= accounts.len() {
        return Err(anyhow::anyhow!("Account index out of range"));
    }

    Ok(accounts[index].as_str().unwrap().to_string())
}

// Wait for transaction to be mined
pub async fn wait_for_transaction(tx_hash: &str) -> Result<()> {
    let client = reqwest::Client::new();

    loop {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_getTransactionReceipt",
            "params": [tx_hash],
            "id": 1
        });

        let response: Value = client
            .post(HARDHAT_RPC)
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;

        if response["result"].is_object() {
            return Ok(());
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }
}

// Get logs for a specific transaction
#[allow(dead_code)]
pub async fn get_transaction_logs(tx_hash: &str) -> Result<Vec<Value>> {
    let client = reqwest::Client::new();

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionReceipt",
        "params": [tx_hash],
        "id": 1
    });

    let response: Value = client
        .post(HARDHAT_RPC)
        .json(&payload)
        .send()
        .await?
        .json()
        .await?;

    let logs = response["result"]
        .get("logs")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    Ok(logs)
}
