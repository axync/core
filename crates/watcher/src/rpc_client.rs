use crate::config::ChainConfig;
use anyhow::Result;
use serde_json::Value;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, error, warn};

pub struct RpcClient {
    client: reqwest::Client,
    config: ChainConfig,
}

impl RpcClient {
    pub fn new(config: ChainConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.rpc_timeout_seconds))
            .build()
            .expect("Failed to create HTTP client");

        Self { client, config }
    }

    pub async fn call(&self, method: &str, params: Value) -> Result<Value> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        });

        let mut last_error = None;

        for attempt in 0..=self.config.max_retries {
            if attempt > 0 {
                let delay = Duration::from_secs(self.config.retry_delay_seconds * attempt as u64);
                warn!(
                    "RPC call failed, retrying in {}s (attempt {}/{}): {}",
                    delay.as_secs(),
                    attempt,
                    self.config.max_retries,
                    last_error
                        .as_ref()
                        .map(|e: &anyhow::Error| e.to_string())
                        .unwrap_or_default()
                );
                sleep(delay).await;
            }

            match self.try_call(&payload).await {
                Ok(response) => {
                    if attempt > 0 {
                        debug!("RPC call succeeded after {} retries", attempt);
                    }
                    return Ok(response);
                }
                Err(e) => {
                    last_error = Some(e);
                    if attempt == self.config.max_retries {
                        error!("RPC call failed after {} retries", self.config.max_retries);
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("RPC call failed")))
    }

    async fn try_call(&self, payload: &Value) -> Result<Value> {
        let response: Value = self
            .client
            .post(&self.config.rpc_url)
            .json(payload)
            .send()
            .await?
            .json()
            .await?;

        if let Some(error) = response.get("error") {
            let error_msg = error
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown RPC error");
            let error_code = error.get("code").and_then(|v| v.as_i64()).unwrap_or(-1);

            // Handle rate limiting
            if error_code == -32005 || error_code == 429 {
                return Err(anyhow::anyhow!("Rate limited: {}", error_msg));
            }

            return Err(anyhow::anyhow!("RPC error ({}): {}", error_code, error_msg));
        }

        Ok(response)
    }

    pub async fn get_block_number(&self) -> Result<u64> {
        let response = self.call("eth_blockNumber", serde_json::json!([])).await?;

        let hex_str = response
            .get("result")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Invalid response format: missing result"))?;

        let block_num = u64::from_str_radix(hex_str.trim_start_matches("0x"), 16)
            .map_err(|e| anyhow::anyhow!("Failed to parse block number: {}", e))?;

        Ok(block_num)
    }

    pub async fn get_logs(
        &self,
        from_block: u64,
        to_block: u64,
        address: &str,
    ) -> Result<Vec<Value>> {
        let params = serde_json::json!([{
            "fromBlock": format!("0x{:x}", from_block),
            "toBlock": format!("0x{:x}", to_block),
            "address": address
        }]);

        let response = self.call("eth_getLogs", params).await?;

        let logs = response
            .get("result")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        Ok(logs)
    }
}
