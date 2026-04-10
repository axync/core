mod constants;

pub use constants::*;

pub type AccountId = u64;
pub type DealId = u64;
pub type NftListingId = u64;
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
    EthereumSepolia = constants::chain_ids::ETHEREUM_SEPOLIA,
    BaseSepolia = constants::chain_ids::BASE_SEPOLIA,
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
            SupportedChain::EthereumSepolia => constants::chain_ids::ETHEREUM_SEPOLIA,
            SupportedChain::BaseSepolia => constants::chain_ids::BASE_SEPOLIA,
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
            constants::chain_ids::ETHEREUM_SEPOLIA => Some(SupportedChain::EthereumSepolia),
            constants::chain_ids::BASE_SEPOLIA => Some(SupportedChain::BaseSepolia),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AssetType {
    ERC721,
    ERC20,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum NftListingStatus {
    Active,
    /// Linked to a Deal — cannot be independently bought or cancelled
    Reserved,
    Sold,
    Cancelled,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NftListing {
    pub id: NftListingId,
    #[serde(with = "serde_bytes")]
    pub seller: Address,
    pub asset_type: AssetType,
    /// Token contract address (ERC-721 or ERC-20)
    #[serde(with = "serde_bytes")]
    pub nft_contract: Address,
    /// ERC-721 token ID (0 for ERC-20)
    pub token_id: u64,
    /// ERC-20 amount (0 for ERC-721)
    #[serde(default)]
    pub amount: u128,
    /// Chain where the asset lives (and is escrowed)
    pub nft_chain_id: ChainId,
    /// Price in wei (ETH)
    pub price: u128,
    /// Chain where buyer pays (via AxyncVault deposit)
    pub payment_chain_id: ChainId,
    pub status: NftListingStatus,
    #[serde(with = "serde_bytes")]
    pub buyer: Address,
    pub created_at: u64,
    /// Listing ID in the on-chain AxyncEscrow contract
    pub on_chain_listing_id: u64,
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

/// One side of a trade — either a fungible balance in Vault or an asset escrowed on-chain.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TradeAsset {
    /// Fungible asset (ETH/ERC20) tracked in off-chain Vault balances.
    Fungible {
        asset_id: AssetId,
        amount: u128,
        chain_id: ChainId,
    },
    /// Asset locked in on-chain AxyncEscrow contract.
    /// References an existing NftListing created by the watcher.
    Escrowed {
        escrow_listing_id: NftListingId,
    },
}

impl TradeAsset {
    pub fn chain_id(&self, get_listing_chain: impl Fn(NftListingId) -> Option<ChainId>) -> Option<ChainId> {
        match self {
            TradeAsset::Fungible { chain_id, .. } => Some(*chain_id),
            TradeAsset::Escrowed { escrow_listing_id } => get_listing_chain(*escrow_listing_id),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Deal {
    pub id: DealId,
    #[serde(with = "serde_bytes")]
    pub maker: Address,
    pub taker: Option<Address>,
    pub visibility: DealVisibility,
    /// What the maker is selling (locked at deal creation)
    pub offer: TradeAsset,
    /// What the maker wants in return
    pub consideration: TradeAsset,
    /// For Fungible offers: how much has been filled (partial fills)
    pub amount_filled: u128,
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
    ListNft,
    BuyNft,
    CancelNftListing,
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
    ListNft(ListNft),
    BuyNft(BuyNft),
    CancelNftListing(CancelNftListing),
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
    /// What the maker is selling
    pub offer: TradeAsset,
    /// What the maker wants in return
    pub consideration: TradeAsset,
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

/// Created by watcher when NftListed/TokenListed event is detected on-chain
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ListNft {
    #[serde(with = "serde_bytes")]
    pub seller: Address,
    pub asset_type: AssetType,
    #[serde(with = "serde_bytes")]
    pub nft_contract: Address,
    pub token_id: u64,
    /// ERC-20 amount (0 for ERC-721)
    #[serde(default)]
    pub amount: u128,
    pub nft_chain_id: ChainId,
    pub price: u128,
    pub payment_chain_id: ChainId,
    pub on_chain_listing_id: u64,
}

/// Submitted by buyer (user-signed)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuyNft {
    pub listing_id: NftListingId,
}

/// Created by watcher when NftCancelled event is detected on-chain
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CancelNftListing {
    pub listing_id: NftListingId,
    pub on_chain_listing_id: u64,
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
