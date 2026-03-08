pub mod chain_ids {
    pub const ETHEREUM: u64 = 1;
    pub const POLYGON: u64 = 137;
    pub const MANTLE: u64 = 5000;
    pub const ARBITRUM: u64 = 42161;
    pub const OPTIMISM: u64 = 10;
    pub const BASE: u64 = 8453;
}

pub mod address {
    pub const ADDRESS_SIZE: usize = 20;
    pub const ZERO_ADDRESS_BYTES: [u8; ADDRESS_SIZE] = [0u8; ADDRESS_SIZE];
}

pub mod signature {
    pub const SIGNATURE_SIZE: usize = 65;
    pub const R_SIZE: usize = 32;
    pub const S_SIZE: usize = 32;
    pub const V_SIZE: usize = 1;
}

pub mod transaction {
    pub const TX_HASH_SIZE: usize = 32;
}

pub mod limits {
    pub const MAX_ASSET_ID: u16 = u16::MAX;
    pub const MAX_ACCOUNT_ID: u64 = u64::MAX;
    pub const MAX_DEAL_ID: u64 = u64::MAX;
    pub const MAX_BLOCK_ID: u64 = u64::MAX;
    pub const MAX_CHAIN_ID: u64 = u64::MAX;
}

pub mod deal {
    pub const MAX_DEAL_DURATION_SECONDS: u64 = 7 * 24 * 60 * 60; // 1 week
}

pub mod defaults {
    pub use super::chain_ids;

    pub const DEFAULT_CHAIN_ID: u64 = chain_ids::ETHEREUM;
}
