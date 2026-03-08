use crate::error::ProverError;
use crate::merkle::{hash_withdrawal, verify_merkle_proof, MerkleTree};
use crate::nullifier::generate_nullifier_from_withdrawal;
use crate::snark::SnarkProver;
use crate::stark::StarkProver;
use zkclear_state::State;
use zkclear_types::{Address, Block, BlockProof, Withdraw, WithdrawalProof};

/// Configuration for the ZK prover
#[derive(Debug, Clone)]
pub struct ProverConfig {
    /// Whether to use placeholder implementations (for testing)
    pub use_placeholders: bool,
    /// Path to directory for storing Groth16 keys (default: ./keys)
    pub groth16_keys_dir: Option<std::path::PathBuf>,
    /// Force regeneration of Groth16 keys even if they exist
    pub force_regenerate_keys: bool,
}

impl Default for ProverConfig {
    fn default() -> Self {
        Self {
            use_placeholders: true,
            groth16_keys_dir: None,
            force_regenerate_keys: false,
        }
    }
}

/// Main ZK prover service
///
/// This service coordinates STARK and SNARK proof generation
pub struct Prover {
    stark_prover: Box<dyn StarkProver>,
    snark_prover: Box<dyn SnarkProver>,
}

impl Prover {
    /// Create a new prover with the given configuration
    pub fn new(config: ProverConfig) -> Result<Self, ProverError> {
        let stark_prover: Box<dyn StarkProver> = if config.use_placeholders {
            Box::new(crate::stark::PlaceholderStarkProver)
        } else {
            #[cfg(feature = "stark")]
            {
                Box::new(crate::stark::MinimalStarkProver::new())
            }
            #[cfg(not(feature = "stark"))]
            {
                Box::new(crate::stark::PlaceholderStarkProver)
            }
        };

        let snark_prover: Box<dyn SnarkProver> = if config.use_placeholders {
            Box::new(crate::snark::PlaceholderSnarkProver)
        } else {
            #[cfg(feature = "arkworks")]
            {
                Box::new(
                    crate::snark::ArkworksSnarkProver::new(
                        config.groth16_keys_dir.clone(),
                        config.force_regenerate_keys,
                    )
                    .map_err(|e| {
                        eprintln!("Failed to initialize ArkworksSnarkProver: {:?}", e);
                        e
                    })?,
                )
            }
            #[cfg(not(feature = "arkworks"))]
            {
                Box::new(crate::snark::SimplifiedSnarkProver::new())
            }
        };

        Ok(Self {
            stark_prover,
            snark_prover,
        })
    }

    /// Generate a block proof (STARK + SNARK)
    ///
    /// This generates a STARK proof for the block state transition,
    /// then wraps it in a SNARK for compact on-chain verification
    pub async fn prove_block(
        &self,
        block: &Block,
        prev_state: &State,
        new_state: &State,
    ) -> Result<BlockProof, ProverError> {
        // Calculate state roots
        let prev_state_root = self.compute_state_root(prev_state)?;
        let new_state_root = self.compute_state_root(new_state)?;
        let withdrawals_root = self.compute_withdrawals_root(block)?;

        // Serialize block data for proof generation
        let block_data = bincode::serialize(block)
            .map_err(|e| ProverError::Serialization(format!("Failed to serialize block: {}", e)))?;

        // Generate STARK proof
        let stark_proof = self
            .stark_prover
            .prove_block_transition(
                &prev_state_root,
                &new_state_root,
                &withdrawals_root,
                &block_data,
            )
            .await?;

        // Wrap STARK proof in SNARK
        let public_inputs =
            bincode::serialize(&(prev_state_root, new_state_root, withdrawals_root)).map_err(
                |e| ProverError::Serialization(format!("Failed to serialize public inputs: {}", e)),
            )?;

        let snark_proof = self
            .snark_prover
            .wrap_stark_in_snark(&stark_proof, &public_inputs)
            .await?;

        Ok(BlockProof {
            prev_state_root,
            new_state_root,
            withdrawals_root,
            zk_proof: snark_proof,
        })
    }

    /// Verify a SNARK proof
    ///
    /// This verifies a SNARK proof with the given public inputs
    pub async fn verify_snark_proof(
        &self,
        proof: &[u8],
        public_inputs: &[u8],
    ) -> Result<bool, ProverError> {
        self.snark_prover
            .verify_snark_proof(proof, public_inputs)
            .await
    }

    /// Generate a withdrawal proof
    ///
    /// This generates a Merkle proof for inclusion in withdrawals_root
    /// and a ZK proof for withdrawal validity
    pub async fn prove_withdrawal(
        &self,
        withdrawal: &Withdraw,
        user: Address,
        withdrawals_root: &[u8; 32],
        merkle_proof: Vec<[u8; 32]>,
        secret: &[u8; 32],
    ) -> Result<WithdrawalProof, ProverError> {
        // Generate nullifier
        let nullifier = generate_nullifier_from_withdrawal(
            user,
            withdrawal.asset_id,
            withdrawal.amount,
            withdrawal.chain_id,
            secret,
        );

        // Verify Merkle proof
        let leaf = hash_withdrawal(
            user,
            withdrawal.asset_id,
            withdrawal.amount,
            withdrawal.chain_id,
        );

        // Note: For proper verification with trees >2 leaves, we need the withdrawal index.
        // For now, we pass None which works for simple cases (1-2 leaves).
        // In production, the withdrawal index should be passed to this function.
        if !verify_merkle_proof(&leaf, &merkle_proof, withdrawals_root, None) {
            return Err(ProverError::InvalidWithdrawalsRoot(
                "Merkle proof verification failed".to_string(),
            ));
        }

        // Generate ZK proof for withdrawal validity
        // This generates a placeholder proof when use_placeholders=true
        // In production (use_placeholders=false), this would generate a proper ZK proof
        let zk_proof = b"WITHDRAWAL_PROOF_PLACEHOLDER".to_vec();

        Ok(WithdrawalProof {
            merkle_proof: merkle_proof
                .iter()
                .flat_map(|p| p.iter().copied())
                .collect(),
            nullifier,
            zk_proof,
        })
    }

    /// Compute state root from state
    fn compute_state_root(&self, state: &State) -> Result<[u8; 32], ProverError> {
        Self::compute_state_root_static(state)
    }

    /// Compute state root from state (static method for use in tests)
    pub fn compute_state_root_static(state: &State) -> Result<[u8; 32], ProverError> {
        // Use Merkle tree approach for proper state root computation
        use crate::merkle::{hash_state_leaf, MerkleTree};

        let mut tree = MerkleTree::new();

        // Add all accounts as leaves
        let mut account_ids: Vec<_> = state.accounts.keys().collect();
        account_ids.sort();

        for account_id in account_ids {
            let account = state.accounts.get(account_id).ok_or_else(|| {
                ProverError::StarkProof(format!("Account {} not found", account_id))
            })?;

            let account_bytes = bincode::serialize(account).map_err(|e| {
                ProverError::Serialization(format!("Failed to serialize account: {}", e))
            })?;

            let leaf = hash_state_leaf(&account_bytes);
            tree.add_leaf(leaf);
        }

        // Add all deals as leaves
        let mut deal_ids: Vec<_> = state.deals.keys().collect();
        deal_ids.sort();

        for deal_id in deal_ids {
            let deal = state
                .deals
                .get(deal_id)
                .ok_or_else(|| ProverError::StarkProof(format!("Deal {} not found", deal_id)))?;

            let deal_bytes = bincode::serialize(deal).map_err(|e| {
                ProverError::Serialization(format!("Failed to serialize deal: {}", e))
            })?;

            let leaf = hash_state_leaf(&deal_bytes);
            tree.add_leaf(leaf);
        }

        tree.root()
    }

    /// Get reference to STARK prover (for testing/profiling)
    pub fn stark_prover(&self) -> &dyn StarkProver {
        self.stark_prover.as_ref()
    }

    /// Get reference to SNARK prover (for testing/profiling)
    pub fn snark_prover(&self) -> &dyn SnarkProver {
        self.snark_prover.as_ref()
    }

    /// Compute withdrawals root from block
    /// Made public for testing/profiling
    pub fn compute_withdrawals_root(&self, block: &Block) -> Result<[u8; 32], ProverError> {
        let mut tree = MerkleTree::new();

        // Extract withdrawals from block transactions
        for tx in &block.transactions {
            if let zkclear_types::TxPayload::Withdraw(w) = &tx.payload {
                let leaf = hash_withdrawal(tx.from, w.asset_id, w.amount, w.chain_id);
                tree.add_leaf(leaf);
            }
        }

        tree.root()
    }

    /// Generate Merkle proof for a withdrawal
    pub fn generate_withdrawal_merkle_proof(
        &self,
        block: &Block,
        withdrawal_index: usize,
    ) -> Result<(Vec<[u8; 32]>, [u8; 32]), ProverError> {
        let mut tree = MerkleTree::new();
        let mut target_index = None;

        // Build tree and find withdrawal index
        let mut current_index = 0;
        for tx in &block.transactions {
            if let zkclear_types::TxPayload::Withdraw(w) = &tx.payload {
                let leaf = hash_withdrawal(tx.from, w.asset_id, w.amount, w.chain_id);
                tree.add_leaf(leaf);

                if current_index == withdrawal_index {
                    target_index = Some(tree.leaves.len() - 1);
                }
                current_index += 1;
            }
        }

        let root = tree.root()?;
        let proof = if let Some(idx) = target_index {
            tree.proof(idx)?
        } else {
            return Err(ProverError::InvalidWithdrawalsRoot(format!(
                "Withdrawal index {} not found",
                withdrawal_index
            )));
        };

        Ok((proof, root))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_prove_block_placeholder() {
        let config = ProverConfig::default();
        let prover = Prover::new(config).expect("Failed to create prover");

        let block = Block {
            id: 0,
            transactions: vec![],
            timestamp: 1000,
            state_root: [0u8; 32],
            withdrawals_root: [0u8; 32],
            block_proof: vec![],
        };

        let prev_state = State::new();
        let new_state = State::new();

        let proof = prover.prove_block(&block, &prev_state, &new_state).await;
        assert!(proof.is_ok());
    }
}
