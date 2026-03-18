use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// On-demand reader for Sablier & Hedgey vesting NFTs via eth_call
pub struct VestingReader {
    client: reqwest::Client,
    rpc_url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VestingPosition {
    pub platform: String,       // "sablier" or "hedgey"
    pub contract: String,       // NFT contract address
    pub token_id: u64,          // NFT token ID (= stream/plan ID)
    pub token: String,          // underlying ERC-20 address
    pub total_amount: String,   // total deposited (wei string)
    pub withdrawn_amount: String,
    pub withdrawable_amount: String,
    pub start_time: u64,
    pub end_time: u64,
    pub is_transferable: bool,
    pub is_cancelable: bool,
    pub status: String,         // "Pending", "Streaming", "Settled", "Canceled", "Depleted"
}

// ABI function selectors (first 4 bytes of keccak256)
const BALANCE_OF: &str = "70a08231";           // balanceOf(address)
const TOKEN_OF_OWNER_BY_INDEX: &str = "2f745c59"; // tokenOfOwnerByIndex(address,uint256)
// Sablier
const GET_DEPOSITED_AMOUNT: &str = "9067b677"; // getDepositedAmount(uint256)
const GET_WITHDRAWN_AMOUNT: &str = "7ee99376"; // getWithdrawnAmount(uint256)
const WITHDRAWABLE_AMOUNT_OF: &str = "8c8c4c29"; // withdrawableAmountOf(uint256)
const GET_START_TIME: &str = "b681f9f6";       // getStartTime(uint256)
const GET_END_TIME: &str = "4f54d896";         // getEndTime(uint256)
const GET_UNDERLYING_TOKEN: &str = "76b58b90"; // getUnderlyingToken(uint256)
const IS_TRANSFERABLE: &str = "7013dbb6";      // isTransferable(uint256)
const IS_CANCELABLE: &str = "90c4d45f";        // isCancelable(uint256)
const STATUS_OF: &str = "a5c18a32";            // statusOf(uint256)
// Hedgey
const HEDGEY_PLANS: &str = "3557ee77";         // plans(uint256) → (token, amount, start, cliff, rate, period)

impl VestingReader {
    pub fn new(rpc_url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
        Self { client, rpc_url }
    }

    async fn eth_call(&self, to: &str, data: &str) -> Result<String> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{"to": to, "data": format!("0x{}", data)}, "latest"],
            "id": 1
        });

        let response: Value = self.client
            .post(&self.rpc_url)
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;

        if let Some(error) = response.get("error") {
            let msg = error.get("message").and_then(|v| v.as_str()).unwrap_or("RPC error");
            return Err(anyhow!("eth_call error: {}", msg));
        }

        response.get("result")
            .and_then(|v| v.as_str())
            .map(|s| s.trim_start_matches("0x").to_string())
            .ok_or_else(|| anyhow!("Missing result in eth_call response"))
    }

    fn decode_uint256(hex: &str) -> u64 {
        if hex.len() < 64 { return 0; }
        u64::from_str_radix(&hex[hex.len()-16..], 16).unwrap_or(0)
    }

    fn decode_uint128(hex: &str) -> u128 {
        if hex.len() < 64 { return 0; }
        u128::from_str_radix(&hex[hex.len()-32..], 16).unwrap_or(0)
    }

    fn decode_address(hex: &str) -> String {
        if hex.len() < 64 { return "0x0".to_string(); }
        format!("0x{}", &hex[24..64])
    }

    fn decode_bool(hex: &str) -> bool {
        Self::decode_uint256(hex) != 0
    }

    fn encode_address(addr: &str) -> String {
        let clean = addr.trim_start_matches("0x");
        format!("{:0>64}", clean)
    }

    fn encode_uint256(val: u64) -> String {
        format!("{:064x}", val)
    }

    /// Get all Sablier vesting positions for an address
    pub async fn get_sablier_positions(
        &self,
        contract: &str,
        owner: &str,
    ) -> Result<Vec<VestingPosition>> {
        // Get NFT balance
        let data = format!("{}{}", BALANCE_OF, Self::encode_address(owner));
        let result = self.eth_call(contract, &data).await?;
        let count = Self::decode_uint256(&result);

        if count == 0 {
            return Ok(vec![]);
        }

        let mut positions = Vec::new();
        for i in 0..count.min(50) { // cap at 50 to avoid DoS
            // Get token ID
            let data = format!(
                "{}{}{}",
                TOKEN_OF_OWNER_BY_INDEX,
                Self::encode_address(owner),
                Self::encode_uint256(i)
            );
            let result = self.eth_call(contract, &data).await?;
            let token_id = Self::decode_uint256(&result);

            let id_param = Self::encode_uint256(token_id);

            // Pre-build calldata strings (needed for borrow checker with tokio::join!)
            let cd_deposited = format!("{}{}", GET_DEPOSITED_AMOUNT, id_param);
            let cd_withdrawn = format!("{}{}", GET_WITHDRAWN_AMOUNT, id_param);
            let cd_withdrawable = format!("{}{}", WITHDRAWABLE_AMOUNT_OF, id_param);
            let cd_start = format!("{}{}", GET_START_TIME, id_param);
            let cd_end = format!("{}{}", GET_END_TIME, id_param);
            let cd_token = format!("{}{}", GET_UNDERLYING_TOKEN, id_param);
            let cd_transferable = format!("{}{}", IS_TRANSFERABLE, id_param);
            let cd_cancelable = format!("{}{}", IS_CANCELABLE, id_param);
            let cd_status = format!("{}{}", STATUS_OF, id_param);

            // Parallel reads for this stream
            let (deposited, withdrawn, withdrawable, start, end, token, transferable, cancelable, status) = tokio::join!(
                self.eth_call(contract, &cd_deposited),
                self.eth_call(contract, &cd_withdrawn),
                self.eth_call(contract, &cd_withdrawable),
                self.eth_call(contract, &cd_start),
                self.eth_call(contract, &cd_end),
                self.eth_call(contract, &cd_token),
                self.eth_call(contract, &cd_transferable),
                self.eth_call(contract, &cd_cancelable),
                self.eth_call(contract, &cd_status),
            );

            let status_val = status.as_ref().map(|s| Self::decode_uint256(s)).unwrap_or(0);
            let status_str = match status_val {
                0 => "Pending",
                1 => "Streaming",
                2 => "Settled",
                3 => "Canceled",
                4 => "Depleted",
                _ => "Unknown",
            };

            positions.push(VestingPosition {
                platform: "sablier".to_string(),
                contract: contract.to_string(),
                token_id,
                token: token.as_ref().map(|t| Self::decode_address(t)).unwrap_or_default(),
                total_amount: deposited.as_ref().map(|d| Self::decode_uint128(d).to_string()).unwrap_or_default(),
                withdrawn_amount: withdrawn.as_ref().map(|w| Self::decode_uint128(w).to_string()).unwrap_or_default(),
                withdrawable_amount: withdrawable.as_ref().map(|w| Self::decode_uint128(w).to_string()).unwrap_or_default(),
                start_time: start.as_ref().map(|s| Self::decode_uint256(s)).unwrap_or(0),
                end_time: end.as_ref().map(|e| Self::decode_uint256(e)).unwrap_or(0),
                is_transferable: transferable.as_ref().map(|t| Self::decode_bool(t)).unwrap_or(false),
                is_cancelable: cancelable.as_ref().map(|c| Self::decode_bool(c)).unwrap_or(false),
                status: status_str.to_string(),
            });
        }

        Ok(positions)
    }

    /// Get all Hedgey lockup positions for an address
    pub async fn get_hedgey_positions(
        &self,
        contract: &str,
        owner: &str,
    ) -> Result<Vec<VestingPosition>> {
        // Get NFT balance
        let data = format!("{}{}", BALANCE_OF, Self::encode_address(owner));
        let result = self.eth_call(contract, &data).await?;
        let count = Self::decode_uint256(&result);

        if count == 0 {
            return Ok(vec![]);
        }

        let mut positions = Vec::new();
        for i in 0..count.min(50) {
            // Get token ID
            let data = format!(
                "{}{}{}",
                TOKEN_OF_OWNER_BY_INDEX,
                Self::encode_address(owner),
                Self::encode_uint256(i)
            );
            let result = self.eth_call(contract, &data).await?;
            let token_id = Self::decode_uint256(&result);

            // Read plan struct: plans(uint256) → (token, amount, start, cliff, rate, period)
            let data = format!("{}{}", HEDGEY_PLANS, Self::encode_uint256(token_id));
            let result = self.eth_call(contract, &data).await?;

            // Decode 6 slots (each 64 hex chars = 32 bytes)
            if result.len() >= 384 {
                let token_addr = Self::decode_address(&result[0..64]);
                let amount = Self::decode_uint128(&result[64..128]);
                let start = Self::decode_uint256(&result[128..192]);
                let cliff = Self::decode_uint256(&result[192..256]);
                let rate = Self::decode_uint128(&result[256..320]);
                let period = Self::decode_uint256(&result[320..384]);

                // Calculate end time: start + (amount / rate) * period
                let end_time = if rate > 0 && period > 0 {
                    let periods = (amount + rate - 1) / rate; // ceil division
                    start + (periods as u64) * period
                } else {
                    0
                };

                // Determine status
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs();

                let status = if amount == 0 {
                    "Depleted"
                } else if now < start {
                    "Pending"
                } else if now < cliff {
                    "Pending" // before cliff
                } else if now >= end_time && end_time > 0 {
                    "Settled"
                } else {
                    "Streaming"
                };

                positions.push(VestingPosition {
                    platform: "hedgey".to_string(),
                    contract: contract.to_string(),
                    token_id,
                    token: token_addr,
                    total_amount: amount.to_string(),
                    withdrawn_amount: "0".to_string(), // Hedgey doesn't track this in the plan struct
                    withdrawable_amount: "0".to_string(), // Would need planBalanceOf call
                    start_time: start,
                    end_time,
                    is_transferable: true, // TokenLockupPlans are transferable
                    is_cancelable: false,  // Lockup plans cannot be revoked
                    status: status.to_string(),
                });
            }
        }

        Ok(positions)
    }

    /// Get all vesting positions from all supported platforms
    pub async fn get_all_positions(
        &self,
        owner: &str,
        sablier_contracts: &[&str],
        hedgey_contracts: &[&str],
    ) -> Vec<VestingPosition> {
        let mut all = Vec::new();

        for contract in sablier_contracts {
            match self.get_sablier_positions(contract, owner).await {
                Ok(positions) => all.extend(positions),
                Err(e) => tracing::warn!("Failed to read Sablier {}: {}", contract, e),
            }
        }

        for contract in hedgey_contracts {
            match self.get_hedgey_positions(contract, owner).await {
                Ok(positions) => all.extend(positions),
                Err(e) => tracing::warn!("Failed to read Hedgey {}: {}", contract, e),
            }
        }

        all
    }
}
