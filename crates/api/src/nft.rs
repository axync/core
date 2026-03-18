use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Generic ERC-721 metadata reader — works with any NFT contract
pub struct NftReader {
    client: reqwest::Client,
    rpc_url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NftMetadata {
    pub contract: String,
    pub token_id: u64,
    pub name: String,       // collection name
    pub symbol: String,     // collection symbol
    pub token_uri: String,  // tokenURI (may be IPFS, HTTP, or data URI)
    pub image: String,      // resolved image URL (from tokenURI JSON)
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NftPosition {
    pub contract: String,
    pub token_id: u64,
    pub name: String,
    pub symbol: String,
    pub token_uri: String,
}

// Function selectors
const NAME: &str = "06fdde03";                    // name()
const SYMBOL: &str = "95d89b41";                  // symbol()
const TOKEN_URI: &str = "c87b56dd";               // tokenURI(uint256)
const BALANCE_OF: &str = "70a08231";              // balanceOf(address)
const TOKEN_OF_OWNER_BY_INDEX: &str = "2f745c59"; // tokenOfOwnerByIndex(address,uint256)
const SUPPORTS_INTERFACE: &str = "01ffc9a7";      // supportsInterface(bytes4)

const ERC721_INTERFACE_ID: &str = "80ac58cd";
const ERC721_ENUMERABLE_ID: &str = "780e9d63";

impl NftReader {
    pub fn new(rpc_url: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
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
            .ok_or_else(|| anyhow!("Missing result"))
    }

    fn encode_address(addr: &str) -> String {
        let clean = addr.trim_start_matches("0x");
        format!("{:0>64}", clean)
    }

    fn encode_uint256(val: u64) -> String {
        format!("{:064x}", val)
    }

    fn encode_bytes4(selector: &str) -> String {
        format!("{}{}", selector, "0".repeat(56))
    }

    fn decode_uint256(hex: &str) -> u64 {
        if hex.len() < 64 { return 0; }
        u64::from_str_radix(&hex[hex.len()-16..], 16).unwrap_or(0)
    }

    fn decode_bool(hex: &str) -> bool {
        Self::decode_uint256(hex) != 0
    }

    /// Decode a dynamic string from ABI-encoded hex
    fn decode_string(hex: &str) -> String {
        if hex.len() < 128 { return String::new(); }

        // First 32 bytes = offset, second 32 bytes = length
        let len_hex = &hex[64..128];
        let len = usize::from_str_radix(&len_hex[len_hex.len().saturating_sub(8)..], 16).unwrap_or(0);

        if len == 0 || hex.len() < 128 + len * 2 {
            return String::new();
        }

        let data_hex = &hex[128..128 + len * 2];
        let bytes: Vec<u8> = (0..data_hex.len())
            .step_by(2)
            .filter_map(|i| u8::from_str_radix(&data_hex[i..i+2], 16).ok())
            .collect();

        String::from_utf8_lossy(&bytes).to_string()
    }

    /// Check if a contract supports ERC-721
    pub async fn is_erc721(&self, contract: &str) -> bool {
        let data = format!("{}{}", SUPPORTS_INTERFACE, Self::encode_bytes4(ERC721_INTERFACE_ID));
        self.eth_call(contract, &data).await
            .map(|r| Self::decode_bool(&r))
            .unwrap_or(false)
    }

    /// Check if a contract supports ERC-721 Enumerable
    pub async fn is_enumerable(&self, contract: &str) -> bool {
        let data = format!("{}{}", SUPPORTS_INTERFACE, Self::encode_bytes4(ERC721_ENUMERABLE_ID));
        self.eth_call(contract, &data).await
            .map(|r| Self::decode_bool(&r))
            .unwrap_or(false)
    }

    /// Read collection name and symbol
    pub async fn get_collection_info(&self, contract: &str) -> (String, String) {
        let (name, symbol) = tokio::join!(
            self.eth_call(contract, NAME),
            self.eth_call(contract, SYMBOL),
        );

        (
            name.as_ref().map(|n| Self::decode_string(n)).unwrap_or_default(),
            symbol.as_ref().map(|s| Self::decode_string(s)).unwrap_or_default(),
        )
    }

    /// Read tokenURI for a specific token
    pub async fn get_token_uri(&self, contract: &str, token_id: u64) -> String {
        let data = format!("{}{}", TOKEN_URI, Self::encode_uint256(token_id));
        self.eth_call(contract, &data).await
            .map(|r| Self::decode_string(&r))
            .unwrap_or_default()
    }

    /// Get full metadata for a single NFT
    pub async fn get_metadata(&self, contract: &str, token_id: u64) -> Result<NftMetadata> {
        let (name, symbol) = self.get_collection_info(contract).await;
        let token_uri = self.get_token_uri(contract, token_id).await;

        // Try to resolve image from tokenURI
        let image = self.resolve_image(&token_uri).await;

        Ok(NftMetadata {
            contract: contract.to_string(),
            token_id,
            name,
            symbol,
            token_uri,
            image,
        })
    }

    /// Get all NFTs owned by address from a specific contract (ERC-721 Enumerable)
    pub async fn get_owned_nfts(
        &self,
        contract: &str,
        owner: &str,
    ) -> Result<Vec<NftPosition>> {
        // Get balance
        let data = format!("{}{}", BALANCE_OF, Self::encode_address(owner));
        let result = self.eth_call(contract, &data).await?;
        let count = Self::decode_uint256(&result);

        if count == 0 {
            return Ok(vec![]);
        }

        // Get collection info once
        let (name, symbol) = self.get_collection_info(contract).await;

        let mut positions = Vec::new();
        for i in 0..count.min(50) {
            let data = format!(
                "{}{}{}",
                TOKEN_OF_OWNER_BY_INDEX,
                Self::encode_address(owner),
                Self::encode_uint256(i)
            );

            match self.eth_call(contract, &data).await {
                Ok(result) => {
                    let token_id = Self::decode_uint256(&result);
                    let token_uri = self.get_token_uri(contract, token_id).await;

                    positions.push(NftPosition {
                        contract: contract.to_string(),
                        token_id,
                        name: name.clone(),
                        symbol: symbol.clone(),
                        token_uri,
                    });
                }
                Err(e) => {
                    tracing::warn!("Failed to read token {} from {}: {}", i, contract, e);
                }
            }
        }

        Ok(positions)
    }

    /// Try to resolve image URL from tokenURI JSON metadata
    async fn resolve_image(&self, token_uri: &str) -> String {
        if token_uri.is_empty() {
            return String::new();
        }

        // Handle data URIs (base64-encoded JSON)
        if token_uri.starts_with("data:application/json;base64,") {
            let b64 = &token_uri["data:application/json;base64,".len()..];
            if let Ok(decoded) = base64_decode(b64) {
                if let Ok(json) = serde_json::from_str::<Value>(&decoded) {
                    return json.get("image")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                }
            }
            return String::new();
        }

        // Handle HTTP(S) URIs
        let url = if token_uri.starts_with("ipfs://") {
            format!("https://ipfs.io/ipfs/{}", &token_uri[7..])
        } else if token_uri.starts_with("http") {
            token_uri.to_string()
        } else {
            return String::new();
        };

        // Fetch and parse JSON
        match self.client.get(&url).send().await {
            Ok(resp) => {
                match resp.json::<Value>().await {
                    Ok(json) => {
                        let img = json.get("image")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        // Convert IPFS image URLs
                        if img.starts_with("ipfs://") {
                            format!("https://ipfs.io/ipfs/{}", &img[7..])
                        } else {
                            img
                        }
                    }
                    Err(_) => String::new(),
                }
            }
            Err(_) => String::new(),
        }
    }
}

fn base64_decode(input: &str) -> Result<String> {
    // Simple base64 decoder
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut output = Vec::new();
    let input = input.as_bytes();
    let mut buf: u32 = 0;
    let mut bits: u32 = 0;

    for &b in input {
        if b == b'=' || b == b'\n' || b == b'\r' { continue; }
        let val = TABLE.iter().position(|&c| c == b)
            .ok_or_else(|| anyhow!("Invalid base64"))? as u32;
        buf = (buf << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }

    String::from_utf8(output).map_err(|e| anyhow!("Invalid UTF-8: {}", e))
}
