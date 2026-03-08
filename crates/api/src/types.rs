use serde::{Deserialize, Deserializer, Serialize};
use zkclear_types::{Address, AssetId, BlockId, DealId};

// Helper to deserialize u128 from string (JSON doesn't support numbers > 2^53)
fn deserialize_u128_from_string<'de, D>(deserializer: D) -> Result<u128, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{Error, Visitor};
    use std::fmt;

    struct U128Visitor;

    impl<'de> Visitor<'de> for U128Visitor {
        type Value = u128;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or number representing u128")
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: Error,
        {
            v.parse().map_err(Error::custom)
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(v as u128)
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            if v < 0 {
                Err(Error::custom("u128 cannot be negative"))
            } else {
                Ok(v as u128)
            }
        }
    }

    deserializer.deserialize_any(U128Visitor)
}

// Helper to deserialize Option<u128> from string or null
fn deserialize_option_u128_from_string<'de, D>(deserializer: D) -> Result<Option<u128>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::{Error, Visitor};
    use std::fmt;

    struct OptionU128Visitor;

    impl<'de> Visitor<'de> for OptionU128Visitor {
        type Value = Option<u128>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("an optional string or number representing u128, or null")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserialize_u128_from_string(deserializer).map(Some)
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: Error,
        {
            v.parse().map(Some).map_err(Error::custom)
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            Ok(Some(v as u128))
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
        where
            E: Error,
        {
            if v < 0 {
                Err(Error::custom("u128 cannot be negative"))
            } else {
                Ok(Some(v as u128))
            }
        }
    }

    // Use deserialize_option for Option types - it properly handles null
    deserializer.deserialize_option(OptionU128Visitor)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountBalanceResponse {
    pub address: Address,
    pub asset_id: AssetId,
    pub chain_id: zkclear_types::ChainId,
    pub amount: u128,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AccountStateResponse {
    pub address: Address,
    pub account_id: u64,
    pub balances: Vec<BalanceInfo>,
    pub nonce: u64,
    pub open_deals: Vec<DealId>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BalanceInfo {
    pub asset_id: AssetId,
    pub chain_id: zkclear_types::ChainId,
    pub amount: u128,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DealDetailsResponse {
    pub deal_id: DealId,
    pub maker: Address,
    pub taker: Option<Address>,
    pub asset_base: AssetId,
    pub asset_quote: AssetId,
    pub chain_id_base: zkclear_types::ChainId,
    pub chain_id_quote: zkclear_types::ChainId,
    pub amount_base: u128,
    pub amount_remaining: u128,
    pub price_quote_per_base: u128,
    pub status: String,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    pub is_cross_chain: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DealListResponse {
    pub deals: Vec<DealDetailsResponse>,
    pub total: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockInfoResponse {
    pub block_id: BlockId,
    pub transaction_count: usize,
    pub timestamp: u64,
    pub transactions: Vec<TransactionInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionInfo {
    pub id: u64,
    pub from: Address,
    pub nonce: u64,
    pub kind: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct QueueStatusResponse {
    pub pending_transactions: usize,
    pub max_queue_size: usize,
    pub current_block_id: BlockId,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitTxRequest {
    pub tx: String,
}

#[allow(dead_code)]
#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitTxResponse {
    pub tx_hash: String,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: serde_json::Value,
    pub id: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub result: Option<serde_json::Value>,
    pub error: Option<JsonRpcError>,
    pub id: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
    pub message: String,
}

// Transaction submission types
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SubmitTransactionRequest {
    Deposit {
        tx_hash: String, // hex string
        account: String, // hex string
        asset_id: AssetId,
        #[serde(deserialize_with = "deserialize_u128_from_string")]
        amount: u128,
        chain_id: zkclear_types::ChainId,
        nonce: u64,
        signature: String, // hex string (65 bytes)
    },
    CreateDeal {
        from: String, // hex string
        deal_id: DealId,
        visibility: String, // "Public" or "Direct"
        taker: Option<String>, // hex string
        asset_base: AssetId,
        asset_quote: AssetId,
        chain_id_base: zkclear_types::ChainId,
        chain_id_quote: zkclear_types::ChainId,
        #[serde(deserialize_with = "deserialize_u128_from_string")]
        amount_base: u128,
        #[serde(deserialize_with = "deserialize_u128_from_string")]
        price_quote_per_base: u128,
        expires_at: Option<u64>,
        external_ref: Option<String>,
        nonce: u64,
        signature: String, // hex string (65 bytes)
    },
    AcceptDeal {
        from: String, // hex string
        deal_id: DealId,
        #[serde(deserialize_with = "deserialize_option_u128_from_string")]
        amount: Option<u128>,
        nonce: u64,
        signature: String, // hex string (65 bytes)
    },
    CancelDeal {
        from: String, // hex string
        deal_id: DealId,
        nonce: u64,
        signature: String, // hex string (65 bytes)
    },
    Withdraw {
        from: String, // hex string
        asset_id: AssetId,
        #[serde(deserialize_with = "deserialize_u128_from_string")]
        amount: u128,
        to: String, // hex string
        chain_id: zkclear_types::ChainId,
        nonce: u64,
        signature: String, // hex string (65 bytes)
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitTransactionResponse {
    pub tx_hash: String,
    pub status: String,
}
