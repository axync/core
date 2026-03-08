use crate::error::ProverError;

/// STARK proof generator trait
///
/// This trait allows for different STARK implementations (minimal STARK prover, etc.)
#[async_trait::async_trait]
pub trait StarkProver: Send + Sync {
    /// Generate a STARK proof for a block state transition
    async fn prove_block_transition(
        &self,
        prev_state_root: &[u8; 32],
        new_state_root: &[u8; 32],
        withdrawals_root: &[u8; 32],
        block_data: &[u8],
    ) -> Result<Vec<u8>, ProverError>;

    /// Verify a STARK proof
    async fn verify_stark_proof(
        &self,
        proof: &[u8],
        public_inputs: &[u8],
    ) -> Result<bool, ProverError>;
}

/// Placeholder STARK prover implementation
///
/// This is a placeholder implementation used when:
/// - `use_placeholders=true` in ProverConfig (for testing)
///
/// In production, use `MinimalStarkProver` by setting `use_placeholders=false`.
pub struct PlaceholderStarkProver;

#[async_trait::async_trait]
impl StarkProver for PlaceholderStarkProver {
    async fn prove_block_transition(
        &self,
        _prev_state_root: &[u8; 32],
        _new_state_root: &[u8; 32],
        _withdrawals_root: &[u8; 32],
        _block_data: &[u8],
    ) -> Result<Vec<u8>, ProverError> {
        // Placeholder implementation: returns a dummy proof immediately
        // This is intentional for testing/development when real proof generation is not needed
        Ok(b"STARK_PROOF_PLACEHOLDER".to_vec())
    }

    async fn verify_stark_proof(
        &self,
        _proof: &[u8],
        _public_inputs: &[u8],
    ) -> Result<bool, ProverError> {
        // Placeholder implementation: always returns true
        // This is intentional for testing/development when real proof verification is not needed
        Ok(true)
    }
}

/// Minimal STARK prover
///
/// This uses a custom minimal STARK prover implementation that doesn't require
/// external dependencies. It generates proofs using standard cryptographic primitives.
#[cfg(feature = "stark")]
pub struct MinimalStarkProver {
    prover: crate::air::MinimalStarkProver,
    verifier: crate::air::MinimalStarkVerifier,
}

#[cfg(feature = "stark")]
impl MinimalStarkProver {
    pub fn new() -> Self {
        Self {
            prover: crate::air::MinimalStarkProver::new(),
            verifier: crate::air::MinimalStarkVerifier::new(),
        }
    }
}

#[cfg(feature = "stark")]
#[async_trait::async_trait]
impl StarkProver for MinimalStarkProver {
    async fn prove_block_transition(
        &self,
        prev_state_root: &[u8; 32],
        new_state_root: &[u8; 32],
        withdrawals_root: &[u8; 32],
        block_data: &[u8],
    ) -> Result<Vec<u8>, ProverError> {
        use crate::air::{BlockTransitionInputs, BlockTransitionPrivateInputs};
        use zkclear_types::Block;

        // Deserialize block to extract metadata
        let block: Block = bincode::deserialize(block_data).map_err(|e| {
            ProverError::Serialization(format!("Failed to deserialize block: {}", e))
        })?;

        // Create public inputs
        let public_inputs = BlockTransitionInputs {
            prev_state_root: *prev_state_root,
            new_state_root: *new_state_root,
            withdrawals_root: *withdrawals_root,
            block_id: block.id,
            timestamp: block.timestamp,
        };

        // Create private inputs
        let private_inputs = BlockTransitionPrivateInputs {
            transactions: block_data.to_vec(),
        };

        // Generate proof using minimal STARK prover
        let proof = self.prover.prove(public_inputs, private_inputs)?;

        // Serialize proof
        let serialized = bincode::serialize(&proof)
            .map_err(|e| ProverError::Serialization(format!("Failed to serialize proof: {}", e)))?;

        Ok(serialized)
    }

    async fn verify_stark_proof(
        &self,
        proof: &[u8],
        public_inputs: &[u8],
    ) -> Result<bool, ProverError> {
        use crate::air::BlockTransitionInputs;

        // Deserialize proof
        let proof: crate::air::MinimalStarkProof = bincode::deserialize(proof).map_err(|e| {
            ProverError::Serialization(format!("Failed to deserialize proof: {}", e))
        })?;

        // Deserialize public inputs if provided
        if !public_inputs.is_empty() {
            let expected_public_inputs: BlockTransitionInputs = bincode::deserialize(public_inputs)
                .map_err(|e| {
                    ProverError::Serialization(format!(
                        "Failed to deserialize public inputs: {}",
                        e
                    ))
                })?;

            // Verify with public inputs check
            self.verifier
                .verify_with_public_inputs(&proof, &expected_public_inputs)
        } else {
            // Basic verification without public inputs check
            self.verifier.verify(&proof)
        }
    }
}
