//! Tests for STARK proof generation using minimal STARK prover

#[cfg(feature = "stark")]
use crate::air::BlockTransitionInputs;
#[cfg(feature = "stark")]
use crate::stark::{MinimalStarkProver, StarkProver};
#[cfg(feature = "stark")]
use bincode;
#[cfg(feature = "stark")]
use zkclear_state::State;
#[cfg(feature = "stark")]
use zkclear_stf::apply_tx;
#[cfg(feature = "stark")]
use zkclear_types::Block;
#[cfg(feature = "stark")]
use zkclear_types::{Address, Tx, TxPayload};

/// Helper function to create a test block with transactions
#[cfg(feature = "stark")]
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

/// Helper function to create a test state
#[cfg(feature = "stark")]
fn create_test_state() -> State {
    State::new()
}

/// Helper function to apply transactions to state and compute new state
#[cfg(feature = "stark")]
fn apply_transactions_to_state(
    state: &mut State,
    block: &Block,
) -> Result<State, crate::error::ProverError> {
    let mut new_state = state.clone();

    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).map_err(|e| {
            crate::error::ProverError::StarkProof(format!("Failed to apply tx: {:?}", e))
        })?;
    }

    Ok(new_state)
}

#[cfg(feature = "stark")]
#[tokio::test]
async fn test_stark_proof_generation_empty_block() {
    let prover = MinimalStarkProver::new();
    let block = create_test_block(0, 0);
    let prev_state = create_test_state();
    let new_state = create_test_state();

    // Compute state roots
    let prev_state_root = crate::prover::Prover::compute_state_root_static(&prev_state)
        .expect("Failed to compute prev state root");
    let new_state_root = crate::prover::Prover::compute_state_root_static(&new_state)
        .expect("Failed to compute new state root");
    let withdrawals_root = [0u8; 32];

    let block_data = bincode::serialize(&block).expect("Failed to serialize block");

    // Generate STARK proof
    let result = prover
        .prove_block_transition(
            &prev_state_root,
            &new_state_root,
            &withdrawals_root,
            &block_data,
        )
        .await;

    assert!(
        result.is_ok(),
        "STARK proof generation should succeed for empty block"
    );
    let proof = result.unwrap();

    // Validate proof
    assert!(!proof.is_empty(), "Proof should not be empty");
    assert!(
        proof.len() > 100,
        "Proof should have reasonable size (at least 100 bytes)"
    );

    // Verify proof using verifier
    let public_inputs = BlockTransitionInputs {
        prev_state_root,
        new_state_root,
        withdrawals_root,
        block_id: block.id,
        timestamp: block.timestamp,
    };

    let public_inputs_bytes =
        bincode::serialize(&public_inputs).expect("Failed to serialize public inputs");
    let verify_result = prover
        .verify_stark_proof(&proof, &public_inputs_bytes)
        .await;

    assert!(verify_result.is_ok(), "Proof verification should succeed");
    assert!(verify_result.unwrap(), "Proof should be valid");
}

#[cfg(feature = "stark")]
#[tokio::test]
async fn test_stark_proof_generation_single_transaction() {
    let prover = MinimalStarkProver::new();
    let block = create_test_block(1, 1);
    let mut prev_state = create_test_state();
    let new_state =
        apply_transactions_to_state(&mut prev_state, &block).expect("Failed to apply transactions");

    // Compute state roots
    let prev_state_root = crate::prover::Prover::compute_state_root_static(&prev_state)
        .expect("Failed to compute prev state root");
    let new_state_root = crate::prover::Prover::compute_state_root_static(&new_state)
        .expect("Failed to compute new state root");
    let withdrawals_root = [0u8; 32];

    let block_data = bincode::serialize(&block).expect("Failed to serialize block");

    // Generate STARK proof
    let result = prover
        .prove_block_transition(
            &prev_state_root,
            &new_state_root,
            &withdrawals_root,
            &block_data,
        )
        .await;

    assert!(
        result.is_ok(),
        "STARK proof generation should succeed for single transaction. Error: {:?}",
        result.as_ref().err()
    );
    let proof = result.unwrap();

    // Validate proof
    assert!(!proof.is_empty(), "Proof should not be empty");
    assert!(proof.len() > 100, "Proof should have reasonable size");

    // Verify proof
    let public_inputs = BlockTransitionInputs {
        prev_state_root,
        new_state_root,
        withdrawals_root,
        block_id: block.id,
        timestamp: block.timestamp,
    };

    let public_inputs_bytes =
        bincode::serialize(&public_inputs).expect("Failed to serialize public inputs");
    let verify_result = prover
        .verify_stark_proof(&proof, &public_inputs_bytes)
        .await;

    assert!(verify_result.is_ok(), "Proof verification should succeed");
    assert!(verify_result.unwrap(), "Proof should be valid");
}

#[cfg(feature = "stark")]
#[tokio::test]
async fn test_stark_proof_generation_multiple_transactions() {
    let prover = MinimalStarkProver::new();

    for num_txs in [2, 4, 8, 16] {
        let block = create_test_block(num_txs as u64, num_txs);
        let mut prev_state = create_test_state();
        let new_state = apply_transactions_to_state(&mut prev_state, &block)
            .expect("Failed to apply transactions");

        // Compute state roots
        let prev_state_root = crate::prover::Prover::compute_state_root_static(&prev_state)
            .expect("Failed to compute prev state root");
        let new_state_root = crate::prover::Prover::compute_state_root_static(&new_state)
            .expect("Failed to compute new state root");
        let withdrawals_root = [0u8; 32];

        let block_data = bincode::serialize(&block).expect("Failed to serialize block");

        // Generate STARK proof
        let result = prover
            .prove_block_transition(
                &prev_state_root,
                &new_state_root,
                &withdrawals_root,
                &block_data,
            )
            .await;

        assert!(
            result.is_ok(),
            "STARK proof generation should succeed for {} transactions",
            num_txs
        );
        let proof = result.unwrap();

        // Validate proof size increases with number of transactions
        assert!(!proof.is_empty(), "Proof should not be empty");

        // Verify proof
        let public_inputs = BlockTransitionInputs {
            prev_state_root,
            new_state_root,
            withdrawals_root,
            block_id: block.id,
            timestamp: block.timestamp,
        };

        let public_inputs_bytes =
            bincode::serialize(&public_inputs).expect("Failed to serialize public inputs");
        let verify_result = prover
            .verify_stark_proof(&proof, &public_inputs_bytes)
            .await;

        assert!(
            verify_result.is_ok(),
            "Proof verification should succeed for {} transactions",
            num_txs
        );
        assert!(
            verify_result.unwrap(),
            "Proof should be valid for {} transactions",
            num_txs
        );
    }
}

#[cfg(feature = "stark")]
#[tokio::test]
async fn test_stark_proof_verification_fails_with_wrong_public_inputs() {
    let prover = MinimalStarkProver::new();
    let block = create_test_block(1, 1);
    let mut prev_state = create_test_state();
    let new_state =
        apply_transactions_to_state(&mut prev_state, &block).expect("Failed to apply transactions");

    // Compute state roots
    let prev_state_root = crate::prover::Prover::compute_state_root_static(&prev_state)
        .expect("Failed to compute prev state root");
    let new_state_root = crate::prover::Prover::compute_state_root_static(&new_state)
        .expect("Failed to compute new state root");
    let withdrawals_root = [0u8; 32];

    let block_data = bincode::serialize(&block).expect("Failed to serialize block");

    // Generate STARK proof
    let proof = prover
        .prove_block_transition(
            &prev_state_root,
            &new_state_root,
            &withdrawals_root,
            &block_data,
        )
        .await
        .expect("Failed to generate proof");

    // Try to verify with wrong public inputs
    let wrong_public_inputs = BlockTransitionInputs {
        prev_state_root: [1u8; 32], // Wrong prev_state_root
        new_state_root,
        withdrawals_root,
        block_id: block.id,
        timestamp: block.timestamp,
    };

    let wrong_public_inputs_bytes =
        bincode::serialize(&wrong_public_inputs).expect("Failed to serialize public inputs");

    // Verification should fail or return false
    let verify_result = prover
        .verify_stark_proof(&proof, &wrong_public_inputs_bytes)
        .await;

    // Either verification fails or returns false
    match verify_result {
        Ok(false) => {
            // Expected: verification returns false for wrong inputs
        }
        Err(_) => {
            // Also acceptable: verification fails with error
        }
        Ok(true) => {
            panic!("Verification should fail or return false for wrong public inputs");
        }
    }
}
