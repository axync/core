use crate::error::ProverError;
use sha3::{Digest, Keccak256};
use axync_types::{Address, AssetId, ChainId};

/// Merkle tree compatible with Solidity's sorted-pair keccak256 verification.
///
/// On-chain verification uses:
///   if (a <= b) { hash(a, b) } else { hash(b, a) }
/// so pair ordering is deterministic by value, not position.
pub struct MerkleTree {
    pub(crate) leaves: Vec<[u8; 32]>,
}

impl MerkleTree {
    pub fn new() -> Self {
        Self { leaves: Vec::new() }
    }

    pub fn add_leaf(&mut self, leaf: [u8; 32]) {
        self.leaves.push(leaf);
    }

    /// Number of leaves in the tree
    pub fn len(&self) -> usize {
        self.leaves.len()
    }

    /// Compute the Merkle root using sorted-pair keccak256 (Solidity-compatible)
    pub fn root(&self) -> Result<[u8; 32], ProverError> {
        if self.leaves.is_empty() {
            return Ok([0u8; 32]);
        }

        if self.leaves.len() == 1 {
            return Ok(self.leaves[0]);
        }

        let mut current_level = self.leaves.clone();

        // Pad to even number if needed
        if current_level.len() % 2 != 0 {
            let last = *current_level.last().unwrap();
            current_level.push(last);
        }

        while current_level.len() > 1 {
            let mut next_level = Vec::with_capacity((current_level.len() + 1) / 2);

            for i in (0..current_level.len()).step_by(2) {
                if i + 1 < current_level.len() {
                    next_level.push(hash_pair_sorted(&current_level[i], &current_level[i + 1]));
                } else {
                    next_level.push(hash_pair_sorted(&current_level[i], &current_level[i]));
                }
            }

            current_level = next_level;
        }

        Ok(current_level[0])
    }

    /// Generate a Merkle proof for a leaf at the given index.
    /// Returns sibling hashes from bottom to top (matches Solidity verification order).
    pub fn proof(&self, leaf_index: usize) -> Result<Vec<[u8; 32]>, ProverError> {
        if leaf_index >= self.leaves.len() {
            return Err(ProverError::MerkleTree(format!(
                "Leaf index {} out of bounds (tree has {} leaves)",
                leaf_index, self.leaves.len()
            )));
        }

        if self.leaves.len() == 1 {
            return Ok(Vec::new());
        }

        let mut proof = Vec::new();
        let mut current_level = self.leaves.clone();

        // Pad to even
        if current_level.len() % 2 != 0 {
            let last = *current_level.last().unwrap();
            current_level.push(last);
        }

        let mut current_index = leaf_index;

        while current_level.len() > 1 {
            let sibling_index = if current_index % 2 == 0 {
                current_index + 1
            } else {
                current_index - 1
            };

            if sibling_index < current_level.len() {
                proof.push(current_level[sibling_index]);
            } else {
                proof.push(current_level[current_index]);
            }

            // Build next level with sorted pairs
            let mut next_level = Vec::new();
            for i in (0..current_level.len()).step_by(2) {
                if i + 1 < current_level.len() {
                    next_level.push(hash_pair_sorted(&current_level[i], &current_level[i + 1]));
                } else {
                    next_level.push(hash_pair_sorted(&current_level[i], &current_level[i]));
                }
            }

            current_index /= 2;
            current_level = next_level;
        }

        Ok(proof)
    }
}

/// Hash two nodes with sorted order (smaller first), using keccak256.
/// Matches Solidity: `keccak256(abi.encodePacked(a <= b ? a : b, a <= b ? b : a))`
fn hash_pair_sorted(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    if a <= b {
        hasher.update(a);
        hasher.update(b);
    } else {
        hasher.update(b);
        hasher.update(a);
    }
    hasher.finalize().into()
}

/// Hash a withdrawal leaf — matches Solidity:
/// `keccak256(abi.encodePacked(user, assetId, amount, chainId))`
/// where assetId/amount/chainId are uint256 (32 bytes big-endian)
pub fn hash_withdrawal(
    user: Address,
    asset_id: AssetId,
    amount: u128,
    chain_id: ChainId,
) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    // address = 20 bytes (abi.encodePacked)
    hasher.update(&user);
    // uint256 = 32 bytes big-endian
    hasher.update(&u256_bytes(asset_id as u128));
    hasher.update(&u256_bytes(amount));
    hasher.update(&u256_bytes(chain_id as u128));
    hasher.finalize().into()
}

/// Hash an ERC-721 release leaf — matches Solidity:
/// `keccak256(abi.encodePacked(tokenContract, tokenId, buyer, chainId, listingId))`
pub fn hash_nft_release(
    nft_contract: Address,
    token_id: u64,
    buyer: Address,
    nft_chain_id: ChainId,
    listing_id: u64,
) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(&nft_contract);
    hasher.update(&u256_bytes(token_id as u128));
    hasher.update(&buyer);
    hasher.update(&u256_bytes(nft_chain_id as u128));
    hasher.update(&u256_bytes(listing_id as u128));
    hasher.finalize().into()
}

/// Hash an ERC-20 release leaf — matches Solidity:
/// `keccak256(abi.encodePacked(tokenContract, amount, buyer, chainId, listingId))`
pub fn hash_token_release(
    token_contract: Address,
    amount: u128,
    buyer: Address,
    chain_id: ChainId,
    listing_id: u64,
) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(&token_contract);
    hasher.update(&u256_bytes(amount));
    hasher.update(&buyer);
    hasher.update(&u256_bytes(chain_id as u128));
    hasher.update(&u256_bytes(listing_id as u128));
    hasher.finalize().into()
}

/// Convert a u128 value to 32-byte big-endian (EVM uint256 format)
fn u256_bytes(val: u128) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    bytes[16..32].copy_from_slice(&val.to_be_bytes());
    bytes
}

/// Hash state data to create a leaf for state root
pub fn hash_state_leaf(data: &[u8]) -> [u8; 32] {
    let mut hasher = Keccak256::new();
    hasher.update(data);
    hasher.finalize().into()
}

/// Verify a Merkle proof (sorted-pair keccak256, Solidity-compatible)
pub fn verify_merkle_proof(
    leaf: &[u8; 32],
    proof: &[[u8; 32]],
    root: &[u8; 32],
) -> bool {
    if proof.is_empty() {
        return leaf == root;
    }

    let mut current = *leaf;

    for sibling in proof {
        current = hash_pair_sorted(&current, sibling);
    }

    current == *root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merkle_tree_single_leaf() {
        let mut tree = MerkleTree::new();
        let leaf = [1u8; 32];
        tree.add_leaf(leaf);
        let root = tree.root().unwrap();
        assert_eq!(root, leaf);
    }

    #[test]
    fn test_merkle_tree_two_leaves() {
        let mut tree = MerkleTree::new();
        let leaf1 = [1u8; 32];
        let leaf2 = [2u8; 32];
        tree.add_leaf(leaf1);
        tree.add_leaf(leaf2);
        let root = tree.root().unwrap();

        let proof = tree.proof(0).unwrap();
        assert!(verify_merkle_proof(&leaf1, &proof, &root));

        let proof = tree.proof(1).unwrap();
        assert!(verify_merkle_proof(&leaf2, &proof, &root));
    }

    #[test]
    fn test_merkle_tree_four_leaves() {
        let mut tree = MerkleTree::new();
        for i in 0..4u8 {
            tree.add_leaf([i; 32]);
        }
        let root = tree.root().unwrap();

        for i in 0..4 {
            let leaf = [i as u8; 32];
            let proof = tree.proof(i).unwrap();
            assert!(
                verify_merkle_proof(&leaf, &proof, &root),
                "Failed to verify proof for leaf {}",
                i
            );
        }
    }

    #[test]
    fn test_merkle_tree_eight_leaves() {
        let mut tree = MerkleTree::new();
        for i in 0..8u8 {
            tree.add_leaf([i; 32]);
        }
        let root = tree.root().unwrap();

        for i in 0..8 {
            let leaf = [i as u8; 32];
            let proof = tree.proof(i).unwrap();
            assert!(
                verify_merkle_proof(&leaf, &proof, &root),
                "Failed to verify proof for leaf {}",
                i
            );
        }
    }

    #[test]
    fn test_sorted_pair_ordering() {
        let a = [1u8; 32];
        let b = [2u8; 32];
        // hash(a, b) should equal hash(a, b) regardless of call order
        // because sorted pairs always put smaller first
        assert_eq!(hash_pair_sorted(&a, &b), hash_pair_sorted(&b, &a));
    }
}
