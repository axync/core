mod constants;

pub use constants::*;

pub type AccountId = u64;
pub type DealId = u64;
pub type AssetId = u16;
pub type BlockId = u64;
pub type ChainId = u64;

pub type Address = [u8; constants::address::ADDRESS_SIZE];
pub type Signature = [u8; constants::signature::SIGNATURE_SIZE];

pub const ZERO_ADDRESS: Address = constants::address::ZERO_ADDRESS_BYTES;

#[repr(u64)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum SupportedChain {
    Ethereum = constants::chain_ids::ETHEREUM,
    Polygon = constants::chain_ids::POLYGON,
    Mantle = constants::chain_ids::MANTLE,
    Arbitrum = constants::chain_ids::ARBITRUM,
    Optimism = constants::chain_ids::OPTIMISM,
    Base = constants::chain_ids::BASE,
}

impl SupportedChain {
    pub fn as_chain_id(&self) -> ChainId {
        match self {
            SupportedChain::Ethereum => constants::chain_ids::ETHEREUM,
            SupportedChain::Polygon => constants::chain_ids::POLYGON,
            SupportedChain::Mantle => constants::chain_ids::MANTLE,
            SupportedChain::Arbitrum => constants::chain_ids::ARBITRUM,
            SupportedChain::Optimism => constants::chain_ids::OPTIMISM,
            SupportedChain::Base => constants::chain_ids::BASE,
        }
    }

    pub fn from_chain_id(chain_id: ChainId) -> Option<Self> {
        match chain_id {
            constants::chain_ids::ETHEREUM => Some(SupportedChain::Ethereum),
            constants::chain_ids::POLYGON => Some(SupportedChain::Polygon),
            constants::chain_ids::MANTLE => Some(SupportedChain::Mantle),
            constants::chain_ids::ARBITRUM => Some(SupportedChain::Arbitrum),
            constants::chain_ids::OPTIMISM => Some(SupportedChain::Optimism),
            constants::chain_ids::BASE => Some(SupportedChain::Base),
            _ => None,
        }
    }

    pub fn is_supported(chain_id: ChainId) -> bool {
        Self::from_chain_id(chain_id).is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DealVisibility {
    Public,
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DealStatus {
    Pending,
    Settled,
    Cancelled,
    Expired,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Account {
    pub id: AccountId,
    #[serde(with = "serde_bytes")]
    pub owner: Address,
    pub balances: Vec<Balance>,
    pub nonce: u64,
    pub created_at: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Balance {
    pub asset_id: AssetId,
    pub amount: u128,
    pub chain_id: ChainId,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Asset {
    pub id: AssetId,
    pub symbol: String,
    pub decimals: u8,
    pub chain_id: ChainId,
    pub contract_address: Option<Address>,
    pub is_wrapped: bool,
    pub original_chain_id: Option<ChainId>,
}

// Note: For asset mapping across chains, one asset_id can have different contract_address
// on different chains. This is managed in the asset registry (State or separate storage).
// Example: USDC (asset_id=1) has different addresses on Ethereum, Polygon, Base, etc.

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Deal {
    pub id: DealId,
    pub maker: Address,
    pub taker: Option<Address>,
    pub visibility: DealVisibility,
    pub asset_base: AssetId,
    pub asset_quote: AssetId,
    pub chain_id_base: ChainId,
    pub chain_id_quote: ChainId,
    pub amount_base: u128,
    pub amount_remaining: u128,
    pub price_quote_per_base: u128,
    pub status: DealStatus,
    pub created_at: u64,
    pub expires_at: Option<u64>,
    pub external_ref: Option<String>,
    pub is_cross_chain: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TxKind {
    Deposit,
    CreateDeal,
    AcceptDeal,
    CancelDeal,
    Withdraw,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Tx {
    pub id: u64,
    #[serde(with = "serde_bytes")]
    pub from: Address,
    pub nonce: u64,
    pub kind: TxKind,
    pub payload: TxPayload,
    #[serde(with = "serde_bytes")]
    pub signature: Signature,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TxPayload {
    Deposit(Deposit),
    CreateDeal(CreateDeal),
    AcceptDeal(AcceptDeal),
    CancelDeal(CancelDeal),
    Withdraw(Withdraw),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Deposit {
    #[serde(with = "serde_bytes")]
    pub tx_hash: [u8; constants::transaction::TX_HASH_SIZE],
    #[serde(with = "serde_bytes")]
    pub account: Address,
    pub asset_id: AssetId,
    pub amount: u128,
    pub chain_id: ChainId,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreateDeal {
    pub deal_id: DealId,
    pub visibility: DealVisibility,
    pub taker: Option<Address>,
    pub asset_base: AssetId,
    pub asset_quote: AssetId,
    pub chain_id_base: ChainId,
    pub chain_id_quote: ChainId,
    pub amount_base: u128,
    pub price_quote_per_base: u128,
    pub expires_at: Option<u64>,
    pub external_ref: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AcceptDeal {
    pub deal_id: DealId,
    pub amount: Option<u128>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CancelDeal {
    pub deal_id: DealId,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Withdraw {
    pub asset_id: AssetId,
    pub amount: u128,
    pub to: Address,
    pub chain_id: ChainId,
}

/// ZK proof for withdrawal (merkle inclusion proof + nullifier)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WithdrawalProof {
    /// Merkle proof for inclusion in withdrawals_root
    #[serde(with = "serde_bytes")]
    pub merkle_proof: Vec<u8>,
    /// Nullifier to prevent double-spending
    #[serde(with = "serde_bytes")]
    pub nullifier: [u8; 32],
    /// ZK proof (STARK wrapped in SNARK) proving withdrawal validity
    #[serde(with = "serde_bytes")]
    pub zk_proof: Vec<u8>,
}

/// ZK proof for block state transition
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockProof {
    /// Previous state root
    #[serde(with = "serde_bytes")]
    pub prev_state_root: [u8; 32],
    /// New state root after block execution
    #[serde(with = "serde_bytes")]
    pub new_state_root: [u8; 32],
    /// Withdrawals root in this block
    #[serde(with = "serde_bytes")]
    pub withdrawals_root: [u8; 32],
    /// ZK proof (STARK wrapped in SNARK) proving state transition correctness
    #[serde(with = "serde_bytes")]
    pub zk_proof: Vec<u8>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Block {
    pub id: BlockId,
    pub transactions: Vec<Tx>,
    pub timestamp: u64,
    /// Merkle root of state after this block
    #[serde(with = "serde_bytes")]
    pub state_root: [u8; 32],
    /// Merkle root of withdrawals in this block
    #[serde(with = "serde_bytes")]
    pub withdrawals_root: [u8; 32],
    /// ZK proof for block state transition (STARK wrapped in SNARK)
    #[serde(with = "serde_bytes")]
    pub block_proof: Vec<u8>,
}
