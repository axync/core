// Helper functions for integration tests with Hardhat

use anyhow::Result;
use serde_json::Value;
use std::process::{Command, Stdio};
use std::time::Duration;
use tokio::time::sleep;
use zkclear_watcher::{ChainConfig, RpcClient};

const HARDHAT_RPC: &str = "http://127.0.0.1:8545";

pub struct HardhatNode {
    process: Option<std::process::Child>,
}

impl HardhatNode {
    pub fn new() -> Self {
        Self { process: None }
    }

    pub async fn start(&mut self) -> Result<()> {
        // Check if Hardhat is already running
        if self.is_running().await {
            return Ok(());
        }

        // Start Hardhat node
        // Note: Path is relative to workspace root
        let contracts_dir = std::env::var("CONTRACTS_DIR")
            .unwrap_or_else(|_| "../../../../contracts/zkclear-contracts".to_string());

        let cmd = Command::new("npx")
            .args(&["hardhat", "node"])
            .current_dir(&contracts_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Wait for node to be ready
        for _ in 0..30 {
            if self.is_running().await {
                self.process = Some(cmd);
                return Ok(());
            }
            sleep(Duration::from_millis(500)).await;
        }

        Err(anyhow::anyhow!("Hardhat node failed to start"))
    }

    pub async fn is_running(&self) -> bool {
        let config = ChainConfig {
            chain_id: 31337,
            rpc_url: HARDHAT_RPC.to_string(),
            deposit_contract_address: "0x0".to_string(),
            required_confirmations: 1,
            poll_interval_seconds: 1,
            rpc_timeout_seconds: 5,
            max_retries: 1,
            retry_delay_seconds: 1,
            reorg_safety_blocks: 2,
        };
        let client = RpcClient::new(config);
        client.get_block_number().await.is_ok()
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for HardhatNode {
    fn drop(&mut self) {
        self.stop();
    }
}

pub async fn deploy_contract() -> Result<String> {
    // Deploy DepositContract via Hardhat
    let contracts_dir = std::env::var("CONTRACTS_DIR")
        .unwrap_or_else(|_| "../../../../contracts/zkclear-contracts".to_string());

    // Try to read from environment first
    if let Ok(address) = std::env::var("TEST_DEPOSIT_CONTRACT_ADDRESS") {
        return Ok(address);
    }

    // Deploy via Hardhat
    let output = Command::new("npx")
        .args(&[
            "hardhat",
            "run",
            "scripts/deploy.js",
            "--network",
            "localhost",
        ])
        .current_dir(&contracts_dir)
        .output()?;

    if !output.status.success() {
        return Err(anyhow::anyhow!(
            "Failed to deploy contract: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Parse contract address from output
    // Hardhat deploy script outputs: "DepositContract deployed to: 0x..."
    let output_str = String::from_utf8_lossy(&output.stdout);
    for line in output_str.lines() {
        if line.contains("DepositContract deployed to:") {
            if let Some(addr) = line.split(":").nth(1) {
                return Ok(addr.trim().to_string());
            }
        }
    }

    // Fallback to default Hardhat address
    Ok("0x5FbDB2315678afecb367f032d93F642f64180aa3".to_string())
}

#[allow(dead_code)]
pub async fn call_contract_function(
    contract_address: &str,
    function_signature: &str,
    _params: Vec<String>,
) -> Result<Value> {
    // Call contract function via JSON-RPC
    let client = reqwest::Client::new();

    // This is simplified - in reality you'd need proper ABI encoding
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_call",
        "params": [{
            "to": contract_address,
            "data": format!("0x{}", function_signature)
        }, "latest"],
        "id": 1
    });

    let response: Value = client
        .post(HARDHAT_RPC)
        .json(&payload)
        .send()
        .await?
        .json()
        .await?;

    Ok(response)
}

#[allow(dead_code)]
pub async fn send_transaction(
    from: &str,
    to: &str,
    data: &str,
    value: Option<u128>,
) -> Result<String> {
    let client = reqwest::Client::new();

    let mut tx = serde_json::json!({
        "from": from,
        "to": to,
        "data": data,
    });

    if let Some(v) = value {
        tx["value"] = serde_json::json!(format!("0x{:x}", v));
    }

    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_sendTransaction",
        "params": [tx],
        "id": 1
    });

    let response: Value = client
        .post(HARDHAT_RPC)
        .json(&payload)
        .send()
        .await?
        .json()
        .await?;

    let tx_hash = response["result"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("No transaction hash in response"))?;

    Ok(tx_hash.to_string())
}

#[allow(dead_code)]
pub async fn wait_for_block(block_number: u64) -> Result<()> {
    let client = reqwest::Client::new();

    loop {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
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

        let current_block = u64::from_str_radix(
            response["result"]
                .as_str()
                .unwrap_or("0x0")
                .trim_start_matches("0x"),
            16,
        )
        .map_err(|e| anyhow::anyhow!("Failed to parse block number: {}", e))?;

        if current_block >= block_number {
            return Ok(());
        }

        sleep(Duration::from_millis(100)).await;
    }
}
