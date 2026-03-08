use sha3::{Digest, Keccak256};
use zkclear_types::{Address, AssetId, ChainId};

/// Generate a nullifier for a withdrawal to prevent double-spending
///
/// The nullifier is computed as: keccak256(user || asset_id || amount || chain_id || secret)
/// where secret is a user-specific secret that should be kept private
pub fn generate_nullifier(
    user: Address,
    asset_id: AssetId,
    amount: u128,
    chain_id: ChainId,
    secret: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(&user);
    hasher.update(&asset_id.to_le_bytes());
    hasher.update(&amount.to_le_bytes());
    hasher.update(&chain_id.to_le_bytes());
    hasher.update(secret);
    hasher.finalize().into()
}

/// Generate a nullifier from withdrawal data and a secret
pub fn generate_nullifier_from_withdrawal(
    user: Address,
    asset_id: AssetId,
    amount: u128,
    chain_id: ChainId,
    secret: &[u8; 32],
) -> [u8; 32] {
    generate_nullifier(user, asset_id, amount, chain_id, secret)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nullifier_generation() {
        let user = [1u8; 20];
        let asset_id = 1;
        let amount = 1000u128;
        let chain_id = 1u64;
        let secret = [42u8; 32];

        let nullifier1 = generate_nullifier(user, asset_id, amount, chain_id, &secret);
        let nullifier2 = generate_nullifier(user, asset_id, amount, chain_id, &secret);

        // Same inputs should produce same nullifier
        assert_eq!(nullifier1, nullifier2);

        // Different secret should produce different nullifier
        let secret2 = [43u8; 32];
        let nullifier3 = generate_nullifier(user, asset_id, amount, chain_id, &secret2);
        assert_ne!(nullifier1, nullifier3);
    }
}
