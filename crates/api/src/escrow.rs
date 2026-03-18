use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// On-demand reader for EscrowSwap contract listings
pub struct EscrowReader {
    client: reqwest::Client,
    rpc_url: String,
    contract: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Listing {
    pub id: u64,
    pub seller: String,
    pub nft_contract: String,
    pub token_id: u64,
    pub payment_token: String,  // "0x0000..." for ETH
    pub price: String,          // wei string
    pub active: bool,
}

// Function selectors
const NEXT_LISTING_ID: &str = "60a2da44";      // nextListingId()
const GET_LISTING: &str = "107a274a";          // getListing(uint256)
const IS_ACTIVE: &str = "82afd23b";            // isActive(uint256)

impl EscrowReader {
    pub fn new(rpc_url: String, contract: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
        Self { client, rpc_url, contract }
    }

    async fn eth_call(&self, data: &str) -> Result<String> {
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "eth_call",
            "params": [{"to": &self.contract, "data": format!("0x{}", data)}, "latest"],
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
            .ok_or_else(|| anyhow!("Missing result"))
    }

    fn decode_uint256(hex: &str, offset: usize) -> u64 {
        let start = offset * 64;
        let end = start + 64;
        if hex.len() < end { return 0; }
        u64::from_str_radix(&hex[start + 48..end], 16).unwrap_or(0)
    }

    fn decode_uint256_big(hex: &str, offset: usize) -> u128 {
        let start = offset * 64;
        let end = start + 64;
        if hex.len() < end { return 0; }
        u128::from_str_radix(&hex[start + 32..end], 16).unwrap_or(0)
    }

    fn decode_address(hex: &str, offset: usize) -> String {
        let start = offset * 64;
        if hex.len() < start + 64 { return "0x0".to_string(); }
        format!("0x{}", &hex[start + 24..start + 64])
    }

    fn decode_bool(hex: &str, offset: usize) -> bool {
        Self::decode_uint256(hex, offset) != 0
    }

    fn encode_uint256(val: u64) -> String {
        format!("{:064x}", val)
    }

    pub fn contract_address(&self) -> &str {
        &self.contract
    }

    /// Get total listing count
    pub async fn get_listing_count(&self) -> Result<u64> {
        let result = self.eth_call(NEXT_LISTING_ID).await?;
        Ok(Self::decode_uint256(&result, 0))
    }

    /// Get a single listing by ID
    pub async fn get_listing(&self, id: u64) -> Result<Listing> {
        let data = format!("{}{}", GET_LISTING, Self::encode_uint256(id));
        let result = self.eth_call(&data).await?;

        // getListing returns: (address seller, address nftContract, uint256 tokenId, address paymentToken, uint256 price, bool active)
        // = 6 slots
        if result.len() < 384 {
            return Err(anyhow!("Invalid listing response"));
        }

        Ok(Listing {
            id,
            seller: Self::decode_address(&result, 0),
            nft_contract: Self::decode_address(&result, 1),
            token_id: Self::decode_uint256(&result, 2),
            payment_token: Self::decode_address(&result, 3),
            price: Self::decode_uint256_big(&result, 4).to_string(),
            active: Self::decode_bool(&result, 5),
        })
    }

    /// Get all active listings
    pub async fn get_active_listings(&self) -> Result<Vec<Listing>> {
        let count = self.get_listing_count().await?;
        let mut listings = Vec::new();

        for id in 0..count.min(200) { // cap at 200
            match self.get_listing(id).await {
                Ok(listing) if listing.active => listings.push(listing),
                Ok(_) => {} // inactive, skip
                Err(e) => tracing::warn!("Failed to read listing {}: {}", id, e),
            }
        }

        Ok(listings)
    }
}
