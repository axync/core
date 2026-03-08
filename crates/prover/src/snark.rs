use crate::error::ProverError;

/// SNARK proof generator trait
///
/// This trait allows for different SNARK implementations (Plonky2, Groth16, etc.)
#[async_trait::async_trait]
pub trait SnarkProver: Send + Sync {
    /// Wrap a STARK proof in a SNARK proof for on-chain verification
    ///
    /// This takes a STARK proof and wraps it in a SNARK to make it more compact
    /// for on-chain verification
    async fn wrap_stark_in_snark(
        &self,
        stark_proof: &[u8],
        public_inputs: &[u8],
    ) -> Result<Vec<u8>, ProverError>;

    /// Verify a SNARK proof
    async fn verify_snark_proof(
        &self,
        proof: &[u8],
        public_inputs: &[u8],
    ) -> Result<bool, ProverError>;
}

/// Placeholder SNARK prover implementation
///
/// This is a placeholder implementation used when:
/// - `use_placeholders=true` in ProverConfig (for testing)
/// - `arkworks` feature is not enabled
///
/// In production, use `ArkworksSnarkProver` by enabling the `arkworks` feature
/// and setting `use_placeholders=false`.
pub struct PlaceholderSnarkProver;

#[async_trait::async_trait]
impl SnarkProver for PlaceholderSnarkProver {
    async fn wrap_stark_in_snark(
        &self,
        _stark_proof: &[u8],
        _public_inputs: &[u8],
    ) -> Result<Vec<u8>, ProverError> {
        // Placeholder implementation: returns a dummy proof immediately
        // This is intentional for testing/development when real proof generation is not needed
        Ok(b"SNARK_PROOF_PLACEHOLDER".to_vec())
    }

    async fn verify_snark_proof(
        &self,
        _proof: &[u8],
        _public_inputs: &[u8],
    ) -> Result<bool, ProverError> {
        // Placeholder implementation: always returns true
        // This is intentional for testing/development when real proof verification is not needed
        Ok(true)
    }
}

/// Arkworks Groth16-based SNARK prover
///
/// This uses Arkworks Groth16 for generating SNARK proofs that wrap STARK proofs
/// for compact on-chain verification
///
/// Arkworks is a popular, stable library that works on stable Rust and is widely used
/// in production systems. Groth16 is a proven SNARK system with efficient on-chain verification.
#[cfg(feature = "arkworks")]
pub struct ArkworksSnarkProver {
    key_manager: crate::keys::KeyManager,
}

#[cfg(feature = "arkworks")]
impl ArkworksSnarkProver {
    /// Create a new Arkworks SNARK prover
    ///
    /// This will load existing keys from disk, or generate new ones if they don't exist.
    ///
    /// # Arguments
    /// * `keys_dir` - Optional path to directory for storing keys. Defaults to `./keys`
    /// * `force_regenerate` - If true, regenerate keys even if they exist
    pub fn new(
        keys_dir: Option<std::path::PathBuf>,
        force_regenerate: bool,
    ) -> Result<Self, crate::error::ProverError> {
        let mut key_manager = crate::keys::KeyManager::new(keys_dir);
        key_manager.load_or_generate(force_regenerate)?;

        Ok(Self { key_manager })
    }
}

/// Simplified SNARK prover for MVP (works without arkworks feature)
///
/// This creates a structured wrapper that can be replaced with real Arkworks
/// when the arkworks feature is enabled
pub struct SimplifiedSnarkProver {
    // Configuration
}

impl SimplifiedSnarkProver {
    pub fn new() -> Self {
        Self {}
    }
}

#[cfg(feature = "arkworks")]
#[async_trait::async_trait]
impl SnarkProver for ArkworksSnarkProver {
    async fn wrap_stark_in_snark(
        &self,
        stark_proof: &[u8],
        public_inputs: &[u8],
    ) -> Result<Vec<u8>, ProverError> {
        use crate::circuit::StarkProofVerifierCircuit;
        use ark_bn254::Bn254;
        use ark_groth16::Groth16;
        use ark_snark::SNARK;
        use ark_std::rand::rngs::StdRng;
        use ark_std::rand::SeedableRng;

        // Parse public inputs
        if public_inputs.len() < 96 {
            return Err(ProverError::SnarkProof(format!(
                "Invalid public inputs length: expected at least 96 bytes, got {}",
                public_inputs.len()
            )));
        }

        // For minimal STARK prover, we don't need deserialized proof structure
        // The circuit will verify the proof structure directly from bytes

        // Get proving key (pre-computed and loaded)
        let pk = self.key_manager.proving_key()?;

        // Create witness (circuit with all values assigned)
        // CRITICAL: The circuit structure must match exactly what was used for key generation
        // - public_inputs: always 96 bytes (3 * 32 bytes for roots)
        // - stark_proof: always exactly 200 bytes (will be padded/truncated to match)
        let min_proof_size = 200; // Same as in keys.rs
        let mut padded_stark_proof = stark_proof.to_vec();
        if padded_stark_proof.len() < min_proof_size {
            padded_stark_proof.resize(min_proof_size, 0);
        } else if padded_stark_proof.len() > min_proof_size {
            // Truncate to match key generation size
            padded_stark_proof.truncate(min_proof_size);
        }
        
        // Ensure public_inputs is exactly 96 bytes
        let mut normalized_public_inputs = public_inputs.to_vec();
        if normalized_public_inputs.len() < 96 {
            normalized_public_inputs.resize(96, 0);
        } else if normalized_public_inputs.len() > 96 {
            normalized_public_inputs.truncate(96);
        }
        
        let circuit_with_witness = StarkProofVerifierCircuit {
            public_inputs: normalized_public_inputs,
            stark_proof: padded_stark_proof,
        };

        // Use deterministic RNG for proof generation
        // In production, this should use secure randomness
        let mut seed = [0u8; 32];
        seed[0..8].copy_from_slice(
            &(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .to_le_bytes()),
        );
        let mut rng = StdRng::from_seed(seed);

        // Generate proof using pre-computed proving key
        // This can take 10-30 seconds depending on circuit complexity
        let proof = Groth16::<Bn254>::prove(pk, circuit_with_witness, &mut rng).map_err(|e| {
            ProverError::SnarkProof(format!("Failed to generate Groth16 proof: {:?}", e))
        })?;

        // Serialize proof and public inputs using ark-serialize
        use ark_serialize::{CanonicalSerialize, Compress};

        // Serialize proof
        let mut proof_bytes = Vec::new();
        proof
            .serialize_with_mode(&mut proof_bytes, Compress::Yes)
            .map_err(|e| {
                ProverError::Serialization(format!("Failed to serialize Groth16 proof: {}", e))
            })?;

        // For MVP, we'll serialize both proof and public inputs
        // In production, verifying key should be stored separately
        #[derive(serde::Serialize, serde::Deserialize)]
        struct SnarkProofWrapper {
            proof: Vec<u8>,
            public_inputs: Vec<u8>,
            version: u8,
        }

        let wrapper = SnarkProofWrapper {
            proof: proof_bytes,
            public_inputs: public_inputs.to_vec(),
            version: 3, // Version 3 for actual Groth16 proof
        };

        bincode::serialize(&wrapper).map_err(|e| {
            ProverError::Serialization(format!("Failed to serialize SNARK wrapper: {}", e))
        })
    }

    async fn verify_snark_proof(
        &self,
        proof: &[u8],
        public_inputs: &[u8],
    ) -> Result<bool, ProverError> {
        use ark_bn254::Bn254;
        use ark_groth16::Groth16;
        use ark_serialize::CanonicalDeserialize;
        use ark_snark::SNARK;

        // Deserialize wrapper
        #[derive(serde::Serialize, serde::Deserialize)]
        struct SnarkProofWrapper {
            proof: Vec<u8>,
            public_inputs: Vec<u8>,
            version: u8,
        }

        let wrapper: SnarkProofWrapper = bincode::deserialize(proof).map_err(|e| {
            ProverError::Serialization(format!("Failed to deserialize SNARK wrapper: {}", e))
        })?;

        // Verify version
        if wrapper.version != 3 {
            return Ok(false);
        }

        // Verify public inputs match
        if wrapper.public_inputs != public_inputs {
            return Ok(false);
        }

        // Deserialize Groth16 proof
        let groth16_proof = ark_groth16::Proof::<Bn254>::deserialize_with_mode(
            &wrapper.proof[..],
            ark_serialize::Compress::Yes,
            ark_serialize::Validate::Yes,
        )
        .map_err(|e| {
            ProverError::Serialization(format!("Failed to deserialize Groth16 proof: {}", e))
        })?;

        // Get verifying key (pre-computed and loaded)
        let vk = self.key_manager.verifying_key()?;

        // Convert public inputs to field elements
        // Each 32-byte root = 8 field elements (4 bytes each)
        // Total: 3 roots * 8 elements = 24 field elements
        if public_inputs.len() < 96 {
            return Err(ProverError::SnarkProof(format!(
                "Invalid public inputs length: expected at least 96 bytes, got {}",
                public_inputs.len()
            )));
        }

        let mut public_inputs_elements = Vec::new();
        // Process each root (32 bytes = 8 u32 values)
        for root_idx in 0..3 {
            let root_start = root_idx * 32;
            for i in 0..8 {
                let byte_start = root_start + (i * 4);
                let chunk = &public_inputs[byte_start..byte_start + 4];
                let value = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                public_inputs_elements.push(ark_bn254::Fr::from(value as u64));
            }
        }

        // Ensure we have exactly 24 elements
        if public_inputs_elements.len() != 24 {
            return Err(ProverError::SnarkProof(format!(
                "Invalid public inputs elements count: expected 24, got {}",
                public_inputs_elements.len()
            )));
        }

        // Check that verifying key has correct number of public inputs
        // gamma_abc_g1 should have length = num_public_inputs + 1
        // We have 24 public inputs, so gamma_abc_g1 should have length 25
        let expected_gamma_abc_len = public_inputs_elements.len() + 1;
        if vk.gamma_abc_g1.len() != expected_gamma_abc_len {
            return Err(ProverError::SnarkProof(format!(
                "Verifying key has incorrect number of public inputs: expected {} ({} + 1), got {}",
                expected_gamma_abc_len,
                public_inputs_elements.len(),
                vk.gamma_abc_g1.len()
            )));
        }

        // Verify proof
        let is_valid = Groth16::<Bn254>::verify(&vk, &public_inputs_elements, &groth16_proof)
            .map_err(|e| {
                ProverError::SnarkProof(format!("Groth16 verification failed: {:?}", e))
            })?;

        Ok(is_valid)
    }
}

#[async_trait::async_trait]
impl SnarkProver for SimplifiedSnarkProver {
    async fn wrap_stark_in_snark(
        &self,
        stark_proof: &[u8],
        public_inputs: &[u8],
    ) -> Result<Vec<u8>, ProverError> {
        // Simplified wrapper for MVP (when arkworks feature is not enabled)
        #[derive(serde::Serialize, serde::Deserialize)]
        struct SnarkProofWrapper {
            stark_proof: Vec<u8>,
            public_inputs: Vec<u8>,
            version: u8,
            metadata: SnarkMetadata,
        }

        #[derive(serde::Serialize, serde::Deserialize)]
        struct SnarkMetadata {
            stark_proof_size: u32,
            public_inputs_size: u32,
            timestamp: u64,
        }

        let wrapper = SnarkProofWrapper {
            stark_proof: stark_proof.to_vec(),
            public_inputs: public_inputs.to_vec(),
            version: 1,
            metadata: SnarkMetadata {
                stark_proof_size: stark_proof.len() as u32,
                public_inputs_size: public_inputs.len() as u32,
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            },
        };

        bincode::serialize(&wrapper).map_err(|e| {
            ProverError::Serialization(format!("Failed to serialize SNARK wrapper: {}", e))
        })
    }

    async fn verify_snark_proof(
        &self,
        proof: &[u8],
        public_inputs: &[u8],
    ) -> Result<bool, ProverError> {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct SnarkProofWrapper {
            stark_proof: Vec<u8>,
            public_inputs: Vec<u8>,
            version: u8,
            metadata: SnarkMetadata,
        }

        #[derive(serde::Serialize, serde::Deserialize)]
        struct SnarkMetadata {
            stark_proof_size: u32,
            public_inputs_size: u32,
            timestamp: u64,
        }

        let wrapper: SnarkProofWrapper = bincode::deserialize(proof).map_err(|e| {
            ProverError::Serialization(format!("Failed to deserialize SNARK wrapper: {}", e))
        })?;

        if wrapper.version != 1 {
            return Ok(false);
        }

        if wrapper.stark_proof.len() != wrapper.metadata.stark_proof_size as usize {
            return Ok(false);
        }
        if wrapper.public_inputs.len() != wrapper.metadata.public_inputs_size as usize {
            return Ok(false);
        }

        if wrapper.public_inputs != public_inputs {
            return Ok(false);
        }

        Ok(true)
    }
}
