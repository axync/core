//! Performance and profiling tests for proof generation

#[cfg(any(feature = "stark", feature = "arkworks"))]
use crate::prover::{Prover, ProverConfig};
#[cfg(any(feature = "stark", feature = "arkworks"))]
use std::time::Instant;
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

async fn test_proof_generation_performance() {
    let mut config = ProverConfig::default();
    config.use_placeholders = false;
    let prover = Prover::new(config).expect("Failed to create prover");

    let block = create_test_block(1, 4);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    // Measure proof generation time
    let start = Instant::now();
    let block_proof = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .expect("Failed to generate proof");
    let duration = start.elapsed();

    println!("Proof generation took: {:?}", duration);
    println!("Proof size: {} bytes", block_proof.zk_proof.len());

    // Performance assertions (adjust thresholds based on requirements)
    assert!(
        duration.as_secs() < 60,
        "Proof generation should complete within 60 seconds"
    );
    assert!(
        !block_proof.zk_proof.is_empty(),
        "Proof should not be empty"
    );
}

#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]

async fn test_proof_size_scaling() {
    let mut config = ProverConfig::default();
    config.use_placeholders = false;
    let prover = Prover::new(config).expect("Failed to create prover");

    // Test with different block sizes
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

        proof_sizes.push((size, block_proof.zk_proof.len()));
        println!(
            "Block size {}: proof size {} bytes",
            size,
            block_proof.zk_proof.len()
        );
    }

    // Validate that proof sizes are reasonable
    for (size, proof_size) in &proof_sizes {
        assert!(
            *proof_size > 0,
            "Proof size should be positive for block size {}",
            size
        );
        assert!(
            *proof_size < 10_000_000,
            "Proof size should be reasonable (< 10MB) for block size {}",
            size
        );
    }
}

#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]

async fn test_proof_generation_time_scaling() {
    let mut config = ProverConfig::default();
    config.use_placeholders = false;
    let prover = Prover::new(config).expect("Failed to create prover");

    // Test with different block sizes
    let sizes = vec![1, 2, 4];
    let mut timings = Vec::new();

    for size in sizes {
        let block = create_test_block(size as u64, size);
        let prev_state = State::new();
        let mut new_state = prev_state.clone();

        for tx in &block.transactions {
            apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
        }

        let start = Instant::now();
        let _block_proof = prover
            .prove_block(&block, &prev_state, &new_state)
            .await
            .expect(&format!("Failed to generate proof for size {}", size));
        let duration = start.elapsed();

        timings.push((size, duration));
        println!("Block size {}: proof generation took {:?}", size, duration);
    }

    // Validate that timings are reasonable
    for (size, duration) in &timings {
        assert!(
            duration.as_secs() < 120,
            "Proof generation should complete within 120 seconds for block size {}",
            size
        );
    }
}

#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]

async fn test_verification_performance() {
    let mut config = ProverConfig::default();
    config.use_placeholders = false;
    let prover = Prover::new(config).expect("Failed to create prover");

    let block = create_test_block(1, 2);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    // Generate proof
    let block_proof = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .expect("Failed to generate proof");

    // Measure verification time
    #[cfg(feature = "arkworks")]
    {
        let public_inputs = bincode::serialize(&(
            block_proof.prev_state_root,
            block_proof.new_state_root,
            block_proof.withdrawals_root,
        ))
        .unwrap();

        let start = Instant::now();
        let verify_result = prover
            .verify_snark_proof(&block_proof.zk_proof, &public_inputs)
            .await
            .expect("Verification should succeed");
        let duration = start.elapsed();

        println!("Proof verification took: {:?}", duration);
        assert!(verify_result, "Proof should be valid");
        assert!(
            duration.as_secs() < 10,
            "Verification should complete within 10 seconds"
        );
    }
}

/// Performance test with placeholder provers
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_proof_generation_performance_placeholders() {
    let mut config = ProverConfig::default();
    config.use_placeholders = true;
    let prover = Prover::new(config).expect("Failed to create prover");

    let block = create_test_block(1, 4);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    // Measure proof generation time
    let start = Instant::now();
    let block_proof = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .expect("Failed to generate proof");
    let duration = start.elapsed();

    println!("Proof generation (placeholders) took: {:?}", duration);
    println!("Proof size: {} bytes", block_proof.zk_proof.len());

    // Performance assertions
    assert!(
        duration.as_millis() < 1000,
        "Proof generation with placeholders should be fast (< 1s)"
    );
    assert!(
        !block_proof.zk_proof.is_empty(),
        "Proof should not be empty"
    );
}

/// Test proof size scaling with placeholders
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_proof_size_scaling_placeholders() {
    let mut config = ProverConfig::default();
    config.use_placeholders = true;
    let prover = Prover::new(config).expect("Failed to create prover");

    // Test with different block sizes
    let sizes = vec![1, 2, 4, 8, 16];
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

        proof_sizes.push((size, block_proof.zk_proof.len()));
        println!(
            "Block size {}: proof size {} bytes",
            size,
            block_proof.zk_proof.len()
        );
    }

    // Validate that proof sizes are reasonable
    for (size, proof_size) in &proof_sizes {
        assert!(
            *proof_size > 0,
            "Proof size should be positive for block size {}",
            size
        );
    }

    // Print summary
    println!("\nProof size scaling summary:");
    for (size, proof_size) in &proof_sizes {
        println!("  Block size {}: {} bytes", size, proof_size);
    }
}

/// Test proof generation time scaling with placeholders
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_proof_generation_time_scaling_placeholders() {
    let mut config = ProverConfig::default();
    config.use_placeholders = true;
    let prover = Prover::new(config).expect("Failed to create prover");

    // Test with different block sizes
    let sizes = vec![1, 2, 4, 8, 16];
    let mut timings = Vec::new();

    for size in sizes {
        let block = create_test_block(size as u64, size);
        let prev_state = State::new();
        let mut new_state = prev_state.clone();

        for tx in &block.transactions {
            apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
        }

        let start = Instant::now();
        let _block_proof = prover
            .prove_block(&block, &prev_state, &new_state)
            .await
            .expect(&format!("Failed to generate proof for size {}", size));
        let duration = start.elapsed();

        timings.push((size, duration));
        println!("Block size {}: proof generation took {:?}", size, duration);
    }

    // Print summary
    println!("\nProof generation time scaling summary:");
    for (size, duration) in &timings {
        println!("  Block size {}: {:?}", size, duration);
    }

    // Validate that timings are reasonable for placeholders
    for (size, duration) in &timings {
        assert!(
            duration.as_millis() < 1000,
            "Proof generation with placeholders should be fast (< 1s) for block size {}",
            size
        );
    }
}

/// Test verification performance with placeholders
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_verification_performance_placeholders() {
    use bincode;
    let mut config = ProverConfig::default();
    config.use_placeholders = true;
    let prover = Prover::new(config).expect("Failed to create prover");

    let block = create_test_block(1, 2);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    // Generate proof
    let block_proof = prover
        .prove_block(&block, &prev_state, &new_state)
        .await
        .expect("Failed to generate proof");

    // Measure verification time
    let public_inputs = bincode::serialize(&(
        block_proof.prev_state_root,
        block_proof.new_state_root,
        block_proof.withdrawals_root,
    ))
    .unwrap();

    let start = Instant::now();
    let verify_result = prover
        .verify_snark_proof(&block_proof.zk_proof, &public_inputs)
        .await
        .expect("Verification should succeed");
    let duration = start.elapsed();

    println!("Proof verification (placeholders) took: {:?}", duration);
    assert!(verify_result, "Proof should be valid");
    assert!(
        duration.as_millis() < 1000,
        "Verification with placeholders should be fast (< 1s)"
    );
}

/// Test state root computation performance
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_state_root_computation_performance() {
    let sizes = vec![1, 2, 4, 8, 16, 32];
    let mut timings = Vec::new();

    for size in sizes {
        let mut state = State::new();
        let block = create_test_block(size as u64, size);

        // Apply transactions
        for tx in &block.transactions {
            apply_tx(&mut state, tx, block.timestamp).expect("Failed to apply transaction");
        }

        // Measure state root computation time
        let start = Instant::now();
        let _root =
            Prover::compute_state_root_static(&state).expect("Failed to compute state root");
        let duration = start.elapsed();

        timings.push((size, duration));
        println!(
            "State root computation for {} accounts: {:?}",
            size, duration
        );
    }

    // Print summary
    println!("\nState root computation performance summary:");
    for (size, duration) in &timings {
        println!("  {} accounts: {:?}", size, duration);
    }

    // Validate that timings are reasonable
    for (size, duration) in &timings {
        assert!(
            duration.as_millis() < 100,
            "State root computation should be fast (< 100ms) for {} accounts",
            size
        );
    }
}

/// Detailed profiling of proof generation stages
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_detailed_proof_generation_profiling() {
    let mut config = ProverConfig::default();
    config.use_placeholders = false;
    let prover = Prover::new(config).expect("Failed to create prover");

    let block = create_test_block(1, 4);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    // Profile state root computation
    let start = Instant::now();
    let prev_state_root = Prover::compute_state_root_static(&prev_state)
        .expect("Failed to compute prev root");
    let new_state_root = Prover::compute_state_root_static(&new_state)
        .expect("Failed to compute new root");
    let withdrawals_root = prover
        .compute_withdrawals_root(&block)
        .expect("Failed to compute withdrawals root");
    let state_root_time = start.elapsed();

    // Profile STARK proof generation
    let block_data = bincode::serialize(&block).expect("Failed to serialize block");
    let stark_start = Instant::now();
    let stark_proof = prover
        .stark_prover()
        .prove_block_transition(
            &prev_state_root,
            &new_state_root,
            &withdrawals_root,
            &block_data,
        )
        .await
        .expect("Failed to generate STARK proof");
    let stark_time = stark_start.elapsed();

    // Profile SNARK proof generation
    let public_inputs = bincode::serialize(&(prev_state_root, new_state_root, withdrawals_root))
        .expect("Failed to serialize public inputs");
    let snark_start = Instant::now();
    let snark_proof = prover
        .snark_prover()
        .wrap_stark_in_snark(&stark_proof, &public_inputs)
        .await
        .expect("Failed to generate SNARK proof");
    let snark_time = snark_start.elapsed();

    // Total time
    let total_time = state_root_time + stark_time + snark_time;

    println!("\nDetailed proof generation profiling:");
    println!("  State root computation: {:?} ({:.2}%)", 
        state_root_time, 
        state_root_time.as_secs_f64() / total_time.as_secs_f64() * 100.0
    );
    println!("  STARK proof generation: {:?} ({:.2}%)", 
        stark_time, 
        stark_time.as_secs_f64() / total_time.as_secs_f64() * 100.0
    );
    println!("  SNARK proof generation: {:?} ({:.2}%)", 
        snark_time, 
        snark_time.as_secs_f64() / total_time.as_secs_f64() * 100.0
    );
    println!("  Total time: {:?}", total_time);
    println!("  STARK proof size: {} bytes", stark_proof.len());
    println!("  SNARK proof size: {} bytes", snark_proof.len());

    // Validate performance
    assert!(
        total_time.as_secs() < 60,
        "Total proof generation should complete within 60 seconds"
    );
}

/// Test multiple proof generations for consistency
#[cfg(any(feature = "stark", feature = "arkworks"))]
#[tokio::test]
async fn test_multiple_proof_generations_performance() {
    let mut config = ProverConfig::default();
    config.use_placeholders = true;
    let prover = Prover::new(config).expect("Failed to create prover");

    let block = create_test_block(1, 4);
    let prev_state = State::new();
    let mut new_state = prev_state.clone();

    for tx in &block.transactions {
        apply_tx(&mut new_state, tx, block.timestamp).expect("Failed to apply transaction");
    }

    // Generate multiple proofs and measure average time
    let num_iterations = 10;
    let mut timings = Vec::new();

    for i in 0..num_iterations {
        let start = Instant::now();
        let _block_proof = prover
            .prove_block(&block, &prev_state, &new_state)
            .await
            .expect(&format!("Failed to generate proof iteration {}", i));
        let duration = start.elapsed();
        timings.push(duration);
    }

    // Calculate statistics
    let total: u128 = timings.iter().map(|d| d.as_millis()).sum();
    let average = total / num_iterations as u128;
    let min = timings.iter().min().unwrap();
    let max = timings.iter().max().unwrap();

    println!("\nMultiple proof generations performance:");
    println!("  Iterations: {}", num_iterations);
    println!("  Average time: {} ms", average);
    println!("  Min time: {:?}", min);
    println!("  Max time: {:?}", max);

    // Validate consistency
    let variance = timings
        .iter()
        .map(|d| {
            let diff = d.as_millis() as i128 - average as i128;
            diff * diff
        })
        .sum::<i128>()
        / num_iterations as i128;

    println!("  Variance: {} ms²", variance);
    assert!(
        variance < 1000,
        "Proof generation should be consistent (variance < 1000 ms²)"
    );
}
