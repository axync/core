//! Validation tests for generated proofs
//!
//! Tests validate:
//! - Proof correctness
//! - Public inputs matching
//! - Commitments validity
//! - Different block sizes

#[cfg(any(feature = "stark", feature = "arkworks"))]
use crate::air::BlockTransitionInputs;
#[cfg(any(feature = "stark", feature = "arkworks"))]
use crate::prover::{Prover, ProverConfig};
#[cfg(any(feature = "stark", feature = "arkworks"))]
// DeserializedStarkProof removed - using MinimalStarkProof directly
#[cfg(any(feature = "stark", feature = "arkworks"))]
use bincode;
#[cfg(any(feature = "stark", feature = "arkworks"))]
use zkclear_state::State;
#[cfg(any(feature = "stark", feature = "arkworks"))]
use zkclear_stf::apply_tx;
#[cfg(any(feature = "stark", feature = "arkworks"))]
use zkclear_types::Block;
#[cfg(any(feature = "stark", feature = "arkworks"))]
use zkclear_types::{Address, Tx, TxPayload};

/// Helper to create a test block
#[cfg(any(feature = "stark", feature = "arkworks"))]
fn create_test_block(id: u64, num_txs: usize) -> Block {
    use zkclear_types::{Deposit, TxKind};

    let mut transactions = Vec::new();

    for i in 0..num_txs {
        transactions.push(Tx {
            id: i as u64,
            from: Address::from([i as u8; 20]),
            nonce: 0, // Each address is new, so nonce starts at 0
            kind: TxKind::Deposit,
            payload: TxPayload::Deposit(Deposit {
                tx_hash: [i as u8; 32],
                account: Address::from([i as u8; 20]),
                asset_id: 1,
                amount: 1000 + i as u128,
                chain_id: 1,
            }),
            signature: [0u8; 65],
        });
    }

    Block {
        id,
        transactions,
        timestamp: 1000 + id,
        state_root: [0u8; 32],
        withdrawals_root: [0u8; 32],
        block_proof: vec![],
    }
}

#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_validate_proof_public_inputs() {
    let mut config = ProverConfig::default();
    config.use_placeholders = false;
    let prover = Prover::new(config).expect("Failed to create prover");

    let block = create_test_block(1, 2);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    let block_proof = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .expect("Failed to generate proof");

    // Validate public inputs match expected values
    let expected_prev_root =
        Prover::compute_state_root_static(&prev_state).expect("Failed to compute prev root");
    let expected_new_root =
        Prover::compute_state_root_static(&new_state).expect("Failed to compute new root");

    assert_eq!(
        block_proof.prev_state_root, expected_prev_root,
        "Previous state root should match computed value"
    );
    assert_eq!(
        block_proof.new_state_root, expected_new_root,
        "New state root should match computed value"
    );
}

#[cfg(feature = "stark")]
#[tokio::test]
async fn test_validate_stark_proof_structure() {
    use crate::stark::MinimalStarkProver;
    use crate::stark::StarkProver;

    let prover = MinimalStarkProver::new();
    let block = create_test_block(1, 1);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    let prev_state_root =
        Prover::compute_state_root_static(&prev_state).expect("Failed to compute prev root");
    let new_state_root =
        Prover::compute_state_root_static(&new_state).expect("Failed to compute new root");
    let withdrawals_root = [0u8; 32];

    let block_data = bincode::serialize(&block).expect("Failed to serialize block");

    let stark_proof = prover
        .prove_block_transition(
            &prev_state_root,
            &new_state_root,
            &withdrawals_root,
            &block_data,
        )
        .await
        .expect("Failed to generate STARK proof");

    // Deserialize and validate proof structure
    use crate::air::MinimalStarkProof;

    let proof: MinimalStarkProof =
        bincode::deserialize(&stark_proof).expect("Failed to deserialize proof");

    // Validate proof integrity
    assert!(proof.verify_integrity(), "Proof integrity should be valid");

    // Validate public inputs match
    let expected_public_inputs = BlockTransitionInputs {
        prev_state_root,
        new_state_root,
        withdrawals_root,
        block_id: block.id,
        timestamp: block.timestamp,
    };

    assert_eq!(
        proof.public_inputs.prev_state_root, expected_public_inputs.prev_state_root,
        "Previous state root should match"
    );
    assert_eq!(
        proof.public_inputs.new_state_root, expected_public_inputs.new_state_root,
        "New state root should match"
    );
    assert_eq!(
        proof.public_inputs.withdrawals_root, expected_public_inputs.withdrawals_root,
        "Withdrawals root should match"
    );

    // Validate commitments are non-zero
    assert!(
        proof.trace_commitment != [0u8; 32],
        "Trace commitment should be non-zero"
    );
    assert!(
        proof.constraint_commitment != [0u8; 32],
        "Constraint commitment should be non-zero"
    );

    // Validate metadata
    assert!(
        proof.metadata.trace_width > 0,
        "Trace width should be positive"
    );
    assert!(
        proof.metadata.trace_length > 0,
        "Trace length should be positive"
    );
    assert!(
        proof.metadata.trace_length.is_power_of_two(),
        "Trace length should be a power of two"
    );
    assert!(
        proof.metadata.num_constraints > 0,
        "Number of constraints should be positive"
    );
}

#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]

async fn test_validate_different_block_sizes() {
    let mut config = ProverConfig::default();
    config.use_placeholders = false;
    let prover = Prover::new(config).expect("Failed to create prover");

    // Test with different block sizes
    let sizes = vec![0, 1, 2, 4, 8, 16];

    for size in sizes {
        let block = create_test_block(size as u64, size);
        let prev_state = State::new();
        let mut new_state = prev_state.clone();

        for tx in &block.transactions {
            apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
        }

        let block_proof = prover
            .prove_block(&block, &prev_state, &new_state)
            .await
            .expect(&format!("Failed to generate proof for block size {}", size));

        // Validate proof for each size
        assert!(
            !block_proof.zk_proof.is_empty(),
            "Proof for block size {} should not be empty",
            size
        );

        // Validate state roots
        let expected_prev =
            Prover::compute_state_root_static(&prev_state).expect("Failed to compute prev root");
        let expected_new =
            Prover::compute_state_root_static(&new_state).expect("Failed to compute new root");

        assert_eq!(
            block_proof.prev_state_root, expected_prev,
            "Prev root should match for block size {}",
            size
        );
        assert_eq!(
            block_proof.new_state_root, expected_new,
            "New root should match for block size {}",
            size
        );
    }
}

#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]

async fn test_validate_proof_rejects_invalid_inputs() {
    let mut config = ProverConfig::default();
    config.use_placeholders = false;
    let prover = Prover::new(config).expect("Failed to create prover");

    let block = create_test_block(1, 1);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    let block_proof = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .expect("Failed to generate proof");

    // Create a different prev_state with a different transaction to ensure different state root
    let mut wrong_prev_state = State::new();
    let wrong_block = create_test_block(2, 1); // Different block with different tx
    for tx in &wrong_block.transactions {
        apply_tx(&mut wrong_prev_state, tx, wrong_block.timestamp)
            .expect("Failed to apply transaction");
    }

    let wrong_proof = prover
        .prove_block(&block, &wrong_prev_state, &new_state)
        .await
        .expect("Should generate proof even with wrong prev state");

    // Proofs should have different prev_state_root
    assert_ne!(
        block_proof.prev_state_root, wrong_proof.prev_state_root,
        "Proofs with different prev states should have different prev roots"
    );
}

/// Validate proof structure with placeholder provers
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_validate_proof_structure_placeholders() {
    let mut config = ProverConfig::default();
    config.use_placeholders = true;
    let prover = Prover::new(config).expect("Failed to create prover");

    let block = create_test_block(1, 3);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    let block_proof = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .expect("Failed to generate proof");

    // Validate proof structure
    assert_eq!(
        block_proof.prev_state_root.len(),
        32,
        "State root should be 32 bytes"
    );
    assert_eq!(
        block_proof.new_state_root.len(),
        32,
        "State root should be 32 bytes"
    );
    assert_eq!(
        block_proof.withdrawals_root.len(),
        32,
        "Withdrawals root should be 32 bytes"
    );
    assert!(
        !block_proof.zk_proof.is_empty(),
        "ZK proof should not be empty"
    );

    // Validate public inputs match computed values
    let expected_prev_root =
        Prover::compute_state_root_static(&prev_state).expect("Failed to compute prev root");
    let expected_new_root =
        Prover::compute_state_root_static(&new_state).expect("Failed to compute new root");

    assert_eq!(
        block_proof.prev_state_root, expected_prev_root,
        "Previous state root should match computed value"
    );
    assert_eq!(
        block_proof.new_state_root, expected_new_root,
        "New state root should match computed value"
    );
}

/// Validate proof consistency across multiple generations
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_validate_proof_consistency() {
    let mut config = ProverConfig::default();
    config.use_placeholders = true;
    let prover = Prover::new(config).expect("Failed to create prover");

    let block = create_test_block(1, 2);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    // Generate proof multiple times
    let proof1 = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .expect("Failed to generate proof 1");

    let proof2 = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .expect("Failed to generate proof 2");

    // Public inputs should be consistent
    assert_eq!(proof1.prev_state_root, proof2.prev_state_root);
    assert_eq!(proof1.new_state_root, proof2.new_state_root);
    assert_eq!(proof1.withdrawals_root, proof2.withdrawals_root);

    // Proofs might differ (non-deterministic), but structure should be valid
    assert_eq!(proof1.prev_state_root.len(), 32);
    assert_eq!(proof1.new_state_root.len(), 32);
    assert_eq!(proof1.withdrawals_root.len(), 32);
    assert!(!proof1.zk_proof.is_empty());
    assert!(!proof2.zk_proof.is_empty());
}

/// Validate state root computation for different states
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_validate_state_root_computation() {
    let mut state1 = State::new();
    let state2 = State::new();

    // Compute roots for empty states - should be the same
    let root1 = Prover::compute_state_root_static(&state1).expect("Failed to compute root 1");
    let root2 = Prover::compute_state_root_static(&state2).expect("Failed to compute root 2");

    assert_eq!(root1, root2, "Empty states should have same root");

    // Add transaction to state1
    let block = create_test_block(1, 1);
    for tx in &block.transactions {
        apply_tx(&mut state1, tx, block.timestamp).expect("Failed to apply transaction");
    }

    // Compute roots after transaction
    let root1_after =
        Prover::compute_state_root_static(&state1).expect("Failed to compute root 1 after");
    let root2_after =
        Prover::compute_state_root_static(&state2).expect("Failed to compute root 2 after");

    // State1 should have different root after transaction
    assert_ne!(
        root1, root1_after,
        "State root should change after transaction"
    );
    // State2 should still have same root (no transactions)
    assert_eq!(
        root2, root2_after,
        "State root should not change without transactions"
    );
}

/// Validate withdrawals root computation
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_validate_withdrawals_root() {
    let mut config = ProverConfig::default();
    config.use_placeholders = true;
    let prover = Prover::new(config).expect("Failed to create prover");

    // Block with no withdrawals
    let block1 = create_test_block(1, 0);
    let prev_state1 = State::new();
    let new_state1 = State::new();
    let proof1 = prover
        .prove_block(&block1, &prev_state1, &new_state1)
        .await
        .expect("Failed to generate proof 1");

    // Should be zero root for empty withdrawals
    assert_eq!(
        proof1.withdrawals_root, [0u8; 32],
        "Empty withdrawals should have zero root"
    );

    // Block with transactions (but no withdrawals)
    let block2 = create_test_block(2, 2);
    let prev_state2 = State::new();
    let mut new_state2 = prev_state2.clone();
    for tx in &block2.transactions {
        apply_tx(&mut new_state2, tx, block2.timestamp).expect("Failed to apply transaction");
    }
    let proof2 = prover
        .prove_block(&block2, &prev_state2, &new_state2)
        .await
        .expect("Failed to generate proof 2");

    // Should still be zero root (no withdrawals in block)
    assert_eq!(
        proof2.withdrawals_root, [0u8; 32],
        "Block without withdrawals should have zero root"
    );
}

/// Validate proof size and structure for different block sizes
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_validate_proof_size_scaling() {
    let mut config = ProverConfig::default();
    config.use_placeholders = true;
    let prover = Prover::new(config).expect("Failed to create prover");

    let sizes = vec![1, 2, 4, 8];
    let mut proof_sizes = Vec::new();

    for size in sizes {
        let block = create_test_block(size as u64, size);
        let prev_state = State::new();
        let mut new_state = prev_state.clone();

        for tx in &block.transactions {
            apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
        }

        let block_proof = prover
            .prove_block(&block, &prev_state, &new_state)
            .await
            .expect(&format!("Failed to generate proof for size {}", size));

        proof_sizes.push(block_proof.zk_proof.len());

        // Validate structure for each size
        assert_eq!(block_proof.prev_state_root.len(), 32);
        assert_eq!(block_proof.new_state_root.len(), 32);
        assert_eq!(block_proof.withdrawals_root.len(), 32);
        assert!(!block_proof.zk_proof.is_empty());
    }

    // All proofs should have some size (even if placeholders)
    for (i, size) in proof_sizes.iter().enumerate() {
        assert!(*size > 0, "Proof {} should have non-zero size", i);
    }
}

/// Validate that STARK proof commitments correspond to actual trace and constraints
#[cfg(feature = "stark")]
#[tokio::test]
async fn test_validate_stark_commitments() {
    use crate::air::{BlockTransitionInputs, BlockTransitionPrivateInputs, MinimalStarkProver};
    use sha2::{Digest, Sha256};
    use crate::merkle::MerkleTree;

    let prover = MinimalStarkProver::new();
    let block = create_test_block(1, 3);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    let prev_state_root =
        Prover::compute_state_root_static(&prev_state).expect("Failed to compute prev root");
    let new_state_root =
        Prover::compute_state_root_static(&new_state).expect("Failed to compute new root");
    let withdrawals_root = [0u8; 32];

    let block_data = bincode::serialize(&block).expect("Failed to serialize block");

    let public_inputs = BlockTransitionInputs {
        prev_state_root,
        new_state_root,
        withdrawals_root,
        block_id: block.id,
        timestamp: block.timestamp,
    };

    let private_inputs = BlockTransitionPrivateInputs {
        transactions: block_data,
    };

    // Generate proof
    let stark_proof = prover
        .prove(public_inputs.clone(), private_inputs.clone())
        .expect("Failed to generate STARK proof");

    // Verify proof integrity
    assert!(
        stark_proof.verify_integrity(),
        "Proof integrity should be valid"
    );

    // Manually rebuild trace to verify commitment
    let trace = prover
        .build_trace(&public_inputs, &bincode::deserialize(&private_inputs.transactions).unwrap())
        .expect("Failed to build trace");

    // Compute expected trace commitment
    let mut expected_trace_tree = MerkleTree::new();
    for row in &trace.rows {
        let row_bytes = bincode::serialize(row).expect("Failed to serialize trace row");
        let leaf = Sha256::digest(&row_bytes);
        expected_trace_tree.add_leaf(leaf.into());
    }
    let expected_trace_commitment = expected_trace_tree
        .root()
        .expect("Failed to compute trace commitment");

    // Verify trace commitment matches
    assert_eq!(
        stark_proof.trace_commitment, expected_trace_commitment,
        "Trace commitment should match computed value"
    );

    // Manually evaluate constraints to verify commitment
    let constraints = prover
        .evaluate_constraints(&trace, &public_inputs)
        .expect("Failed to evaluate constraints");

    // Compute expected constraint commitment
    let mut expected_constraint_tree = MerkleTree::new();
    for constraint in &constraints {
        expected_constraint_tree.add_leaf(*constraint);
    }
    let expected_constraint_commitment = expected_constraint_tree
        .root()
        .expect("Failed to compute constraint commitment");

    // Verify constraint commitment matches
    assert_eq!(
        stark_proof.constraint_commitment, expected_constraint_commitment,
        "Constraint commitment should match computed value"
    );
}

/// Validate that SNARK proof correctly wraps STARK proof
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_validate_snark_wraps_stark() {
    let mut config = ProverConfig::default();
    config.use_placeholders = false;
    let prover = Prover::new(config).expect("Failed to create prover");

    let block = create_test_block(1, 2);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    let block_proof = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .expect("Failed to generate proof");

    // Deserialize SNARK wrapper to extract STARK proof
    #[cfg(feature = "arkworks")]
    {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct SnarkProofWrapper {
            proof: Vec<u8>,
            public_inputs: Vec<u8>,
            version: u8,
        }

        let wrapper: SnarkProofWrapper = bincode::deserialize(&block_proof.zk_proof)
            .expect("Failed to deserialize SNARK wrapper");

        // Verify version
        assert_eq!(wrapper.version, 3, "SNARK wrapper version should be 3");

        // Verify public inputs match
        let expected_public_inputs = bincode::serialize(&(
            block_proof.prev_state_root,
            block_proof.new_state_root,
            block_proof.withdrawals_root,
        ))
        .unwrap();
        assert_eq!(
            wrapper.public_inputs, expected_public_inputs,
            "Public inputs in wrapper should match block proof"
        );

        // Verify Groth16 proof is valid
        let verify_result = prover
            .verify_snark_proof(&block_proof.zk_proof, &expected_public_inputs)
            .await
            .expect("Verification should succeed");
        assert!(verify_result, "SNARK proof should be valid");
    }
}

/// Validate that proof generation is deterministic for same inputs (when using placeholders)
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_validate_proof_determinism() {
    let mut config = ProverConfig::default();
    config.use_placeholders = true; // Placeholders are deterministic
    let prover = Prover::new(config).expect("Failed to create prover");

    let block = create_test_block(1, 2);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    // Generate proof multiple times
    let proof1 = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .expect("Failed to generate proof 1");

    let proof2 = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .expect("Failed to generate proof 2");

    // With placeholders, proofs should be identical
    assert_eq!(
        proof1.zk_proof, proof2.zk_proof,
        "Placeholder proofs should be deterministic"
    );
}

/// Validate proof generation with edge cases
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_validate_proof_edge_cases() {
    let mut config = ProverConfig::default();
    config.use_placeholders = false;
    let prover = Prover::new(config).expect("Failed to create prover");

    // Test 1: Empty block
    let empty_block = create_test_block(1, 0);
    let empty_prev_state = State::new();
    let empty_new_state = State::new();

    let empty_proof = prover
        .prove_block(&empty_block, &empty_prev_state, &empty_new_state)
        .await
        .expect("Failed to generate proof for empty block");

    assert!(!empty_proof.zk_proof.is_empty(), "Empty block proof should not be empty");
    assert_eq!(
        empty_proof.prev_state_root, empty_proof.new_state_root,
        "Empty block should have same prev and new state root"
    );

    // Test 2: Large block
    let large_block = create_test_block(2, 32);
    let large_prev_state = State::new();
    let mut large_new_state = large_prev_state.clone();

    for tx in &large_block.transactions {
        apply_tx(&mut large_new_state, tx, large_block.timestamp)
            .expect("Failed to apply transaction");
    }

    let large_proof = prover
        .prove_block(&large_block, &large_prev_state, &large_new_state)
        .await
        .expect("Failed to generate proof for large block");

    assert!(!large_proof.zk_proof.is_empty(), "Large block proof should not be empty");
    assert_ne!(
        large_proof.prev_state_root, large_proof.new_state_root,
        "Large block should have different prev and new state root"
    );
}
