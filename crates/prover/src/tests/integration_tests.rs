//! End-to-end integration tests for ZK proof flow
//!
//! Tests the complete flow: block creation → proof generation → verification

#[cfg(any(feature = "stark", feature = "arkworks"))]
use crate::prover::{Prover, ProverConfig};
#[cfg(any(feature = "stark", feature = "arkworks"))]
use bincode;
#[cfg(any(feature = "stark", feature = "arkworks"))]
use zkclear_state::State;
#[cfg(any(feature = "stark", feature = "arkworks"))]
use zkclear_stf::apply_tx;
#[cfg(any(feature = "stark", feature = "arkworks"))]
use zkclear_types::Block;
#[cfg(any(feature = "stark", feature = "arkworks"))]
use zkclear_types::{Address, BlockProof, Tx, TxPayload};

/// Helper to create a test block
#[cfg(any(feature = "stark", feature = "arkworks"))]
fn create_test_block(id: u64, num_txs: usize) -> Block {
    create_test_block_with_offset(id, num_txs, 0)
}

/// Helper to create a test block with address offset
#[cfg(any(feature = "stark", feature = "arkworks"))]
fn create_test_block_with_offset(id: u64, num_txs: usize, address_offset: usize) -> Block {
    use zkclear_types::{Deposit, TxKind};

    let mut transactions = Vec::new();

    for i in 0..num_txs {
        // Each transaction uses a different address, so nonce should be 0 for each
        let addr_byte = (address_offset + i) as u8;
        transactions.push(Tx {
            id: i as u64,
            from: Address::from([addr_byte; 20]),
            nonce: 0, // Each address is new, so nonce starts at 0
            kind: TxKind::Deposit,
            payload: TxPayload::Deposit(Deposit {
                tx_hash: [i as u8; 32],
                account: Address::from([addr_byte; 20]),
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

async fn test_e2e_block_creation_to_proof_generation() {
    // Create prover
    let mut config = ProverConfig::default();
    config.use_placeholders = false; // Use real provers
    let prover = Prover::new(config).expect("Failed to create prover");

    // Create block with transactions
    let block = create_test_block(1, 3);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    // Apply transactions
    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    // Generate proof
    let result = prover.prove_block(&block, &prev_state, &new_state).await;
    assert!(result.is_ok(), "Proof generation should succeed");

    let block_proof = result.unwrap();

    // Validate proof structure
    assert_ne!(
        block_proof.prev_state_root, block_proof.new_state_root,
        "State roots should be different after transactions"
    );
    assert!(
        !block_proof.zk_proof.is_empty(),
        "ZK proof should not be empty"
    );

    // Verify proof
    #[cfg(feature = "arkworks")]
    {
        let verify_result = prover
            .verify_snark_proof(
                &block_proof.zk_proof,
                &bincode::serialize(&(
                    block_proof.prev_state_root,
                    block_proof.new_state_root,
                    block_proof.withdrawals_root,
                ))
                .unwrap(),
            )
            .await;

        assert!(
            verify_result.is_ok(),
            "SNARK proof verification should succeed"
        );
        assert!(verify_result.unwrap(), "SNARK proof should be valid");
    }
}

#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]

async fn test_e2e_multiple_blocks_sequential() {
    let mut config = ProverConfig::default();
    config.use_placeholders = false;
    let prover = Prover::new(config).expect("Failed to create prover");

    let mut state = State::new();
    let mut address_offset = 0;

    // Process multiple blocks sequentially
    for block_id in 1..=5 {
        // Use different addresses for each block to avoid nonce conflicts
        let block = create_test_block_with_offset(block_id, 2, address_offset);
        address_offset += 2; // Increment offset for next block
        let prev_state = state.clone();

        // Apply transactions
        for tx in &block.transactions {
            apply_tx(&mut state, tx, block.timestamp).expect("Failed to apply transaction");
        }

        // Generate proof
        let block_proof = prover
            .prove_block(&block, &prev_state, &state)
            .await
            .expect("Failed to generate proof");

        // Validate each block proof
        assert!(
            !block_proof.zk_proof.is_empty(),
            "Block {} proof should not be empty",
            block_id
        );

        // State root should change after transactions
        if !block.transactions.is_empty() {
            assert_ne!(
                block_proof.prev_state_root, block_proof.new_state_root,
                "Block {} state root should change",
                block_id
            );
        }
    }
}

#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]

async fn test_e2e_proof_consistency() {
    let mut config = ProverConfig::default();
    config.use_placeholders = false;
    let prover = Prover::new(config).expect("Failed to create prover");

    let block = create_test_block(1, 2);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    // Apply transactions
    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    // Generate proof twice - should be consistent
    let proof1 = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .expect("Failed to generate proof 1");

    let proof2 = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .expect("Failed to generate proof 2");

    // Public inputs should match
    assert_eq!(proof1.prev_state_root, proof2.prev_state_root);
    assert_eq!(proof1.new_state_root, proof2.new_state_root);
    assert_eq!(proof1.withdrawals_root, proof2.withdrawals_root);

    // Proofs might differ (non-deterministic), but should both verify
    #[cfg(feature = "arkworks")]
    {
        let public_inputs = bincode::serialize(&(
            proof1.prev_state_root,
            proof1.new_state_root,
            proof1.withdrawals_root,
        ))
        .unwrap();

        let verify1 = prover
            .verify_snark_proof(&proof1.zk_proof, &public_inputs)
            .await
            .expect("Verification 1 should succeed");
        assert!(verify1, "Proof 1 should be valid");

        let verify2 = prover
            .verify_snark_proof(&proof2.zk_proof, &public_inputs)
            .await
            .expect("Verification 2 should succeed");
        assert!(verify2, "Proof 2 should be valid");
    }
}

/// End-to-end test with placeholder provers
/// This tests the full flow structure without requiring real proof generation
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_e2e_flow_structure_with_placeholders() {
    // Use placeholder provers to test flow structure
    let mut config = ProverConfig::default();
    config.use_placeholders = true;
    let prover = Prover::new(config).expect("Failed to create prover");

    // Create block with transactions
    let block = create_test_block(1, 3);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    // Apply transactions
    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    // Generate proof (will use placeholders)
    let result = prover.prove_block(&block, &prev_state, &new_state).await;
    assert!(
        result.is_ok(),
        "Proof generation should succeed with placeholders"
    );

    let block_proof = result.unwrap();

    // Validate proof structure
    assert_ne!(
        block_proof.prev_state_root, block_proof.new_state_root,
        "State roots should be different after transactions"
    );
    assert!(
        !block_proof.zk_proof.is_empty(),
        "ZK proof should not be empty"
    );
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

    // Verify proof (will use placeholders)
    let public_inputs = bincode::serialize(&(
        block_proof.prev_state_root,
        block_proof.new_state_root,
        block_proof.withdrawals_root,
    ))
    .unwrap();

    let verify_result = prover
        .verify_snark_proof(&block_proof.zk_proof, &public_inputs)
        .await;

    assert!(
        verify_result.is_ok(),
        "SNARK proof verification should succeed with placeholders"
    );
    assert!(
        verify_result.unwrap(),
        "SNARK proof should be valid with placeholders"
    );
}

/// Test state root computation consistency
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_e2e_state_root_computation() {
    let mut config = ProverConfig::default();
    config.use_placeholders = true;
    let _prover = Prover::new(config).expect("Failed to create prover");

    let mut state = State::new();
    let block = create_test_block(1, 2);

    // Apply transactions
    for tx in &block.transactions {
        apply_tx(&mut state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    // Compute state roots
    let prev_state_root = Prover::compute_state_root_static(&State::new())
        .expect("Failed to compute prev state root");
    let new_state_root =
        Prover::compute_state_root_static(&state).expect("Failed to compute new state root");

    // State roots should be different after transactions
    assert_ne!(
        prev_state_root, new_state_root,
        "State roots should be different after transactions"
    );

    // State roots should be 32 bytes
    assert_eq!(prev_state_root.len(), 32);
    assert_eq!(new_state_root.len(), 32);
}

/// Test proof serialization/deserialization
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_e2e_proof_serialization() {
    let mut config = ProverConfig::default();
    config.use_placeholders = true;
    let prover = Prover::new(config).expect("Failed to create prover");

    let block = create_test_block(1, 2);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    // Apply transactions
    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    // Generate proof
    let block_proof = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .expect("Failed to generate proof");

    // Serialize proof
    let serialized = bincode::serialize(&block_proof).expect("Failed to serialize proof");

    // Deserialize proof
    let deserialized: BlockProof =
        bincode::deserialize(&serialized).expect("Failed to deserialize proof");

    // Verify deserialized proof matches original
    assert_eq!(block_proof.prev_state_root, deserialized.prev_state_root);
    assert_eq!(block_proof.new_state_root, deserialized.new_state_root);
    assert_eq!(block_proof.withdrawals_root, deserialized.withdrawals_root);
    assert_eq!(block_proof.zk_proof, deserialized.zk_proof);
}

/// Test multiple blocks with state transitions
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_e2e_multiple_blocks_state_transitions() {
    let mut config = ProverConfig::default();
    config.use_placeholders = true;
    let prover = Prover::new(config).expect("Failed to create prover");

    let mut state = State::new();
    let mut prev_state_roots = Vec::new();
    let mut new_state_roots = Vec::new();

    // Process multiple blocks
    // Use different addresses for each block to avoid nonce conflicts
    for block_id in 1..=5 {
        // Create block with addresses offset by block_id to ensure uniqueness
        let block = create_test_block_with_offset(block_id, 2, (block_id * 10) as usize);
        let prev_state = state.clone();
        prev_state_roots.push(
            Prover::compute_state_root_static(&prev_state)
                .expect("Failed to compute prev state root"),
        );

        // Apply transactions
        for tx in &block.transactions {
            apply_tx(&mut state, tx, block.timestamp).expect("Failed to apply transaction");
        }

        new_state_roots.push(
            Prover::compute_state_root_static(&state).expect("Failed to compute new state root"),
        );

        // Generate proof
        let block_proof = prover
            .prove_block(&block, &prev_state, &state)
            .await
            .expect("Failed to generate proof");

        // Validate proof structure
        assert!(
            !block_proof.zk_proof.is_empty(),
            "Block {} proof should not be empty",
            block_id
        );
        assert_eq!(
            block_proof.prev_state_root,
            prev_state_roots[block_id as usize - 1]
        );
        assert_eq!(
            block_proof.new_state_root,
            new_state_roots[block_id as usize - 1]
        );
    }

    // Verify state roots are different for each block
    for i in 1..prev_state_roots.len() {
        assert_ne!(
            prev_state_roots[i],
            prev_state_roots[i - 1],
            "State roots should change between blocks"
        );
    }
}
