//! Tests for SNARK wrapping and verification

#[cfg(feature = "arkworks")]
use crate::circuit::StarkProofVerifierCircuit;
#[cfg(feature = "arkworks")]
use crate::snark::{ArkworksSnarkProver, SnarkProver};

#[cfg(feature = "arkworks")]
#[tokio::test]
async fn test_snark_wrap_stark_proof() {
    let prover = ArkworksSnarkProver::new(None, false).expect("Failed to create SNARK prover");

    // Create a dummy STARK proof
    let stark_proof = b"STARK_PROOF_TEST_DATA".repeat(10);
    let public_inputs = vec![0u8; 96]; // 3 * 32 bytes for roots

    // Wrap STARK proof in SNARK
    let result = prover
        .wrap_stark_in_snark(&stark_proof, &public_inputs)
        .await;

    assert!(result.is_ok(), "SNARK wrapping should succeed");
    let snark_proof = result.unwrap();

    // Validate SNARK proof
    assert!(!snark_proof.is_empty(), "SNARK proof should not be empty");
}

#[cfg(feature = "arkworks")]
#[tokio::test]
async fn test_snark_verify_proof() {
    let prover = ArkworksSnarkProver::new(None, false).expect("Failed to create SNARK prover");

    // Create a dummy STARK proof
    let stark_proof = b"STARK_PROOF_TEST_DATA".repeat(10);
    let public_inputs = vec![0u8; 96];

    // Wrap and verify
    let snark_proof = prover
        .wrap_stark_in_snark(&stark_proof, &public_inputs)
        .await
        .expect("Failed to wrap proof");

    let verify_result = prover
        .verify_snark_proof(&snark_proof, &public_inputs)
        .await;

    assert!(verify_result.is_ok(), "SNARK verification should succeed");
    assert!(verify_result.unwrap(), "SNARK proof should be valid");
}

#[cfg(feature = "arkworks")]
#[tokio::test]
async fn test_snark_verify_fails_with_wrong_inputs() {
    let prover = ArkworksSnarkProver::new(None, false).expect("Failed to create SNARK prover");

    let stark_proof = b"STARK_PROOF_TEST_DATA".repeat(10);
    let public_inputs = vec![0u8; 96];

    // Generate proof with original inputs
    let snark_proof = prover
        .wrap_stark_in_snark(&stark_proof, &public_inputs)
        .await
        .expect("Failed to wrap proof");

    // Try to verify with wrong public inputs
    let wrong_inputs = vec![1u8; 96];
    let verify_result = prover.verify_snark_proof(&snark_proof, &wrong_inputs).await;

    // Verification should fail or return false
    match verify_result {
        Ok(false) => {
            // Expected: verification returns false
        }
        Err(_) => {
            // Also acceptable: verification fails with error
        }
        Ok(true) => {
            panic!("Verification should fail or return false for wrong inputs");
        }
    }
}

#[cfg(feature = "arkworks")]
#[tokio::test]
async fn test_snark_circuit_constraints() {
    use ark_bn254::Fr;
    use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystem};

    // Create circuit with test data
    let circuit = StarkProofVerifierCircuit {
        public_inputs: vec![0u8; 96],
        stark_proof: b"TEST_PROOF".to_vec(),
    };

    // Generate constraints
    let cs = ConstraintSystem::<Fr>::new_ref();
    let result = circuit.generate_constraints(cs.clone());

    assert!(result.is_ok(), "Constraint generation should succeed");

    // Check that constraints were added
    assert!(cs.num_constraints() > 0, "Should have constraints");
}
